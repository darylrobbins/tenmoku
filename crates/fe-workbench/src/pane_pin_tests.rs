//! Pinned tabs, as VS Code's sticky tabs: a pinned tab moves to the
//! strip's start, after the tabs pinned before it, and is drawn at its
//! icon's width with its title in a tooltip; no reorder crosses the
//! pinned group's edge; Close Other Tabs, Close Saved and Close All
//! Editors leave it open, while Close Tab closes it; and a saved layout
//! keeps it pinned ([`kiln_shell::PaneView::pin_tab`]).

use std::time::Duration;

use fe_command::{CommandId, ContextFrame, ContextPredicate, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{AppContext as _, Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln::platform::command_actions::typed_command;

use crate::{Launch, Pane, Workbench};

const FILES: [&str; 4] = ["a.txt", "b.txt", "c.txt", "d.txt"];

/// A workbench on a folder of `a.txt` to `d.txt`, all open in one pane in
/// order, `d.txt` active.
fn opened(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<Workbench>, VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    for name in FILES {
        std::fs::write(dir.path().join(name), "one\n").unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let root = dir.path().to_owned();
    let (workbench, mut vcx) = window(&root, None, cx);
    let cx = &mut vcx;
    for name in FILES {
        let pane = active_pane(&workbench, cx);
        let path = dir.path().join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    (dir, workbench, vcx)
}

/// A window on `root`, restoring `entry` if given, drawn and focused.
fn window(
    root: &std::path::Path,
    entry: Option<fe_command::Value>,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), move |window, cx| {
            cx.new(|cx| {
                let theme = EditorTheme::default_dark(cx);
                let mut workbench =
                    Workbench::new(launch, theme, EditorSettings::default(), window, cx);
                if let Some(entry) = &entry {
                    workbench.restore_layout(entry, window, cx);
                }
                workbench
            })
        })
        .unwrap()
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, _| window.activate_window());
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut vcx);
    (workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if cx.update(|window, cx| window.simulate_next_frame(cx)) == 0 {
            break;
        }
    }
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn active_pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// The pane's tabs' file names, a pinned one's after a `*`, and its active
/// tab's.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> (Vec<String>, String) {
    pane.read_with(cx, |pane, _| {
        let names: Vec<String> = (pane.tabs().iter().enumerate())
            .map(|(ix, tab)| {
                let name = fe_gpui::pane_item::file_name(tab.path.as_deref());
                if pane.is_pinned(ix) {
                    format!("*{name}")
                } else {
                    name
                }
            })
            .collect();
        let active = pane.active().map(|ix| names[ix].clone());
        (names, active.unwrap_or_default())
    })
}

fn expect(names: &[&str], active: &str) -> (Vec<String>, String) {
    let names = names.iter().map(|name| name.to_string()).collect();
    (names, active.to_owned())
}

/// The index of `name`'s tab.
fn index(pane: &Entity<Pane>, name: &str, cx: &mut VisualTestContext) -> usize {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .position(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()) == name)
            .expect("the tab is open")
    })
}

fn pin(pane: &Entity<Pane>, name: &str, cx: &mut VisualTestContext) {
    let ix = index(pane, name, cx);
    pane.update(cx, |pane, cx| pane.pin(ix, cx));
    draw(cx);
}

fn unpin(pane: &Entity<Pane>, name: &str, cx: &mut VisualTestContext) {
    let ix = index(pane, name, cx);
    pane.update(cx, |pane, cx| pane.unpin(ix, cx));
    draw(cx);
}

/// Moves `name`'s tab to before the tab now at `before`, or to the end.
fn reorder(pane: &Entity<Pane>, name: &str, before: Option<usize>, cx: &mut VisualTestContext) {
    let ix = index(pane, name, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.reorder(ix, before, window, cx)));
    draw(cx);
}

fn show(pane: &Entity<Pane>, name: &str, cx: &mut VisualTestContext) {
    let ix = index(pane, name, cx);
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.activate(ix, window, cx);
            cx.notify();
        })
    });
    draw(cx);
}

#[gpui::test]
fn pinning_moves_the_tab_to_the_pinned_group(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let pane = active_pane(&workbench, cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["a.txt", "b.txt", "c.txt", "d.txt"], "d.txt")
    );

    // The third of four goes first; the active tab stays the same one.
    pin(&pane, "c.txt", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*c.txt", "a.txt", "b.txt", "d.txt"], "d.txt")
    );
    // Another goes after it.
    pin(&pane, "d.txt", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*c.txt", "*d.txt", "a.txt", "b.txt"], "*d.txt")
    );
    // Pinning a pinned tab changes nothing.
    pin(&pane, "c.txt", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*c.txt", "*d.txt", "a.txt", "b.txt"], "*d.txt")
    );

    // A pinned tab can't be reordered past the pinned group, nor an
    // unpinned one into it, but each moves on its own side.
    reorder(&pane, "c.txt", None, cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*d.txt", "*c.txt", "a.txt", "b.txt"], "*c.txt")
    );
    reorder(&pane, "b.txt", Some(0), cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*d.txt", "*c.txt", "b.txt", "a.txt"], "b.txt")
    );
    reorder(&pane, "c.txt", Some(0), cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*c.txt", "*d.txt", "b.txt", "a.txt"], "*c.txt")
    );

    // Unpinned, a tab goes right after the pinned group.
    unpin(&pane, "c.txt", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*d.txt", "c.txt", "b.txt", "a.txt"], "c.txt")
    );
    unpin(&pane, "d.txt", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["d.txt", "c.txt", "b.txt", "a.txt"], "c.txt")
    );
}

#[gpui::test]
fn bulk_closes_skip_pinned_tabs(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let pane = active_pane(&workbench, cx);
    pin(&pane, "b.txt", cx);
    pin(&pane, "c.txt", cx);

    // Close Other Tabs keeps the pinned tabs and the active one.
    show(&pane, "a.txt", cx);
    run("workbench.close_other_tabs", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*b.txt", "*c.txt", "a.txt"], "a.txt")
    );
    // Close Saved and Close All Editors keep the pinned tabs.
    run("workbench.close_saved", cx);
    assert_eq!(tabs(&pane, cx), expect(&["*b.txt", "*c.txt"], "*c.txt"));
    show(&pane, "b.txt", cx);
    let d = dir.path().join("d.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(d, window, cx)));
    draw(cx);
    assert_eq!(tabs(&pane, cx).0.len(), 3);
    run("workbench.close_all_editors", cx);
    assert_eq!(tabs(&pane, cx).0, ["*b.txt", "*c.txt"]);
    run("workbench.close_group_editors", cx);
    assert_eq!(tabs(&pane, cx).0, ["*b.txt", "*c.txt"]);

    // Close Tab closes a pinned tab.
    show(&pane, "b.txt", cx);
    run("workbench.close_tab", cx);
    assert_eq!(tabs(&pane, cx), expect(&["*c.txt"], "*c.txt"));
}

#[gpui::test]
fn pinned_flags_round_trip_the_layout(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = opened(cx);
    let pane = active_pane(&workbench, &mut vcx);
    pin(&pane, "c.txt", &mut vcx);
    pin(&pane, "a.txt", &mut vcx);
    show(&pane, "b.txt", &mut vcx);
    let before = tabs(&pane, &mut vcx);
    assert_eq!(
        before,
        expect(&["*c.txt", "*a.txt", "b.txt", "d.txt"], "b.txt")
    );
    let entry = workbench.read_with(&vcx, |workbench, cx| workbench.layout_entry(cx));
    let items = &entry["panes"]["tree"]["pane"]["items"];
    let pinned: Vec<bool> = (items.as_array().unwrap().iter())
        .map(|item| item.get("pinned").is_some())
        .collect();
    assert_eq!(pinned, [true, true, false, false], "{entry}");

    let (restored, mut vcx) = window(dir.path(), Some(entry.clone()), cx);
    let cx = &mut vcx;
    let pane = active_pane(&restored, cx);
    assert_eq!(tabs(&pane, cx), before);
    let again = restored.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(again["panes"], entry["panes"]);
}

#[gpui::test]
fn pin_commands_are_in_the_catalog(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    cx.update(|_, cx| {
        for (id, title) in [
            ("workbench.pin_tab", "Pin Tab"),
            ("workbench.unpin_tab", "Unpin Tab"),
        ] {
            let command = typed_command(&CommandId::parse(id).unwrap(), cx);
            let command = command.unwrap_or_else(|| panic!("{id} is a typed command"));
            assert_eq!(command.title, title, "{id}");
            assert_eq!(
                command.context,
                Some(ContextPredicate::identifier("workbench")),
                "{id}"
            );
        }
    });
    let pane = active_pane(&workbench, cx);
    show(&pane, "b.txt", cx);
    run("workbench.pin_tab", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*b.txt", "a.txt", "c.txt", "d.txt"], "*b.txt")
    );
    show(&pane, "d.txt", cx);
    run("workbench.pin_tab", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*b.txt", "*d.txt", "a.txt", "c.txt"], "*d.txt")
    );
    show(&pane, "b.txt", cx);
    run("workbench.unpin_tab", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*d.txt", "b.txt", "a.txt", "c.txt"], "b.txt")
    );
    // Unpinning an unpinned tab changes nothing.
    run("workbench.unpin_tab", cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(&["*d.txt", "b.txt", "a.txt", "c.txt"], "b.txt")
    );
}

#[gpui::test]
fn a_pinned_tab_lays_out_at_icon_width(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let pane = active_pane(&workbench, cx);
    let width = |ix: usize, cx: &mut VisualTestContext| {
        (pane.read_with(cx, |pane, _| pane.tab_bounds(ix)))
            .expect("the tab is laid out")
            .size
            .width
    };
    let wide = width(2, cx);
    assert!(wide > fe_ui::Tab::PINNED_WIDTH, "{wide:?}");

    pin(&pane, "c.txt", cx);
    assert_eq!(width(0, cx), fe_ui::Tab::PINNED_WIDTH);
    // Drawn as its icon alone, its title in a tooltip only.
    assert!(cx.debug_bounds("tab-icon:c.txt:file-text").is_some());
    assert!(cx.debug_bounds("tab-pin:c.txt").is_none());
    assert!(cx.debug_bounds("tab-close:c.txt").is_none());
    assert!(cx.debug_bounds("tab-pin:a.txt").is_none());
    let tab = pane.read_with(cx, |pane, _| pane.tab_bounds(0)).unwrap();
    assert!(cx.debug_bounds("tab-tooltip:c.txt").is_none());
    cx.simulate_mouse_move(tab.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(600));
    draw(cx);
    assert!(
        cx.debug_bounds("tab-tooltip:c.txt").is_some(),
        "the title shows in a tooltip"
    );

    // Unpinned, it's as wide as before.
    unpin(&pane, "c.txt", cx);
    assert_eq!(width(0, cx), wide);
}

/// Gives the active tab's editor unsaved changes.
fn change(pane: &Entity<Pane>, cx: &mut VisualTestContext) {
    let editor = pane.read_with(cx, |pane, _| pane.active_editor().cloned());
    let editor = editor.expect("an editor is shown");
    editor.update(cx, |editor, cx| {
        editor.model_mut().insert("x");
        cx.notify();
    });
    draw(cx);
}

/// Whether the tab of `name` has unsaved changes.
fn dirty(pane: &Entity<Pane>, name: &str, cx: &mut VisualTestContext) -> bool {
    let ix = index(pane, name, cx);
    pane.read_with(cx, |pane, cx| pane.tabs()[ix].item().is_dirty(cx))
}

fn prompt(cx: &mut VisualTestContext) -> Option<String> {
    cx.pending_prompt().map(|(question, _)| question)
}

/// A pinned tab replaced in place (an SVG's viewer swapped for an editor
/// and back) keeps its place and its pin, and the pinned tab after it,
/// with unsaved changes, stays open.
#[gpui::test]
fn a_pinned_svg_swaps_in_place_keeping_its_pin(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let svg = dir.path().join("pic.svg");
    std::fs::write(
        &svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="3"><rect width="4" height="3"/></svg>"#,
    )
    .unwrap();
    let pane = active_pane(&workbench, cx);
    let opened = svg.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    let is_viewer = |pane: &Entity<Pane>, cx: &mut VisualTestContext| {
        let ix = index(pane, "pic.svg", cx);
        pane.read_with(cx, |pane, _| pane.tabs()[ix].image_viewer().is_some())
    };
    assert!(is_viewer(&pane, cx));
    pin(&pane, "pic.svg", cx);
    pin(&pane, "a.txt", cx);
    show(&pane, "a.txt", cx);
    change(&pane, cx);
    let order = ["*pic.svg", "*a.txt", "b.txt", "c.txt", "d.txt"];

    let path = svg.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open_as_text(path, window, cx)));
    draw(cx);
    assert_eq!(tabs(&pane, cx), expect(&order, "*a.txt"));
    assert!(!is_viewer(&pane, cx), "an editor now");
    assert!(dirty(&pane, "a.txt", cx), "a.txt kept its changes");
    assert_eq!(prompt(cx), None);

    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open_as_image(&svg, window, cx)));
    draw(cx);
    assert_eq!(tabs(&pane, cx), expect(&order, "*a.txt"));
    assert!(is_viewer(&pane, cx), "a viewer again");
    assert!(dirty(&pane, "a.txt", cx), "a.txt kept its changes");
    assert_eq!(prompt(cx), None);
}

/// A pinned large-file viewer converted to an editor stays in its place,
/// pinned, beside a pinned tab with unsaved changes.
#[gpui::test]
fn a_pinned_viewer_converts_in_place_keeping_its_pin(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let threshold = 64 * 1024;
    crate::loader::test_support::lower_threshold(dir.path(), threshold, false);
    let big = dir.path().join("big.txt");
    let text: String = (0..(threshold as usize / 12 + 100))
        .map(|row| format!("line {row:06}\n"))
        .collect();
    std::fs::write(&big, &text).unwrap();
    let pane = active_pane(&workbench, cx);
    let opened = big.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    let viewer = |pane: &Entity<Pane>, cx: &mut VisualTestContext| {
        let ix = index(pane, "big.txt", cx);
        pane.read_with(cx, |pane, _| pane.tabs()[ix].viewer().cloned())
    };
    let shown = viewer(&pane, cx).expect("big.txt opens in a viewer");
    pin(&pane, "big.txt", cx);
    pin(&pane, "a.txt", cx);
    show(&pane, "a.txt", cx);
    change(&pane, cx);
    show(&pane, "big.txt", cx);

    shown.update(cx, |_, cx| cx.emit(fe_gpui::FileViewerEvent::EditRequested));
    draw(cx);
    assert_eq!(
        tabs(&pane, cx),
        expect(
            &["*big.txt", "*a.txt", "b.txt", "c.txt", "d.txt"],
            "*big.txt"
        )
    );
    assert!(viewer(&pane, cx).is_none(), "an editor now");
    assert!(dirty(&pane, "a.txt", cx), "a.txt kept its changes");
    assert_eq!(prompt(cx), None);
}

/// A tab pinned while Close All Editors asks about another tab stays open,
/// with its changes, unasked.
#[gpui::test]
fn a_tab_pinned_while_close_all_asks_stays_open(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let pane = active_pane(&workbench, cx);
    show(&pane, "a.txt", cx);
    change(&pane, cx);
    show(&pane, "d.txt", cx);
    change(&pane, cx);

    run("workbench.close_all_editors", cx);
    assert!(prompt(cx).is_some(), "asked about a dirty tab");
    pin(&pane, "d.txt", cx);
    pin(&pane, "b.txt", cx);
    cx.simulate_prompt_answer("Discard");
    draw(cx);
    assert_eq!(prompt(cx), None, "the pinned d.txt isn't asked");
    assert_eq!(tabs(&pane, cx).0, ["*d.txt", "*b.txt"]);
    assert!(dirty(&pane, "d.txt", cx), "d.txt kept its changes");
}

/// A tab pinned while Close Editors in Group waits behind another close's
/// prompt stays open once that close is answered.
#[gpui::test]
fn a_tab_pinned_while_a_bulk_close_waits_stays_open(cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let pane = active_pane(&workbench, cx);
    show(&pane, "a.txt", cx);
    change(&pane, cx);
    run("workbench.close_tab", cx);
    assert!(prompt(cx).is_some(), "Close Tab asks about a.txt");
    run("workbench.close_group_editors", cx);
    pin(&pane, "c.txt", cx);
    cx.simulate_prompt_answer("Discard");
    draw(cx);
    // The waiting close reads its tabs afresh: a.txt is gone.
    assert_eq!(prompt(cx), None);
    assert_eq!(tabs(&pane, cx).0, ["*c.txt"]);
}

/// Runs git in `dir`, as a fixed user.
fn git(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

/// Each tab: `diff` for a diff view, else its file's name, a pinned
/// one's after a `*`.
fn tab_kinds(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter().enumerate())
            .map(|(ix, tab)| {
                let name = fe_gpui::pane_item::file_name(tab.path.as_deref());
                let diff = tab.item().downcast_ref::<fe_gpui::DiffView>().is_some();
                let name = if diff { "diff".to_owned() } else { name };
                if pane.is_pinned(ix) {
                    format!("*{name}")
                } else {
                    name
                }
            })
            .collect()
    })
}

/// A pinned diff view rebuilt in its tab (a deleted file's, once the file
/// is back) keeps its place and its pin, beside a pinned tab with unsaved
/// changes.
#[gpui::test]
fn a_pinned_diff_rebuilt_in_place_keeps_its_pin(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    git(&root, &["init", "-q"]);
    let rows: String = (0..20).map(|row| format!("line {row}\n")).collect();
    std::fs::write(root.join("e.rs"), &rows).unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(root.join(name), "one\n").unwrap();
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    std::fs::remove_file(root.join("e.rs")).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let (workbench, mut vcx) = window(&root, None, cx);
    let cx = &mut vcx;
    let pane = active_pane(&workbench, cx);
    for name in ["a.txt", "b.txt"] {
        let path = root.join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    let e = root.join("e.rs");
    let path = e.clone();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_head_diff(path, window, cx)
        })
    });
    draw(cx);
    assert_eq!(tab_kinds(&pane, cx), ["a.txt", "b.txt", "diff"]);
    let old = pane.read_with(cx, |pane, _| pane.tabs()[2].id());
    cx.update(|_, cx| pane.update(cx, |pane, cx| pane.pin(2, cx)));
    pin(&pane, "b.txt", cx);
    show(&pane, "b.txt", cx);
    change(&pane, cx);
    assert_eq!(tab_kinds(&pane, cx), ["*diff", "*b.txt", "a.txt"]);

    // The file back, a change bar click in its editor rebuilds the diff.
    let changed = rows.replace("line 9\n", "changed 9\n");
    std::fs::write(&e, changed).unwrap();
    let path = e.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = pane.read_with(cx, |pane, _| pane.active_editor().cloned());
    let editor = editor.expect("e.rs is open");
    editor.update(cx, |_, cx| {
        cx.emit(fe_gpui::commands::EditorViewEvent::ChangeBarClicked { rows: 9..10 })
    });
    draw(cx);
    let new = pane.read_with(cx, |pane, _| pane.tabs()[0].id());
    assert_ne!(new, old, "the diff was rebuilt");
    assert_eq!(tab_kinds(&pane, cx), ["*diff", "*b.txt", "a.txt", "e.rs"]);
    assert!(dirty(&pane, "b.txt", cx), "b.txt kept its changes");
    assert_eq!(prompt(cx), None);
}

/// With `command`, a bulk close, asking about a.txt, the only tab it
/// closes, a.txt is pinned and Close Tab asked of it: that close is its
/// own, not the bulk one, and closes a.txt once it agrees.
fn close_tab_on_a_tab_pinned_during(command: &str, cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let pane = active_pane(&workbench, cx);
    for name in ["b.txt", "c.txt", "d.txt"] {
        pin(&pane, name, cx);
    }
    show(&pane, "a.txt", cx);
    change(&pane, cx);

    run(command, cx);
    assert_eq!(prompt(cx), Some("Discard unsaved changes to a.txt?".into()));
    pin(&pane, "a.txt", cx);
    run("workbench.close_tab", cx);
    cx.simulate_prompt_answer("Discard");
    draw(cx);
    // The bulk close leaves the pinned a.txt; Close Tab then asks of it.
    assert_eq!(prompt(cx), Some("Discard unsaved changes to a.txt?".into()));
    cx.simulate_prompt_answer("Discard");
    draw(cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(tabs(&pane, cx).0, ["*b.txt", "*c.txt", "*d.txt"]);
}

#[gpui::test]
fn close_tab_on_a_tab_pinned_during_close_all_closes_it(cx: &mut TestAppContext) {
    close_tab_on_a_tab_pinned_during("workbench.close_all_editors", cx);
}

#[gpui::test]
fn close_tab_on_a_tab_pinned_during_close_in_group_closes_it(cx: &mut TestAppContext) {
    close_tab_on_a_tab_pinned_during("workbench.close_group_editors", cx);
}

/// With Close Tab asking about d.txt, `command`, a bulk close, waits
/// behind it; a.txt, with unsaved changes, pinned meanwhile, isn't asked
/// about when the bulk close starts, and the others close.
fn a_dirty_tab_pinned_while(command: &str, cx: &mut TestAppContext) {
    let (_dir, workbench, mut vcx) = opened(cx);
    let cx = &mut vcx;
    let pane = active_pane(&workbench, cx);
    show(&pane, "a.txt", cx);
    change(&pane, cx);
    show(&pane, "d.txt", cx);
    change(&pane, cx);

    run("workbench.close_tab", cx);
    assert_eq!(prompt(cx), Some("Discard unsaved changes to d.txt?".into()));
    run(command, cx);
    pin(&pane, "a.txt", cx);
    cx.simulate_prompt_answer("Discard");
    draw(cx);
    assert_eq!(prompt(cx), None, "the pinned a.txt isn't asked about");
    assert_eq!(tabs(&pane, cx).0, ["*a.txt"]);
    assert!(dirty(&pane, "a.txt", cx), "a.txt kept its changes");
}

#[gpui::test]
fn a_dirty_tab_pinned_while_close_all_waits_is_not_asked(cx: &mut TestAppContext) {
    a_dirty_tab_pinned_while("workbench.close_all_editors", cx);
}

#[gpui::test]
fn a_dirty_tab_pinned_while_close_in_group_waits_is_not_asked(cx: &mut TestAppContext) {
    a_dirty_tab_pinned_while("workbench.close_group_editors", cx);
}
