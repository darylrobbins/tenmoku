//! The Debug Console (`debug_panel/console.rs`, card D10) against fe-dap's
//! fake adapter, in a headless workbench (D8's harness, [`Debug`]): output
//! rows by category, `repl` evaluation in the selected frame with an
//! expandable result, the input's history, `debug.show_console`, and
//! accessible names.

use fe_dap::{SessionState, fake::THREAD_ID, protocol::OutputBody};
use fe_ui::{ActiveTheme as _, Role};
use gpui::{ElementId, Entity, Focusable as _, TestAppContext};
use kiln_l10n::tr;
use serde_json::json;

use super::{
    debug_commands::{Debug, main_path, requests, script, spinning, trusted},
    debug_panel::{DEBUG, bottom_shows, fetches, keys, panel, panel_focused, tab_and_part},
};
use crate::debug_panel::{Category, ConsoleRow, HISTORY_MAX, Part, Tab, ValueRow};

fn out(category: Option<&str>, text: &str) -> OutputBody {
    OutputBody {
        output: text.to_owned(),
        category: category.map(str::to_owned),
        ..Default::default()
    }
}

/// The fake's program, sending mixed output as it starts: the adapter's
/// own message, program output, telemetry, two lines of its error stream
/// in one event, a line written in two events, a blank line, an important
/// message, and output with no category and with one of the adapter's own.
fn with_output(root: &std::path::Path) -> fe_dap::fake::Script {
    let mut script = script(root);
    script.output = vec![
        out(Some("console"), "Launching main\n"),
        out(Some("stdout"), "hello\n"),
        out(Some("telemetry"), "{\"event\": \"launched\"}\n"),
        out(Some("stderr"), "warning: first\n  second line\n"),
        out(Some("stdout"), "partial "),
        out(Some("stdout"), "line\nnext\n\nafter a blank\n"),
        out(Some("important"), "Breakpoint moved\n"),
        out(None, "no category\r\n"),
        out(Some("custom"), "the adapter's own\n"),
    ];
    script
}

fn rows(debug: &mut Debug) -> Vec<ConsoleRow> {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, _| panel.console_rows().0)
}

/// The output rows: category and text.
fn output(debug: &mut Debug) -> Vec<(Category, String)> {
    (rows(debug).into_iter())
        .filter_map(|row| match row {
            ConsoleRow::Output { category, text } => Some((category, text)),
            _ => None,
        })
        .collect()
}

/// Each evaluation's rows as text: `› expression`, its value (with ▸ or
/// ▾ when it expands), `✕ error`, or a child as "  name = value".
fn shown(debug: &mut Debug) -> Vec<String> {
    (rows(debug).into_iter())
        .filter_map(|row| match row {
            ConsoleRow::Output { .. } => None,
            ConsoleRow::Echo { expression } => Some(format!("› {expression}")),
            ConsoleRow::Evaluating => Some("…".to_owned()),
            ConsoleRow::Result {
                value, expanded, ..
            } => Some(match (value.reference > 0, expanded) {
                (false, _) => value.text,
                (true, false) => format!("▸ {}", value.text),
                (true, true) => format!("▾ {}", value.text),
            }),
            ConsoleRow::Failed { error } => Some(format!("✕ {error}")),
            ConsoleRow::Child(ValueRow::Variable {
                depth, name, value, ..
            }) => Some(format!("{}{name} = {}", "  ".repeat(depth), value.text)),
            ConsoleRow::Child(row) => Some(row.name().to_string()),
        })
        .collect()
}

/// The console rows drawn, as a screen reader has them: index and name.
fn drawn_rows(debug: &mut Debug) -> Vec<(usize, String)> {
    let nodes = debug.cx.update(|window, cx| fe_ui::nodes(window, cx));
    (nodes.into_iter())
        .filter(|node| node.role == Role::ListItem)
        .filter_map(|node| match node.id {
            ElementId::NamedInteger(name, ix) if name.as_ref() == "debug-console-row" => {
                Some((ix as usize, node.label.to_string()))
            }
            _ => None,
        })
        .collect()
}

fn input(debug: &mut Debug) -> Entity<fe_ui::TextInput> {
    let panel = panel(debug);
    panel.read_with(debug.cx, |panel, _| panel.console_input().clone())
}

fn input_text(debug: &mut Debug) -> String {
    let input = input(debug);
    input.read_with(debug.cx, |input, _| input.text().to_owned())
}

fn input_focused(debug: &mut Debug) -> bool {
    let input = input(debug);
    debug
        .cx
        .update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// Types `expression` in the focused input and presses enter.
fn evaluate(debug: &mut Debug, expression: &str) {
    debug.cx.simulate_input(expression);
    keys(debug, "enter");
}

/// C1: output events append in order, a row a line: an event's lines are
/// rows of their own, a line written in two events is one row, a blank
/// line stays. Telemetry isn't shown, from the adapter or handed to the
/// console. Each category has its look: the error stream in the error
/// colour with an edge of it, the adapter's messages faint, important
/// ones in the warning colour with ▲.
#[gpui::test]
fn output_events_append_in_order_with_stderr_tinted(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, with_output);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    debug.wait_stop_at(1);
    let want = [
        (Category::Console, "Launching main"),
        (Category::Stdout, "hello"),
        (Category::Stderr, "warning: first"),
        (Category::Stderr, "  second line"),
        (Category::Stdout, "partial line"),
        (Category::Stdout, "next"),
        (Category::Stdout, ""),
        (Category::Stdout, "after a blank"),
        (Category::Important, "Breakpoint moved"),
        (Category::Console, "no category"),
        (Category::Stdout, "the adapter's own"),
    ]
    .map(|(category, text)| (category, text.to_owned()));
    debug.until("the output", |debug| output(debug).len() >= want.len());
    assert_eq!(output(&mut debug), want);

    // Telemetry that reaches the console isn't shown either.
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| {
        panel.console_output(&out(Some("telemetry"), "{}\n"), cx)
    });
    assert_eq!(output(&mut debug), want);

    debug.cx.update(|_, cx| {
        let c = cx.theme().colors;
        let stderr = Category::Stderr.style(cx);
        assert_eq!(
            (stderr.ink, stderr.edge),
            (c.status.error, Some(c.status.error))
        );
        let stdout = Category::Stdout.style(cx);
        assert_eq!(
            (stdout.ink, stdout.edge, stdout.mark),
            (c.text.primary, None, false)
        );
        let console = Category::Console.style(cx);
        assert_eq!((console.ink, console.edge), (c.text.faint, None));
        let important = Category::Important.style(cx);
        assert_eq!((important.ink, important.mark), (c.status.warning, true));
    });

    // Drawn in order, a list item a row.
    debug.run("debug.show_console");
    debug.cx.update(|_, cx| fe_ui::clear_nodes(cx));
    debug.draw();
    let names: Vec<String> = (rows(&mut debug).iter())
        .map(|row| row.name().to_string())
        .collect();
    let drawn = drawn_rows(&mut debug);
    assert!(!drawn.is_empty());
    for (ix, label) in drawn {
        assert_eq!(label, names[ix], "row {ix}");
    }
    assert_eq!(
        names[2],
        tr!(
            "workbench-debug-console-stderr-name",
            text = "warning: first"
        )
    );
    assert_eq!(names[6], tr!("workbench-debug-console-blank"));
}

/// C2: an expression is evaluated with context `repl` in the frame Call
/// Stack has selected (the caller here, where `point` is and `a` isn't);
/// its result row expands, its children fetched once in the stop, by keys
/// and clicks; once the stop is over, the result no longer expands.
#[gpui::test]
fn an_evaluations_result_row_expands(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 3);
    debug.start("main");
    let fake = debug.fake(0);
    debug.wait_stop_at(3);
    let panel = panel(&mut debug);
    debug.cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.select_frame(THREAD_ID, 1, window, cx))
    });
    debug.draw();
    let debugger = debug.debugger();
    let caller = debugger.read_with(debug.cx, |debugger, _| {
        let stop = debugger.session().and_then(|session| session.stop_data());
        stop.expect("the stop's data").frames[1].id
    });

    debug.run("debug.show_console");
    assert!(input_focused(&mut debug));
    evaluate(&mut debug, "point");
    evaluate(&mut debug, "a");
    debug.until("both answered", |debug| {
        shown(debug)
            == [
                "› point",
                "▸ Point { x: 1, y: 2 }",
                "› a",
                "✕ a isn't defined",
            ]
    });
    let asked: Vec<_> = (requests(&fake, "evaluate").into_iter())
        .map(|arguments| {
            (
                arguments["expression"].clone(),
                arguments["context"].clone(),
                arguments["frameId"].clone(),
            )
        })
        .collect();
    assert_eq!(
        asked,
        [
            (json!("point"), json!("repl"), json!(caller)),
            (json!("a"), json!("repl"), json!(caller)),
        ]
    );

    // Its children, fetched when it expands, from a click.
    let reference = match &rows(&mut debug)[1] {
        ConsoleRow::Result { value, .. } => value.reference,
        other => panic!("not a result: {other:?}"),
    };
    assert!(fetches(&fake, reference).is_empty(), "not until expanded");
    panel.update(debug.cx, |panel, cx| panel.click_console_row(1, cx));
    debug.until("point's children", |debug| {
        shown(debug)[..4] == ["› point", "▾ Point { x: 1, y: 2 }", "  x = 1", "  y = 2"]
    });
    assert_eq!(fetches(&fake, reference).len(), 1);
    // Collapsed and expanded from the keys, the same stop: fetched once.
    keys(&mut debug, "escape");
    assert_eq!(tab_and_part(&mut debug), (Tab::Console, Part::Console));
    keys(&mut debug, "down left");
    assert_eq!(
        shown(&mut debug)[..2],
        ["› point", "▸ Point { x: 1, y: 2 }"]
    );
    assert_eq!(shown(&mut debug).len(), 4);
    keys(&mut debug, "right");
    assert_eq!(shown(&mut debug)[2..4], ["  x = 1", "  y = 2"]);
    assert_eq!(fetches(&fake, reference).len(), 1);

    // The stop over, its references are too: the value stays, unexpandable.
    debug.run("debug.continue");
    debug.wait_state("ended", SessionState::is_ended);
    debug.draw();
    assert_eq!(
        shown(&mut debug),
        [
            "› point",
            "Point { x: 1, y: 2 }",
            "› a",
            "✕ a isn't defined"
        ]
    );
    panel.update(debug.cx, |panel, cx| panel.click_console_row(1, cx));
    assert_eq!(fetches(&fake, reference).len(), 1);
}

/// C3: up and down in the input recall what was evaluated, newest first,
/// stopping at the oldest; down past the newest gives back what was being
/// typed. A blank line and a repeat of the last aren't added.
#[gpui::test]
fn history_recalls_with_up_and_down(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, script);
    let main = main_path(&debug.root);
    debug.toggle(&main, 1);
    debug.start("main");
    debug.wait_stop_at(1);
    debug.run("debug.show_console");
    for expression in ["x", "point", "point", "  "] {
        evaluate(&mut debug, expression);
    }
    let panel = panel(&mut debug);
    let history =
        |debug: &mut Debug| panel.read_with(debug.cx, |panel, _| panel.console_history().to_vec());
    assert_eq!(history(&mut debug), ["x", "point"]);
    assert_eq!(input_text(&mut debug), "");

    debug.cx.simulate_input("dra");
    let recalled = [
        ("up", "point"),
        ("up", "x"),
        ("up", "x"),
        ("down", "point"),
        ("down", "dra"),
        ("down", "dra"),
    ];
    for (key, text) in recalled {
        keys(&mut debug, key);
        assert_eq!(input_text(&mut debug), text, "after {key}");
    }
    // A recalled entry evaluates as typed.
    keys(&mut debug, "up up enter");
    debug.until("x's value", |debug| {
        shown(debug).last().is_some_and(|row| row == "1")
    });
    assert_eq!(history(&mut debug), ["x", "point", "x"]);
    assert_eq!(input_text(&mut debug), "");
    keys(&mut debug, "up");
    assert_eq!(input_text(&mut debug), "x");

    // It keeps the last HISTORY_MAX.
    panel.update(debug.cx, |panel, cx| {
        for ix in 0..HISTORY_MAX {
            panel.evaluate_in_console(format!("e{ix}"), cx);
        }
    });
    let kept = history(&mut debug);
    assert_eq!(kept.len(), HISTORY_MAX);
    assert_eq!(kept[0], "e0");
}

/// C4: `debug.show_console` shows the Debug tool window on Console with
/// its input focused, and again hides it, focus going back; from another
/// tab it switches to Console. A session opens the window on Console
/// without focus. The tab, the list, its rows and the input (named for
/// the frame it evaluates in) have accessible names.
#[gpui::test]
fn debug_show_console_shows_and_focuses_then_hides(cx: &mut TestAppContext) {
    let mut debug = trusted(cx, |root| spinning(with_output(root)));
    // No session: the empty state, the keys in the tool window.
    debug.run("debug.show_console");
    assert_eq!(bottom_shows(&mut debug), Some(DEBUG));
    assert_eq!(tab_and_part(&mut debug).0, Tab::Console);
    assert!(panel_focused(&mut debug));
    debug.run("debug.show_console");
    assert_eq!(bottom_shows(&mut debug), None);
    assert!(!panel_focused(&mut debug), "focus went back");

    // A session opens it on Console, focus staying in the editor.
    debug.start("main");
    debug.wait_state("running", |state| *state == SessionState::Running);
    debug.until("the output", |debug| output(debug).len() >= 11);
    assert_eq!(bottom_shows(&mut debug), Some(DEBUG));
    assert_eq!(tab_and_part(&mut debug).0, Tab::Console);
    assert!(!panel_focused(&mut debug));

    // From Variables, it switches to Console, the input focused.
    let panel = panel(&mut debug);
    panel.update(debug.cx, |panel, cx| panel.pick_tab(Tab::Variables, cx));
    debug.draw();
    debug.run("debug.show_console");
    assert_eq!(bottom_shows(&mut debug), Some(DEBUG));
    assert_eq!(tab_and_part(&mut debug), (Tab::Console, Part::ConsoleInput));
    assert!(input_focused(&mut debug));
    debug.run("debug.show_console");
    assert_eq!(bottom_shows(&mut debug), None);
    assert!(!panel_focused(&mut debug));
    debug.run("debug.show_console");
    assert_eq!(bottom_shows(&mut debug), Some(DEBUG));
    assert!(input_focused(&mut debug));

    // Paused, the input evaluates in `main`, and is named for it; the tab
    // picked, the stop leaves Console.
    debug.run("debug.pause");
    debug.until("the pause's frames", |debug| debug.stopped_row().is_some());
    assert_eq!(tab_and_part(&mut debug).0, Tab::Console);
    // The stop took focus to its line: back to the input.
    debug.run("debug.show_console");
    debug.run("debug.show_console");
    assert!(input_focused(&mut debug));
    evaluate(&mut debug, "point");
    debug.until("point's value", |debug| {
        shown(debug)
            .last()
            .is_some_and(|row| row.ends_with("Point { x: 1, y: 2 }"))
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
        (Role::Tab, tr!("workbench-debug-panel-tab-console")),
        (Role::List, tr!("workbench-debug-console-output")),
        (
            Role::ListItem,
            tr!("workbench-debug-console-echo", expression = "point"),
        ),
        (
            Role::ListItem,
            tr!(
                "workbench-debug-console-result-name",
                value = "Point { x: 1, y: 2 }",
                kind = "Point"
            ),
        ),
    ] {
        assert!(has(role, &label), "{role:?} {label:?} in {nodes:#?}");
    }
    let names: Vec<String> = (rows(&mut debug).iter())
        .map(|row| row.name().to_string())
        .collect();
    for (ix, label) in drawn_rows(&mut debug) {
        assert_eq!(label, names[ix], "row {ix}");
    }
    let input = input(&mut debug);
    let name = input.read_with(debug.cx, |input, _| input.accessible_name());
    assert_eq!(
        name,
        tr!("workbench-debug-console-input-name", frame = "main")
    );

    // A tab that hides the focused input takes focus to the tool window.
    let tab = (debug.cx)
        .debug_bounds(Tab::Variables.selector())
        .expect("the Variables tab is drawn");
    debug
        .cx
        .simulate_click(tab.center(), gpui::Modifiers::none());
    debug.draw();
    assert_eq!(tab_and_part(&mut debug).0, Tab::Variables);
    let focus = panel.read_with(debug.cx, |panel, cx| panel.focus_handle(cx));
    assert!(debug.cx.update(|window, _| focus.is_focused(window)));
}
