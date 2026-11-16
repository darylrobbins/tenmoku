//! The Debug tool window (`debug_panel.rs`, card D9) against fe-dap's fake
//! adapter, in a headless workbench (D8's harness, [`Debug`]): lazy
//! expansion and paging, frame selection re-scoping Variables and Watch,
//! watches kept in the state file, exception filters re-sent, the rail's
//! Debug button and the tool window's toggle, and accessible names.

use std::{fs, path::Path};

use fe_dap::fake::{FakeAdapter, Fault, FaultKind, Local, Script};
use fe_editor::decorations::Tint;
use fe_ui::{Icon, Role};
use gpui::{Entity, Focusable as _, TestAppContext};
use kiln_l10n::tr;
use kiln_shell::{DockView as _, PanelId};
use serde_json::{Value, json};

use super::debug_commands::{Debug, main_path, requests, script, spinning, trusted};
use crate::{
    debug_breakpoints::{BreakpointStore, WRITE_REST, set_state_dir, state_file},
    debug_panel::{DebugPanel, Part, StackRow, Tab, ValueRow},
};

const DEBUG: PanelId = PanelId(crate::dock::DEBUG_BUTTON);

fn panel(debug: &mut Debug) -> Entity<DebugPanel> {
    debug
        .workbench
        .read_with(debug.cx, |workbench, _| workbench.debug_panel().clone())
}

fn value_rows(debug: &mut Debug, part: Part) -> Vec<ValueRow> {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, _| match part {
        Part::Watch => panel.watch_rows().0,
        _ => panel.variable_rows().0,
    })
}

fn stack_rows(debug: &mut Debug) -> Vec<StackRow> {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, cx| panel.stack_rows(cx))
}

/// Each Variables row as "name = value", indented two spaces a level, or
/// its name for the others.
fn shown(rows: &[ValueRow]) -> Vec<String> {
    (rows.iter())
        .map(|row| match row {
            ValueRow::Variable {
                depth, name, value, ..
            } => format!("{}{name} = {}", "  ".repeat(*depth), value.text),
            ValueRow::Watch {
                expression, value, ..
            } => match value {
                Some(Ok(value)) => format!("{expression} = {}", value.text),
                Some(Err(error)) => format!("{expression} ✕ {error}"),
                None => expression.clone(),
            },
            ValueRow::Scope { name, .. } => name.clone(),
            ValueRow::More { remaining, .. } => format!("more {remaining}"),
            ValueRow::Message { text, .. } => text.to_string(),
        })
        .collect()
}

/// The index of the Variables (or Watch) row named `name` at `depth`.
fn row_named(debug: &mut Debug, part: Part, name: &str) -> usize {
    let rows = value_rows(debug, part);
    (rows.iter())
        .position(|row| match row {
            ValueRow::Variable { name: have, .. } => have == name,
            ValueRow::Scope { name: have, .. } => have == name,
            ValueRow::Watch { expression, .. } => expression == name,
            _ => false,
        })
        .unwrap_or_else(|| panic!("no row {name} in {:#?}", shown(&rows)))
}

fn click_value(debug: &mut Debug, part: Part, ix: usize) {
    let panel = panel(debug);
    panel.update(debug.cx, |panel, cx| panel.click_value_row(part, ix, cx));
    debug.draw();
}

fn click_stack(debug: &mut Debug, ix: usize) {
    let panel = panel(debug);
    debug
        .cx
        .update(|window, cx| panel.update(cx, |panel, cx| panel.click_stack_row(ix, window, cx)));
    debug.draw();
}

/// The variables requests the fake got for `reference`.
fn fetches(fake: &FakeAdapter, reference: i64) -> Vec<Value> {
    (requests(fake, "variables").into_iter())
        .filter(|arguments| arguments["variablesReference"] == reference)
        .collect()
}

/// Row `name`'s children's reference.
fn reference_of(debug: &mut Debug, part: Part, name: &str) -> i64 {
    let ix = row_named(debug, part, name);
    value_rows(debug, part)[ix].reference().expect("a parent").0
}

/// The fake's program with `big`, of 250 children, in `main`'s locals.
fn with_big(root: &Path) -> Script {
    let mut script = script(root);
    let big = Local {
        children: (0..250)
            .map(|ix| Local::new(&format!("[{ix}]"), &ix.to_string(), "i32"))
            .collect(),
        ..Local::new("big", "Vec(250)", "Vec<i32>")
    };
    script.program.functions[0].locals.push(big);
    script
}

/// C1: a node's children are fetched when it expands, once per stop: a
/// collapse and expand in the same stop fetches nothing, and the next stop,
/// the node still expanded, fetches them anew. 250 children show 100 at a
/// time.
#[gpui::test]
fn expanding_a_variable_fetches_its_children_once_per_stop(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, with_big);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.toggle(&main, 2);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(1);
    debug.until("the locals", |debug| {
        shown(&value_rows(debug, Part::Variables))
            .contains(&"  point = Point { x: 1, y: 2 }".into())
    });

    let point = reference_of(&mut debug, Part::Variables, "point");
    assert!(
        fetches(&fake, point).is_empty(),
        "not fetched until expanded"
    );
    let ix = row_named(&mut debug, Part::Variables, "point");
    click_value(&mut debug, Part::Variables, ix);
    debug.until("point's children", |debug| {
        shown(&value_rows(debug, Part::Variables)).contains(&"    y = 2".into())
    });
    assert_eq!(fetches(&fake, point).len(), 1);
    // Collapsed and expanded again: drawn from what was fetched.
    click_value(&mut debug, Part::Variables, ix);
    assert!(!shown(&value_rows(&mut debug, Part::Variables)).contains(&"    y = 2".into()));
    click_value(&mut debug, Part::Variables, ix);
    assert!(shown(&value_rows(&mut debug, Part::Variables)).contains(&"    y = 2".into()));
    assert_eq!(fetches(&fake, point).len(), 1, "once per stop");

    // 250 children: 100, then 100 more, then the last 50, from one fetch.
    let big = reference_of(&mut debug, Part::Variables, "big");
    let ix = row_named(&mut debug, Part::Variables, "big");
    click_value(&mut debug, Part::Variables, ix);
    debug.until("big's children", |debug| {
        shown(&value_rows(debug, Part::Variables)).contains(&"more 150".into())
    });
    let children = |debug: &mut Debug| {
        (value_rows(debug, Part::Variables).iter())
            .filter(|row| matches!(row, ValueRow::Variable { name, .. } if name.starts_with('[')))
            .count()
    };
    assert_eq!(children(&mut debug), 100);
    for (more, after) in [(150, 200), (50, 250)] {
        let rows = shown(&value_rows(&mut debug, Part::Variables));
        let ix = (rows.iter())
            .position(|row| *row == format!("more {more}"))
            .unwrap();
        click_value(&mut debug, Part::Variables, ix);
        assert_eq!(children(&mut debug), after);
    }
    assert!(
        !shown(&value_rows(&mut debug, Part::Variables))
            .iter()
            .any(|row| row.starts_with("more"))
    );
    assert_eq!(fetches(&fake, big).len(), 1, "paged here, fetched once");

    // The next stop: point is still expanded, and fetched once there.
    debug.run("debug.continue");
    debug.wait_stop_at(2);
    debug.until("point's children at the next stop", |debug| {
        shown(&value_rows(debug, Part::Variables)).contains(&"    y = 2".into())
    });
    let again = reference_of(&mut debug, Part::Variables, "point");
    assert_ne!(again, point, "a new stop's reference");
    assert_eq!(fetches(&fake, again).len(), 1);
}

/// C1: an adapter that counts a node's `indexedVariables` past 100 is asked
/// for them 100 at a time, with `start` and `count`, as "Show 100 more"
/// asks for each page.
#[gpui::test]
fn children_past_100_are_asked_for_a_page_at_a_time(cx: &mut TestAppContext) {
    let page = |start: usize, count: usize| {
        let variables: Vec<Value> = (start..start + count)
            .map(|ix| json!({"name": format!("[{ix}]"), "value": ix.to_string(), "variablesReference": 0}))
            .collect();
        json!({ "variables": variables })
    };
    // The session's own fetch of the locals, then the panel's; then point's
    // children, which hold `items`, and items' three pages.
    let items = json!({"variables": [
        {"name": "items", "value": "Vec(250)", "variablesReference": 9001, "indexedVariables": 250, "namedVariables": 0}
    ]});
    let mut debug = trusted(cx, |root| Script {
        faults: vec![
            Fault::new("variables", FaultKind::Answer(items)).nth(3),
            Fault::new("variables", FaultKind::Answer(page(0, 100))).nth(4),
            Fault::new("variables", FaultKind::Answer(page(100, 100))).nth(5),
            Fault::new("variables", FaultKind::Answer(page(200, 50))).nth(6),
        ],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(1);
    debug.until("the locals, fetched twice", |debug| {
        shown(&value_rows(debug, Part::Variables))
            .contains(&"  point = Point { x: 1, y: 2 }".into())
            && requests(&fake, "variables").len() == 2
    });
    let ix = row_named(&mut debug, Part::Variables, "point");
    click_value(&mut debug, Part::Variables, ix);
    debug.until("items", |debug| {
        shown(&value_rows(debug, Part::Variables)).contains(&"    items = Vec(250)".into())
    });
    let ix = row_named(&mut debug, Part::Variables, "items");
    click_value(&mut debug, Part::Variables, ix);
    debug.until("the first page", |debug| {
        shown(&value_rows(debug, Part::Variables)).contains(&"more 150".into())
    });
    for (more, next) in [(150, "more 50"), (50, "      [249] = 249")] {
        let rows = shown(&value_rows(&mut debug, Part::Variables));
        let ix = (rows.iter())
            .position(|row| *row == format!("more {more}"))
            .unwrap();
        click_value(&mut debug, Part::Variables, ix);
        debug.until(next, |debug| {
            shown(&value_rows(debug, Part::Variables)).contains(&next.into())
        });
    }
    let asked: Vec<(Value, Value, Value)> = (fetches(&fake, 9001).into_iter())
        .map(|arguments| {
            (
                arguments["filter"].clone(),
                arguments["start"].clone(),
                arguments["count"].clone(),
            )
        })
        .collect();
    assert_eq!(
        asked,
        [
            (json!("indexed"), json!(0), json!(100)),
            (json!("indexed"), json!(100), json!(100)),
            (json!("indexed"), json!(200), json!(50)),
        ]
    );
}

/// The debugger's bands the active editor's last frame laid out: each
/// one's buffer row and tint.
fn bands(debug: &mut Debug) -> Vec<(u32, Tint)> {
    let editor = debug.active_editor().expect("an editor");
    editor.update(debug.cx, |editor, _| {
        let bands = editor.drawn_row_backgrounds();
        let text = editor.model().text();
        let starts: Vec<usize> = (0..text.row_count())
            .map(|row| text.row_start(row))
            .collect();
        let mut rows = Vec::new();
        for (bounds, tint, _) in bands {
            if !matches!(tint, Tint::DebugCurrent | Tint::DebugFrame) {
                continue;
            }
            let row = (starts.iter()).position(|&start| {
                (editor.caret_bounds_at(start))
                    .is_some_and(|caret| caret.origin.y == bounds.origin.y)
            });
            rows.push((row.expect("a band on a row") as u32, tint));
        }
        rows.sort_by_key(|(row, _)| *row);
        rows
    })
}

fn selected(debug: &mut Debug) -> Option<usize> {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, _| {
        panel.selected_frame().map(|(_, ix)| ix)
    })
}

/// C2: selecting the second frame (the caller, `main`) opens its file with
/// the caret on its line, bands that row with the caller tint (the current
/// line keeping its own), and re-scopes Variables and Watch to it;
/// `debug.frame_down` and `debug.frame_up` move the selection.
#[gpui::test]
fn selecting_frame_2_opens_its_file_and_bands_the_row_with_the_frame_tint(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 3);
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| panel.add_watch("x".into(), cx));
    debug.start("main");
    debug.wait_stop_at(3);
    debug.until("add's locals and the watch", |debug| {
        let variables = shown(&value_rows(debug, Part::Variables));
        let watch = shown(&value_rows(debug, Part::Watch));
        variables.contains(&"  a = 1".into()) && watch.iter().any(|row| row.starts_with("x ✕"))
    });
    let frames: Vec<(String, bool)> = (stack_rows(&mut debug).into_iter())
        .filter_map(|row| match row {
            StackRow::Frame {
                function, current, ..
            } => Some((function, current)),
            _ => None,
        })
        .collect();
    assert_eq!(frames, [("add".into(), true), ("main".into(), false)]);
    assert_eq!(bands(&mut debug), [(3, Tint::DebugCurrent)]);

    let second = (stack_rows(&mut debug).iter())
        .position(|row| matches!(row, StackRow::Frame { index: 1, .. }))
        .unwrap();
    click_stack(&mut debug, second);
    debug.until("main's frame", |debug| {
        debug.caret() == Some((main.clone(), 2))
            && shown(&value_rows(debug, Part::Variables))
                .contains(&"  point = Point { x: 1, y: 2 }".into())
            && shown(&value_rows(debug, Part::Watch)) == ["x = 1"]
    });
    assert_eq!(selected(&mut debug), Some(1));
    debug.until("the caller band", |debug| {
        bands(debug) == [(2, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    });
    assert!((stack_rows(&mut debug).iter()).any(|row| matches!(
        row,
        StackRow::Frame {
            index: 1,
            selected: true,
            ..
        }
    )));

    debug.run("debug.frame_down");
    assert_eq!(selected(&mut debug), Some(0));
    debug.until("add's locals again", |debug| {
        shown(&value_rows(debug, Part::Variables)).contains(&"  a = 1".into())
    });
    debug.until("no caller band", |debug| {
        bands(debug) == [(3, Tint::DebugCurrent)]
    });
    debug.run("debug.frame_down");
    assert_eq!(selected(&mut debug), Some(0), "no callee below the top");
    debug.run("debug.frame_up");
    assert_eq!(selected(&mut debug), Some(1));
    debug.run("debug.frame_up");
    assert_eq!(selected(&mut debug), Some(1), "no caller above main");
}

/// The `watch`-context evaluations the fake got: their expressions.
fn watch_evaluations(fake: &FakeAdapter) -> Vec<String> {
    (requests(fake, "evaluate").into_iter())
        .filter(|arguments| arguments["context"] == "watch")
        .map(|arguments| {
            arguments["expression"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// C3: a watch is evaluated with context `watch` on each stop, the
/// session's restart's too, and is kept in the root's state file's
/// `watches` list, which a restarted app reads back.
#[gpui::test]
fn watch_expressions_reevaluate_on_each_stop_and_survive_a_restart(
    cx: &mut TestAppContext,
    restarted: &mut TestAppContext,
) {
    let state = tempfile::tempdir().unwrap();
    let state_dir = state.path().to_owned();
    cx.update(|cx| set_state_dir(cx, Some(state_dir.clone())));
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 0);
    debug.toggle(&main, 1);
    let panel = panel(&mut debug);
    let input = panel.read_with(debug.cx, |panel, _| panel.watch_input().clone());
    input.update(debug.cx, |input, cx| {
        input.set_text("point", cx);
        input.submit(cx);
    });
    debug.draw();
    assert_eq!(
        panel.read_with(debug.cx, |panel, _| panel.watches().to_vec()),
        ["point"]
    );
    assert_eq!(
        input.read_with(debug.cx, |input, _| input.text().to_owned()),
        ""
    );

    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(0);
    debug.until("the watch's value", |debug| {
        shown(&value_rows(debug, Part::Watch)) == ["point = Point { x: 1, y: 2 }"]
    });
    assert_eq!(watch_evaluations(&fake), ["point"]);
    // Its value expands as a variable does.
    click_value(&mut debug, Part::Watch, 0);
    debug.until("its children", |debug| {
        shown(&value_rows(debug, Part::Watch)).contains(&"  y = 2".into())
    });

    debug.run("debug.continue");
    debug.wait_stop_at(1);
    debug.until("evaluated again", |_| watch_evaluations(&fake).len() == 2);
    debug.until("its value and children again", |debug| {
        let rows = shown(&value_rows(debug, Part::Watch));
        rows.first().map(String::as_str) == Some("point = Point { x: 1, y: 2 }")
            && rows.contains(&"  y = 2".into())
    });

    debug.run("debug.restart");
    debug.until("the restart's stop", |_| {
        watch_evaluations(&fake).len() == 3
    });
    debug.until("the value after the restart", |debug| {
        shown(&value_rows(debug, Part::Watch))
            .first()
            .map(String::as_str)
            == Some("point = Point { x: 1, y: 2 }")
    });

    // In the state file, and read back by an app started afresh.
    let root = debug.root.clone();
    let file = state_file(&state_dir, &root);
    debug.until("the state file's watches", |debug| {
        debug.cx.executor().advance_clock(WRITE_REST);
        let read = fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        read.is_some_and(|read| read["watches"] == json!(["point"]))
    });
    let store = restarted.update(|cx| {
        set_state_dir(cx, Some(state_dir.clone()));
        BreakpointStore::for_root(&root, cx)
    });
    let read = store.read_with(restarted, |store, _| store.watch_expressions());
    assert_eq!(read, ["point"]);

    // Removing it removes it from the file too.
    panel.update(debug.cx, |panel, cx| panel.remove_watch(0, cx));
    debug.until("the watch gone from the file", |debug| {
        debug.cx.executor().advance_clock(WRITE_REST);
        let read = fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        read.is_some_and(|read| read["watches"] == json!([]))
    });
}

/// The filters of each `setExceptionBreakpoints` the fake got.
fn exception_sends(fake: &FakeAdapter) -> Vec<Value> {
    (requests(fake, "setExceptionBreakpoints").into_iter())
        .map(|arguments| arguments["filters"].clone())
        .collect()
}

fn filters(debug: &mut Debug) -> Vec<(String, bool)> {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, cx| {
        (panel.breakpoint_rows(cx).into_iter())
            .filter_map(|row| match row {
                crate::debug_panel::BreakpointRow::Filter { label, on, .. } => Some((label, on)),
                _ => None,
            })
            .collect()
    })
}

/// C4: the adapter's exception filters show as checkboxes, its defaults on;
/// toggling one sends `setExceptionBreakpoints` again with the filters on,
/// and the next session starts with them.
#[gpui::test]
fn toggling_an_exception_filter_resends_set_exception_breakpoints(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| spinning(script(root)));
    assert_eq!(filters(&mut debug), [], "no session yet");
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_state("running", |state| *state == fe_dap::SessionState::Running);
    assert_eq!(exception_sends(&fake), [json!(["uncaught"])]);
    assert_eq!(
        filters(&mut debug),
        [
            ("Raised Exceptions".into(), false),
            ("Uncaught Exceptions".into(), true)
        ]
    );

    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| panel.pick_tab(Tab::Breakpoints, cx));
    debug.draw();
    // Space on the Raised row, as its checkbox's click does.
    let raised = panel.read_with(debug.cx, |panel, cx| {
        (panel.breakpoint_rows(cx).iter()).position(|row| {
            matches!(row, crate::debug_panel::BreakpointRow::Filter { filter, .. } if filter == "raised")
        })
    });
    panel.update(debug.cx, |panel, cx| {
        panel.toggle_breakpoint_row(raised.unwrap(), cx)
    });
    debug.until("the filters sent again", |_| {
        exception_sends(&fake).len() == 2
    });
    assert_eq!(exception_sends(&fake)[1], json!(["raised", "uncaught"]));
    panel.update(debug.cx, |panel, cx| {
        panel.set_exception_filter("uncaught", false, cx)
    });
    debug.until("and again", |_| exception_sends(&fake).len() == 3);
    assert_eq!(exception_sends(&fake)[2], json!(["raised"]));
    assert_eq!(
        filters(&mut debug),
        [
            ("Raised Exceptions".into(), true),
            ("Uncaught Exceptions".into(), false)
        ]
    );

    // A new session of the adapter starts with them.
    debug.run("debug.rerun_last");
    let second = debug.fake(1);
    debug.until("the second session's filters", |_| {
        !exception_sends(&second).is_empty()
    });
    assert_eq!(exception_sends(&second), [json!(["raised"])]);
}

fn foot(debug: &mut Debug) -> Vec<Icon> {
    let dock = debug
        .workbench
        .read_with(debug.cx, |workbench, _| workbench.dock().clone());
    let icons = dock.read_with(debug.cx, |dock, _| dock.rail_icons());
    icons[5..].to_vec()
}

fn bottom_shows(debug: &mut Debug) -> Option<PanelId> {
    let workbench = debug.workbench.clone();
    debug.cx.update(|_, cx| {
        let dock = workbench.read(cx).terminal_dock().read(cx);
        (dock.is_visible())
            .then(|| dock.dock().rail_highlighted())
            .flatten()
    })
}

fn panel_focused(debug: &mut Debug) -> bool {
    let panel = panel(debug);
    debug
        .cx
        .update(|window, cx| panel.focus_handle(cx).contains_focused(window, cx))
}

/// C5: the rail's foot gets Debug, after Runs, once a session starts, and
/// the session shows the tool window without taking focus; a stop dots the
/// button; `debug.show_panels` hides the tool window, then shows it
/// focused.
#[gpui::test]
fn the_rail_shows_debug_next_to_runs_after_the_first_session(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let without = [
        Icon::TriangleAlert,
        Icon::ListVideo,
        Icon::GitGraph,
        Icon::FileTerminal,
    ];
    assert_eq!(foot(&mut debug), without);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    debug.wait_stop_at(1);
    let with = [
        Icon::TriangleAlert,
        Icon::ListVideo,
        Icon::Bug,
        Icon::GitGraph,
        Icon::FileTerminal,
    ];
    assert_eq!(foot(&mut debug), with);
    assert_eq!(bottom_shows(&mut debug), Some(DEBUG));
    assert!(!panel_focused(&mut debug), "a session doesn't take focus");
    let dock = debug
        .workbench
        .read_with(debug.cx, |workbench, _| workbench.dock().clone());
    let badge = dock.read_with(debug.cx, |dock, _| {
        dock.state()
            .drawn()
            .rail_badges()
            .into_iter()
            .find(|(id, _)| *id == DEBUG)
    });
    assert_eq!(
        badge.map(|(_, badge)| badge.name),
        Some(tr!("workbench-debug-panel-rail-stopped"))
    );

    debug.run("debug.show_panels");
    assert_eq!(bottom_shows(&mut debug), None);
    debug.run("debug.show_panels");
    assert_eq!(bottom_shows(&mut debug), Some(DEBUG));
    assert!(panel_focused(&mut debug));
    debug.run("debug.show_panels");
    assert_eq!(bottom_shows(&mut debug), None);
    assert!(!panel_focused(&mut debug), "hiding leaves the tool window");

    // Ended, the button stays; the next session doesn't add another.
    debug.run("debug.stop");
    debug.wait_state("ended", |state| state.is_ended());
    debug.start("main");
    debug.wait_stop_at(1);
    assert_eq!(foot(&mut debug), with);
}

/// C6: the tool window's tabs, trees, rows and controls have accessible
/// names, as the design's table has them.
#[gpui::test]
fn rows_and_controls_have_accessible_names(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 3);
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| panel.add_watch("b".into(), cx));
    debug.start("main");
    debug.wait_stop_at(3);
    debug.until("add's locals and the watch", |debug| {
        shown(&value_rows(debug, Part::Variables)).contains(&"  a = 1".into())
            && shown(&value_rows(debug, Part::Watch)) == ["b = 2"]
    });
    debug.cx.update(|_, cx| fe_ui::clear_nodes(cx));
    debug.draw();
    let nodes = debug.cx.update(|window, cx| fe_ui::nodes(window, cx));
    let has = |role: Role, label: &str| {
        nodes
            .iter()
            .any(|node| node.role == role && node.label == label)
    };
    for (role, label) in [
        (Role::Tab, tr!("workbench-debug-panel-tab-variables")),
        (Role::Tab, tr!("workbench-debug-panel-tab-breakpoints")),
        (Role::Tree, tr!("workbench-debug-panel-call-stack")),
        (Role::Tree, tr!("workbench-debug-panel-watch-tree")),
        (Role::Tree, tr!("workbench-debug-panel-variables")),
        (
            Role::TreeItem,
            tr!(
                "workbench-debug-panel-thread-stopped-reason",
                name = "main",
                reason = "breakpoint"
            ),
        ),
        (
            Role::TreeItem,
            tr!(
                "workbench-debug-panel-frame-name-current",
                function = "add",
                file = "main.rs",
                line = 4
            ),
        ),
        (
            Role::TreeItem,
            tr!(
                "workbench-debug-panel-frame-name",
                function = "main",
                file = "main.rs",
                line = 3
            ),
        ),
        (
            Role::TreeItem,
            tr!(
                "workbench-debug-panel-variable-name",
                name = "a",
                kind = "i32",
                value = "1"
            ),
        ),
        (
            Role::TreeItem,
            tr!(
                "workbench-debug-panel-variable-name",
                name = "b",
                kind = "i32",
                value = "2"
            ),
        ),
        (
            Role::Button,
            tr!("workbench-debug-panel-remove-watch", expression = "b"),
        ),
    ] {
        assert!(has(role, &label), "{role:?} {label:?} in {nodes:#?}");
    }
    let input = panel.read_with(debug.cx, |panel, _| panel.watch_input().clone());
    let name = input.read_with(debug.cx, |input, _| input.accessible_name());
    assert_eq!(name, tr!("workbench-debug-panel-add-watch"));

    panel.update(debug.cx, |panel, cx| panel.pick_tab(Tab::Breakpoints, cx));
    debug.cx.update(|_, cx| fe_ui::clear_nodes(cx));
    debug.draw();
    let nodes = debug.cx.update(|window, cx| fe_ui::nodes(window, cx));
    let has = |role: Role, label: &str| {
        nodes
            .iter()
            .any(|node| node.role == role && node.label == label)
    };
    for (role, label) in [
        (Role::Tree, tr!("workbench-debug-panel-breakpoints")),
        (Role::CheckBox, "Raised Exceptions".into()),
        (Role::CheckBox, "Uncaught Exceptions".into()),
        (
            Role::CheckBox,
            tr!(
                "workbench-debug-panel-breakpoint-name",
                file = "main.rs",
                line = 4
            ),
        ),
        (
            Role::TreeItem,
            tr!(
                "workbench-debug-panel-file-name",
                path = "src/main.rs",
                count = 1
            ),
        ),
    ] {
        assert!(has(role, &label), "{role:?} {label:?} in {nodes:#?}");
    }
}

/// The Breakpoints tab lists them grouped by file, each row's checkbox
/// enabling it, its line and code; a click opens its line.
#[gpui::test]
fn breakpoints_are_grouped_by_file_and_enabled_from_their_row(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 0);
    debug.toggle(&main, 3);
    let panel = panel(&mut debug);
    debug.run("debug.show_panels");
    panel.update(debug.cx, |panel, cx| panel.pick_tab(Tab::Breakpoints, cx));
    debug.until("the code read", |debug| {
        let panel = panel.clone();
        panel.read_with(debug.cx, |panel, cx| {
            panel.breakpoint_rows(cx).iter().any(|row| {
                matches!(row, crate::debug_panel::BreakpointRow::Breakpoint { code: Some(code), .. } if code == "a + b")
            })
        })
    });
    let rows = panel.read_with(debug.cx, |panel, cx| panel.breakpoint_rows(cx));
    let names: Vec<String> = rows.iter().map(|row| row.name().to_string()).collect();
    assert_eq!(
        names,
        [
            tr!("workbench-debug-panel-exception-filters").to_string(),
            tr!("workbench-debug-panel-no-filters").to_string(),
            tr!(
                "workbench-debug-panel-file-name",
                path = "src/main.rs",
                count = 2
            )
            .to_string(),
            tr!(
                "workbench-debug-panel-breakpoint-name",
                file = "main.rs",
                line = 1
            )
            .to_string(),
            tr!(
                "workbench-debug-panel-breakpoint-name",
                file = "main.rs",
                line = 4
            )
            .to_string(),
        ]
    );
    // Space on the second disables it; enter opens its line.
    panel.update(debug.cx, |panel, cx| panel.toggle_breakpoint_row(4, cx));
    debug.draw();
    assert_eq!(debug.breakpoints(&main), [(1, true), (4, false)]);
    debug.cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.click_breakpoint_row(4, window, cx))
    });
    debug.until("its line open", |debug| {
        debug.caret() == Some((main.clone(), 3))
    });
}

/// With vim on, the dock keys leave the Debug tool window as they leave
/// Runs: its context is `debug_panel`, with the `vim` flag.
#[test]
fn dock_keys_leave_the_debug_panel() {
    use fe_command::{ContextFrame, ContextStack};
    let stack: ContextStack = [
        ContextFrame::new("workbench"),
        ContextFrame::new(crate::debug_panel::KEY_CONTEXT).flag("vim"),
    ]
    .into_iter()
    .collect();
    let (layer, diagnostics) = fe_keymap::KeymapLayer::parse_for(
        Path::new(fe_keymap::DEFAULT_KEYMAP_PATH),
        fe_keymap::DEFAULT_LINUX,
        None,
        kiln::core::Os::Linux,
    );
    assert_eq!(diagnostics, []);
    let keymap = fe_keymap::Keymap::from_layers(vec![layer.unwrap()]);
    for (keys, command) in [
        ("escape", "dock.focus_editor"),
        ("ctrl-w k", "dock.focus_up"),
    ] {
        let mut matcher = fe_keymap::Matcher::new(keymap.clone());
        let mut outcome = None;
        for key in keys.split(' ') {
            outcome = Some(matcher.push(fe_keymap::Keystroke::parse(key).unwrap(), &stack));
        }
        match outcome {
            Some(fe_keymap::KeyOutcome::Dispatch(invocation)) => {
                assert_eq!(invocation.command.as_str(), command, "{keys}")
            }
            other => panic!("{keys}: {other:?}"),
        }
    }
}

fn tab_and_part(debug: &mut Debug) -> (Tab, Part) {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, _| panel.tab_and_part())
}

fn keys(debug: &mut Debug, keys: &str) {
    debug.cx.simulate_keystrokes(keys);
    debug.draw();
}

/// Review F1, F6: the keys reach every part of the tool window: tab and
/// shift-tab go round the row of tabs, Call Stack, Watch, the Add watch
/// input and Variables; left and right on the tabs switch them; a watch is
/// typed, edited and added with enter, and removed with delete; space on
/// the Breakpoints tab toggles a breakpoint. No pointer.
#[gpui::test]
fn the_keys_reach_every_tree_and_add_a_watch(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 3);
    debug.start("main");
    debug.wait_stop_at(3);
    debug.run("debug.show_panels");
    debug.run("debug.show_panels");
    assert!(panel_focused(&mut debug));
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Stack));

    keys(&mut debug, "shift-tab");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Tabs));
    keys(&mut debug, "right");
    assert_eq!(tab_and_part(&mut debug), (Tab::Breakpoints, Part::Tabs));
    keys(&mut debug, "left");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Tabs));
    keys(&mut debug, "tab");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Stack));
    // The caller frame, from the keys.
    keys(&mut debug, "down down enter");
    assert_eq!(selected(&mut debug), Some(1));
    keys(&mut debug, "tab tab");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::WatchInput));
    let input = panel(&mut debug).read_with(debug.cx, |panel, _| panel.watch_input().clone());
    assert!(
        debug
            .cx
            .update(|window, cx| input.focus_handle(cx).is_focused(window))
    );
    debug.cx.simulate_input("pointx");
    keys(&mut debug, "backspace enter");
    let panel = panel(&mut debug);
    assert_eq!(
        panel.read_with(debug.cx, |panel, _| panel.watches().to_vec()),
        ["point"]
    );
    assert_eq!(
        input.read_with(debug.cx, |input, _| input.text().to_owned()),
        ""
    );
    debug.until("the watch's value", |debug| {
        shown(&value_rows(debug, Part::Watch)) == ["point = Point { x: 1, y: 2 }"]
    });
    // Escape leaves the input for the Watch tree, whose keys expand and
    // remove the watch.
    keys(&mut debug, "escape");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Watch));
    assert!(panel_focused(&mut debug));
    keys(&mut debug, "right");
    debug.until("its children", |debug| {
        shown(&value_rows(debug, Part::Watch)).contains(&"  y = 2".into())
    });
    keys(&mut debug, "left delete");
    assert_eq!(
        panel.read_with(debug.cx, |panel, _| panel.watches().len()),
        0
    );
    // On to Variables, then round to the tabs and over to Breakpoints.
    keys(&mut debug, "tab tab");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Variables));
    keys(&mut debug, "tab right tab");
    assert_eq!(
        tab_and_part(&mut debug),
        (Tab::Breakpoints, Part::Breakpoints)
    );
    let row = panel.read_with(debug.cx, |panel, cx| {
        (panel.breakpoint_rows(cx).iter())
            .position(|row| matches!(row, crate::debug_panel::BreakpointRow::Breakpoint { .. }))
            .unwrap()
    });
    for _ in 0..row {
        keys(&mut debug, "down");
    }
    keys(&mut debug, "space");
    assert_eq!(debug.breakpoints(&main), [(4, false)]);
}

/// Review F7: a session's first stop switching the tool window to
/// Variables moves the keys there too: space doesn't reach the hidden
/// Breakpoints tree.
#[gpui::test]
fn the_first_stops_switch_to_variables_takes_the_keys_along(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    let panel = panel(&mut debug);
    debug.run("debug.show_panels");
    panel.update(debug.cx, |panel, cx| panel.pick_tab(Tab::Breakpoints, cx));
    debug.draw();
    keys(&mut debug, "down down down");
    debug.start("main");
    debug.wait_stop_at(1);
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Stack));
    let focus = panel.read_with(debug.cx, |panel, cx| panel.focus_handle(cx));
    debug.cx.update(|window, cx| window.focus(&focus, cx));
    keys(&mut debug, "space");
    assert_eq!(
        debug.breakpoints(&main),
        [(2, true)],
        "nothing hidden toggled"
    );
}

/// Review F8: activating the selected frame again, after the caret went
/// elsewhere, reveals its line again.
#[gpui::test]
fn activating_the_selected_frame_again_reveals_it(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 3);
    debug.start("main");
    debug.wait_stop_at(3);
    debug.until("the stop's caret", |debug| {
        debug.caret() == Some((main.clone(), 3))
    });
    let editor = debug.active_editor().unwrap();
    debug.caret_to(&editor, 0);
    let top = (stack_rows(&mut debug).iter())
        .position(|row| matches!(row, StackRow::Frame { index: 0, .. }))
        .unwrap();
    click_stack(&mut debug, top);
    debug.until("the frame revealed again", |debug| {
        debug.caret() == Some((main.clone(), 3))
    });
    // And from the keys.
    debug.caret_to(&editor, 0);
    let panel = panel(&mut debug);
    let focus = panel.read_with(debug.cx, |panel, cx| panel.focus_handle(cx));
    debug.cx.update(|window, cx| window.focus(&focus, cx));
    // The thread's row, then its top frame's.
    keys(&mut debug, "down down enter");
    assert_eq!(selected(&mut debug), Some(0));
    debug.until("revealed by enter", |debug| {
        debug.caret() == Some((main.clone(), 3))
    });
}

/// A stack frame for a faked `stackTrace` answer: its function, file (or
/// none) and line.
fn frame(id: i64, function: &str, path: Option<&Path>, line: i64) -> Value {
    match path {
        Some(path) => json!({
            "id": id, "name": function, "line": line, "column": 1,
            "source": {"path": path.to_string_lossy()},
        }),
        None => json!({"id": id, "name": function, "line": line, "column": 1}),
    }
}

/// Review F2: a frame whose file is still being read when another frame
/// is selected doesn't open once read: not when the next is in a file
/// already open, nor when it has no source.
#[gpui::test]
fn a_frames_late_read_never_opens_over_a_later_selection(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| {
        let main = main_path(root);
        let other = root.join("src").join("other.rs");
        let frames = json!({"stackFrames": [
            frame(1, "add", Some(&main), 4),
            frame(2, "other", Some(&other), 2),
            frame(3, "main", Some(&main), 1),
            frame(4, "native", None, 7),
        ], "totalFrames": 4});
        Script {
            faults: vec![Fault::new("stackTrace", FaultKind::Answer(frames))],
            ..script(root)
        }
    });
    let main = main_path(&debug.root);
    let other = debug.root.join("src").join("other.rs");
    fs::write(&other, "fn other() {}\nfn more() {}\n").unwrap();
    debug.open_at(&main, 0);
    let (gate, held) = async_channel::unbounded();
    debug
        .cx
        .update(|_, cx| cx.set_global(crate::debug_session::StopOpenGate(held)));
    debug.toggle(&main, 3);
    debug.start("main");
    debug.wait_stop_at(3);
    debug.until("the stop's caret", |debug| {
        debug.caret() == Some((main.clone(), 3))
    });
    let row_of = |debug: &mut Debug, index: usize| {
        (stack_rows(debug).iter())
            .position(|row| matches!(row, StackRow::Frame { index: at, .. } if *at == index))
            .unwrap()
    };
    let opened = |debug: &mut Debug, path: &Path| {
        let pane = debug
            .workbench
            .read_with(debug.cx, |workbench, _| workbench.pane().clone());
        pane.read_with(debug.cx, |pane, _| pane.position(path).is_some())
    };

    // other.rs is read, held; main.rs's frame is selected meanwhile.
    let ix = row_of(&mut debug, 1);
    click_stack(&mut debug, ix);
    let ix = row_of(&mut debug, 2);
    click_stack(&mut debug, ix);
    debug.until("main's frame", |debug| {
        debug.caret() == Some((main.clone(), 0))
    });
    gate.send_blocking(()).unwrap();
    for _ in 0..5 {
        debug.draw();
    }
    assert!(!opened(&mut debug, &other), "the late read opened nothing");
    assert_eq!(debug.caret(), Some((main.clone(), 0)));

    // other.rs again, held by a gate of its own, then a frame with no
    // source.
    let (gate, held) = async_channel::unbounded();
    debug
        .cx
        .update(|_, cx| cx.set_global(crate::debug_session::StopOpenGate(held)));
    let ix = row_of(&mut debug, 1);
    click_stack(&mut debug, ix);
    let ix = row_of(&mut debug, 3);
    click_stack(&mut debug, ix);
    assert_eq!(selected(&mut debug), Some(3));
    gate.send_blocking(()).unwrap();
    for _ in 0..5 {
        debug.draw();
    }
    assert!(!opened(&mut debug, &other), "nor after a source-less frame");
}

/// `count` frames of `main.rs` for a faked `stackTrace` answer, from
/// `start`.
fn frames(root: &Path, start: usize, count: usize, total: usize) -> Value {
    let main = main_path(root);
    let frames: Vec<Value> = (start..start + count)
        .map(|ix| {
            frame(
                5000 + ix as i64,
                &format!("f{ix}"),
                Some(&main),
                1 + (ix % 5) as i64,
            )
        })
        .collect();
    json!({"stackFrames": frames, "totalFrames": total})
}

/// Review F5: a stack past the first page of frames offers the rest: a
/// "Show more" row fetches the next page, and `debug.frame_up` past the
/// last frame fetched asks for it too; another thread's frames page alike.
#[gpui::test]
fn stacks_past_20_frames_fetch_the_rest_on_demand(cx: &mut TestAppContext) {
    let threads = json!({"threads": [{"id": 1, "name": "main"}, {"id": 2, "name": "worker"}]});
    let mut debug = trusted(cx, |root| {
        let mut faults: Vec<Fault> = (1..=12)
            .map(|nth| Fault::new("threads", FaultKind::Answer(threads.clone())).nth(nth))
            .collect();
        faults.extend([
            Fault::new("stackTrace", FaultKind::Answer(frames(root, 0, 20, 25))).nth(1),
            Fault::new("stackTrace", FaultKind::Answer(frames(root, 20, 5, 25))).nth(2),
            Fault::new("stackTrace", FaultKind::Answer(frames(root, 0, 20, 22))).nth(3),
            Fault::new("stackTrace", FaultKind::Answer(frames(root, 20, 2, 22))).nth(4),
        ]);
        Script {
            faults,
            ..script(root)
        }
    });
    let main = main_path(&debug.root);
    debug.toggle(&main, 0);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(0);
    let frames_of = |debug: &mut Debug, thread: i64| {
        (stack_rows(debug).iter())
            .filter(|row| matches!(row, StackRow::Frame { thread: at, .. } if *at == thread))
            .count()
    };
    let more_of = |debug: &mut Debug, thread: i64| {
        (stack_rows(debug).iter()).find_map(|row| match row {
            StackRow::More {
                thread: at,
                remaining,
            } if *at == thread => Some(*remaining),
            _ => None,
        })
    };
    debug.until("the first page", |debug| frames_of(debug, 1) == 20);
    assert_eq!(more_of(&mut debug, 1), Some(Some(5)));

    // Up the stack to the last frame fetched, then past it.
    for _ in 0..19 {
        debug.run("debug.frame_up");
    }
    assert_eq!(selected(&mut debug), Some(19));
    debug.run("debug.frame_up");
    debug.until("the next page and frame 21", |debug| {
        frames_of(debug, 1) == 25 && selected(debug) == Some(20)
    });
    assert_eq!(more_of(&mut debug, 1), None);
    let starts: Vec<Value> = (requests(&fake, "stackTrace").into_iter())
        .map(|arguments| arguments["startFrame"].clone())
        .collect();
    assert_eq!(starts[1], json!(20));

    // The other thread: its first page when expanded, the rest from its row.
    let worker = (stack_rows(&mut debug).iter())
        .position(|row| matches!(row, StackRow::Thread { id: 2, .. }))
        .unwrap();
    click_stack(&mut debug, worker);
    debug.until("the worker's first page", |debug| frames_of(debug, 2) == 20);
    assert_eq!(more_of(&mut debug, 2), Some(Some(2)));
    let more = (stack_rows(&mut debug).iter())
        .position(|row| matches!(row, StackRow::More { thread: 2, .. }))
        .unwrap();
    click_stack(&mut debug, more);
    debug.until("the worker's rest", |debug| frames_of(debug, 2) == 22);
    assert_eq!(more_of(&mut debug, 2), None);
}

/// Review F3: a node with named children and more than 100 indexed ones
/// shows its named children, asked for with `filter: named`, before its
/// indexed ones, paged: for a scope, a variable and a watch alike.
#[gpui::test]
fn paged_nodes_keep_their_named_children(cx: &mut TestAppContext) {
    mixed_children(cx, true);
}

/// Review round 2 F3: the same with no `namedVariables` given, which the
/// protocol leaves optional: named children are asked for unless counted 0.
#[gpui::test]
fn paged_nodes_keep_named_children_they_dont_count(cx: &mut TestAppContext) {
    mixed_children(cx, false);
}

/// A scope, a variable and a watch each with named children and more than
/// 100 indexed ones, `namedVariables` given if `counted`.
fn mixed_children(cx: &mut TestAppContext, counted: bool) {
    let counts = |mut node: Value| {
        if !counted {
            node.as_object_mut().unwrap().remove("namedVariables");
        }
        node
    };
    let page = |start: usize, count: usize, first: Value| {
        let mut variables: Vec<Value> = (start..start + count)
            .map(|ix| json!({"name": format!("[{ix}]"), "value": ix.to_string(), "variablesReference": 0}))
            .collect();
        variables[0] = first;
        json!({ "variables": variables })
    };
    let leaf = |ix: usize| json!({"name": format!("[{ix}]"), "value": ix.to_string(), "variablesReference": 0});
    let named = |name: &str, value: &str| json!({"variables": [{"name": name, "value": value, "variablesReference": 0}]});
    // In the caller's frame, selected: its scope (the third `scopes`, after
    // the session's and the panel's of the top frame) and the watch's
    // second evaluation; then each node's named and indexed children.
    let scopes = json!({"scopes": [counts(json!({
        "name": "Locals", "variablesReference": 7001, "expensive": false,
        "namedVariables": 1, "indexedVariables": 150
    }))]});
    let watch = counts(
        json!({"result": "Vec(130)", "variablesReference": 7003, "namedVariables": 1, "indexedVariables": 130}),
    );
    let nested = counts(
        json!({"name": "[0]", "value": "Vec(120)", "variablesReference": 7002, "namedVariables": 1, "indexedVariables": 120}),
    );
    let mut debug = trusted(cx, |root| Script {
        faults: vec![
            Fault::new("scopes", FaultKind::Answer(scopes)).nth(3),
            Fault::new("evaluate", FaultKind::Answer(watch)).nth(2),
            Fault::new("variables", FaultKind::Answer(named("len", "150"))).nth(3),
            Fault::new("variables", FaultKind::Answer(page(0, 100, nested))).nth(4),
            Fault::new("variables", FaultKind::Answer(named("cap", "120"))).nth(5),
            Fault::new("variables", FaultKind::Answer(page(0, 100, leaf(0)))).nth(6),
            Fault::new("variables", FaultKind::Answer(named("len", "130"))).nth(7),
            Fault::new("variables", FaultKind::Answer(page(0, 100, leaf(0)))).nth(8),
        ],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.toggle(&main, 3);
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| panel.add_watch("w".into(), cx));
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(3);
    debug.until("the top frame's scopes and locals, twice each", |debug| {
        requests(&fake, "scopes").len() == 2
            && requests(&fake, "variables").len() == 2
            && shown(&value_rows(debug, Part::Variables)).contains(&"  a = 1".into())
    });
    let caller = (stack_rows(&mut debug).iter())
        .position(|row| matches!(row, StackRow::Frame { index: 1, .. }))
        .unwrap();
    click_stack(&mut debug, caller);
    debug.until("the scope's named and indexed children", |debug| {
        let rows = shown(&value_rows(debug, Part::Variables));
        rows.contains(&"  len = 150".into()) && rows.contains(&"more 50".into())
    });
    let rows = shown(&value_rows(&mut debug, Part::Variables));
    let len = rows.iter().position(|row| row == "  len = 150").unwrap();
    assert_eq!(rows[len + 1], "  [0] = Vec(120)", "named first: {rows:#?}");
    let ix = row_named(&mut debug, Part::Variables, "[0]");
    click_value(&mut debug, Part::Variables, ix);
    debug.until("the variable's named and indexed children", |debug| {
        let rows = shown(&value_rows(debug, Part::Variables));
        rows.contains(&"    cap = 120".into()) && rows.contains(&"    [99] = 99".into())
    });
    assert!(shown(&value_rows(&mut debug, Part::Variables)).contains(&"more 20".into()));
    debug.until("the watch's value", |debug| {
        shown(&value_rows(debug, Part::Watch)) == ["w = Vec(130)"]
    });
    click_value(&mut debug, Part::Watch, 0);
    debug.until("the watch's named and indexed children", |debug| {
        let rows = shown(&value_rows(debug, Part::Watch));
        rows.get(1).map(String::as_str) == Some("  len = 130") && rows.contains(&"more 30".into())
    });
    let filters: Vec<(i64, Value)> = (requests(&fake, "variables").into_iter().skip(2))
        .map(|arguments| {
            (
                arguments["variablesReference"].as_i64().unwrap(),
                arguments["filter"].clone(),
            )
        })
        .collect();
    assert_eq!(
        filters,
        [
            (7001, json!("named")),
            (7001, json!("indexed")),
            (7002, json!("named")),
            (7002, json!("indexed")),
            (7003, json!("named")),
            (7003, json!("indexed")),
        ]
    );
}

/// The fake's script with no `restart` request, so a restart is emulated:
/// the adapter launched and configured again.
fn emulated_restart(root: &Path) -> Script {
    let mut script = script(root);
    script.capabilities.supports_restart_request = None;
    script
}

/// Review F4: filters changed during a session are the ones an emulated
/// restart configures the new adapter with.
#[gpui::test]
fn an_emulated_restart_keeps_the_filters_chosen(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| spinning(emulated_restart(root)));
    debug.start("main");
    let first = debug.fake(0);
    debug.wait_state("running", |state| *state == fe_dap::SessionState::Running);
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| {
        panel.set_exception_filter("raised", true, cx)
    });
    debug.until("the filters sent", |_| exception_sends(&first).len() == 2);
    debug.run("debug.restart");
    let second = debug.fake(1);
    debug.until("the restart's configuration", |_| {
        !exception_sends(&second).is_empty()
    });
    assert_eq!(exception_sends(&second)[0], json!(["raised", "uncaught"]));
}

/// Review F9: an emulated restart is the same session: a Debug tool window
/// hidden and a tab picked stay as they were.
#[gpui::test]
fn an_emulated_restart_keeps_the_window_hidden_and_the_tab(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, emulated_restart);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    debug.wait_stop_at(1);
    assert_eq!(bottom_shows(&mut debug), Some(DEBUG));
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| panel.pick_tab(Tab::Breakpoints, cx));
    debug.run("debug.show_panels");
    assert_eq!(bottom_shows(&mut debug), None);
    debug.run("debug.restart");
    debug.fake(1);
    debug.wait_stop_at(1);
    for _ in 0..3 {
        debug.draw();
    }
    assert_eq!(bottom_shows(&mut debug), None, "still hidden");
    assert_eq!(
        tab_and_part(&mut debug).0,
        Tab::Breakpoints,
        "the tab picked"
    );
}

/// Review F10: a read of a breakpoint line's code that started before the
/// store changed lands nothing; the read after it does.
#[gpui::test]
fn a_code_read_from_before_a_change_lands_nothing(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    let (gate, held) = async_channel::unbounded();
    debug
        .cx
        .update(|_, cx| cx.set_global(crate::debug_panel::CodeReadGate(held)));
    debug.toggle(&main, 3);
    debug.run("debug.show_panels");
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| panel.pick_tab(Tab::Breakpoints, cx));
    debug.draw();
    // The first read waits; the file changes, and a change to the store
    // starts another.
    let text = fs::read_to_string(&main).unwrap().replace("a + b", "a - b");
    fs::write(&main, text).unwrap();
    debug.toggle(&main, 0);
    let code = |debug: &mut Debug| {
        let panel = panel.clone();
        panel.read_with(debug.cx, |panel, cx| {
            (panel.breakpoint_rows(cx).into_iter())
                .find_map(|row| match row {
                    crate::debug_panel::BreakpointRow::Breakpoint { row: 3, code, .. } => {
                        Some(code)
                    }
                    _ => None,
                })
                .flatten()
        })
    };
    gate.send_blocking(()).unwrap();
    for _ in 0..3 {
        debug.draw();
    }
    assert_eq!(code(&mut debug), None, "the old read landed nothing");
    gate.send_blocking(()).unwrap();
    debug.until("the new read", |debug| {
        code(debug).as_deref() == Some("a - b")
    });
}

/// Review round 2 F5: with no `totalFrames` a stack offers more frames until
/// a page comes back short, and `debug.frame_up` crosses the page; with a
/// total past the stack's end, a short page ends it. Both for the stopped
/// thread and for another.
#[gpui::test]
fn stacks_page_without_a_total_or_past_a_wrong_one(cx: &mut TestAppContext) {
    let threads = json!({"threads": [{"id": 1, "name": "main"}, {"id": 2, "name": "worker"}]});
    let untotalled = |root: &Path, start: usize, count: usize| {
        let mut page = frames(root, start, count, 0);
        page.as_object_mut().unwrap().remove("totalFrames");
        page
    };
    let mut debug = trusted(cx, |root| {
        let mut faults: Vec<Fault> = (1..=12)
            .map(|nth| Fault::new("threads", FaultKind::Answer(threads.clone())).nth(nth))
            .collect();
        faults.extend([
            // The stopped thread: no totals; 20, then 3.
            Fault::new("stackTrace", FaultKind::Answer(untotalled(root, 0, 20))).nth(1),
            Fault::new("stackTrace", FaultKind::Answer(untotalled(root, 20, 3))).nth(2),
            // The other: a total of 40 it doesn't have; 20, then 5.
            Fault::new("stackTrace", FaultKind::Answer(frames(root, 0, 20, 40))).nth(3),
            Fault::new("stackTrace", FaultKind::Answer(frames(root, 20, 5, 40))).nth(4),
        ]);
        Script {
            faults,
            ..script(root)
        }
    });
    let main = main_path(&debug.root);
    debug.toggle(&main, 0);
    debug.start("main");
    debug.wait_stop_at(0);
    let frames_of = |debug: &mut Debug, thread: i64| {
        (stack_rows(debug).iter())
            .filter(|row| matches!(row, StackRow::Frame { thread: at, .. } if *at == thread))
            .count()
    };
    let more_of = |debug: &mut Debug, thread: i64| {
        (stack_rows(debug).iter()).find_map(|row| match row {
            StackRow::More {
                thread: at,
                remaining,
            } if *at == thread => Some(*remaining),
            _ => None,
        })
    };
    debug.until("the first page", |debug| frames_of(debug, 1) == 20);
    assert_eq!(more_of(&mut debug, 1), Some(None), "more, how many unknown");
    for _ in 0..20 {
        debug.run("debug.frame_up");
    }
    debug.until("the short page and frame 21", |debug| {
        frames_of(debug, 1) == 23 && selected(debug) == Some(20)
    });
    assert_eq!(more_of(&mut debug, 1), None, "a short page ends the stack");

    let worker = (stack_rows(&mut debug).iter())
        .position(|row| matches!(row, StackRow::Thread { id: 2, .. }))
        .unwrap();
    click_stack(&mut debug, worker);
    debug.until("the worker's first page", |debug| frames_of(debug, 2) == 20);
    assert_eq!(more_of(&mut debug, 2), Some(Some(20)));
    let more = (stack_rows(&mut debug).iter())
        .position(|row| matches!(row, StackRow::More { thread: 2, .. }))
        .unwrap();
    click_stack(&mut debug, more);
    debug.until("the worker's short page", |debug| frames_of(debug, 2) == 25);
    assert_eq!(
        more_of(&mut debug, 2),
        None,
        "short, whatever the total said"
    );
}

/// Review round 2 F13: a page of frames asked for by `debug.frame_up` lands
/// its frames but selects nothing if the user selected a frame meanwhile,
/// even the one it was asked from.
#[gpui::test]
fn a_late_frame_page_never_overrides_a_later_selection(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| Script {
        faults: vec![
            Fault::new("stackTrace", FaultKind::Answer(frames(root, 0, 20, 25))).nth(1),
            Fault::new("stackTrace", FaultKind::Answer(frames(root, 20, 5, 25))).nth(2),
        ],
        ..script(root)
    });
    let main = main_path(&debug.root);
    debug.toggle(&main, 0);
    debug.start("main");
    debug.wait_stop_at(0);
    let (gate, held) = async_channel::unbounded();
    debug
        .cx
        .update(|_, cx| cx.set_global(crate::debug_panel::FramePageGate(held)));
    for _ in 0..19 {
        debug.run("debug.frame_up");
    }
    assert_eq!(selected(&mut debug), Some(19));
    // Past the page: held. Away to frame 6, and back to frame 20.
    debug.run("debug.frame_up");
    let row_of = |debug: &mut Debug, index: usize| {
        (stack_rows(debug).iter())
            .position(|row| matches!(row, StackRow::Frame { index: at, .. } if *at == index))
            .unwrap()
    };
    let ix = row_of(&mut debug, 5);
    click_stack(&mut debug, ix);
    let ix = row_of(&mut debug, 19);
    click_stack(&mut debug, ix);
    gate.send_blocking(()).unwrap();
    debug.until("the page", |debug| {
        (stack_rows(debug).iter())
            .filter(|row| matches!(row, StackRow::Frame { .. }))
            .count()
            == 25
    });
    for _ in 0..3 {
        debug.draw();
    }
    assert_eq!(selected(&mut debug), Some(19), "the later choice stands");
}

/// Review round 2 F11: with nothing stopped, Variables draws its empty
/// state, so tab doesn't reach its trees or the Add watch input; and the
/// program running on while the input has focus moves focus to the panel.
#[gpui::test]
fn the_keys_skip_what_an_empty_state_hides(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| spinning(script(root)));
    debug.run("debug.show_panels");
    assert!(panel_focused(&mut debug));
    keys(&mut debug, "tab tab tab");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Tabs));
    let input = panel(&mut debug).read_with(debug.cx, |panel, _| panel.watch_input().clone());
    assert!(
        !debug
            .cx
            .update(|window, cx| input.focus_handle(cx).is_focused(window))
    );
    keys(&mut debug, "right tab");
    assert_eq!(
        tab_and_part(&mut debug),
        (Tab::Breakpoints, Part::Breakpoints)
    );
    keys(&mut debug, "shift-tab left");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Tabs));

    // Stopped, the input has focus; Continue runs the program, which spins.
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    debug.wait_stop_at(1);
    let focus = panel(&mut debug).read_with(debug.cx, |panel, cx| panel.focus_handle(cx));
    debug.cx.update(|window, cx| window.focus(&focus, cx));
    keys(&mut debug, "tab tab tab");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::WatchInput));
    debug.run("debug.continue");
    debug.wait_state("running", |state| *state == fe_dap::SessionState::Running);
    debug.draw();
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Tabs));
    assert!(
        panel_focused(&mut debug),
        "focus left the hidden input for the panel"
    );
    assert!(
        !debug
            .cx
            .update(|window, cx| input.focus_handle(cx).is_focused(window))
    );
    keys(&mut debug, "right");
    assert_eq!(
        tab_and_part(&mut debug),
        (Tab::Breakpoints, Part::Tabs),
        "the keys still reach it"
    );
}

/// Whether child `ix` of the Watch tree's scrolled list is inside its
/// viewport, as last drawn.
fn watch_child_shown(debug: &mut Debug, ix: usize) -> bool {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, _| {
        let scroll = &panel.watch_scroll;
        let (view, offset) = (scroll.bounds(), scroll.offset());
        let Some(child) = scroll.bounds_for_item(ix) else {
            return false;
        };
        child.top() + offset.y >= view.top() - gpui::px(0.5)
            && child.bottom() + offset.y <= view.bottom() + gpui::px(0.5)
    })
}

/// Review round 2 F12: the Watch tree, past its height, scrolls the row the
/// keys select into view, an expanded result's children too, and the Add
/// watch input when tab reaches it.
#[gpui::test]
fn watch_keys_scroll_their_row_into_view(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| {
        panel.add_watch("point".into(), cx);
        for ix in 0..8 {
            panel.add_watch(format!("x{ix}"), cx);
        }
    });
    debug.start("main");
    debug.wait_stop_at(1);
    debug.until("the watches' values", |debug| {
        shown(&value_rows(debug, Part::Watch))
            .first()
            .map(String::as_str)
            == Some("point = Point { x: 1, y: 2 }")
    });
    debug.run("debug.show_panels");
    debug.run("debug.show_panels");
    let count = value_rows(&mut debug, Part::Watch).len();
    assert!(
        crate::debug_panel::WATCH_MAX_HEIGHT < fe_ui::height::ROW * (count + 2) as f32,
        "the watches overflow the tree"
    );
    // Watch, then down to its last row: child count, after the heading.
    keys(&mut debug, "tab");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::Watch));
    for _ in 0..count {
        keys(&mut debug, "down");
    }
    debug.draw();
    assert!(
        watch_child_shown(&mut debug, count),
        "the last watch scrolled into view"
    );
    // Back to the first, expanded: its children come into view as the keys
    // go down through them.
    for _ in 0..count {
        keys(&mut debug, "up");
    }
    keys(&mut debug, "right");
    debug.until("point's children", |debug| {
        shown(&value_rows(debug, Part::Watch)).contains(&"  y = 2".into())
    });
    keys(&mut debug, "down down");
    debug.draw();
    assert!(watch_child_shown(&mut debug, 3), "point's y");
    keys(&mut debug, "tab");
    assert_eq!(tab_and_part(&mut debug), (Tab::Variables, Part::WatchInput));
    debug.draw();
    let last = value_rows(&mut debug, Part::Watch).len() + 1;
    assert!(watch_child_shown(&mut debug, last), "the Add watch row");
}
