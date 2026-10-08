//! Go to definition, workspace symbols, back and forward, by their default
//! keys, over a small Rust project.

use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use fe_gpui::EditorView;
use fe_nav::Tier;
use fe_project::FsChange;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{open_root, panel};
use crate::{
    StatusSide, Workbench,
    nav::{HISTORY_LIMIT, NavHistory, Place, SOURCE_ITEM},
    workbench::Palette,
};

/// A project of `files`, and its root, resolved as the watcher reports it.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    for (rel, text) in files {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    (dir, root)
}

const MAIN: &str = "mod util;\n\nfn main() {\n    util::helper();\n}\n";
const UTIL: &str = "pub fn helper() {}\n";

/// A workbench rooted at `root`, focused, showing `file` with the caret on
/// the first `word` in it.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    file: &str,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    let path = root.join(file);
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    });
    settle(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown")
}

fn caret_on(workbench: &Entity<Workbench>, word: &str, cx: &mut VisualTestContext) -> usize {
    let editor = editor(workbench, cx);
    editor.update(cx, |editor, cx| {
        let offset = editor.model().text().to_string().find(word).unwrap() + 1;
        editor.model_mut().set_cursor(offset);
        cx.notify();
        offset
    })
}

/// The active editor's file and its selection.
fn at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, Range<usize>) {
    let editor = editor(workbench, cx);
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.path().unwrap().to_owned(),
            model.selections()[0].range.clone(),
        )
    })
}

fn focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = editor(workbench, cx);
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

fn palette(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Palette> {
    workbench.read_with(cx, |workbench, _| workbench.palette)
}

fn builds(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.nav_state.builds)
}

fn progress(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(usize, bool)> {
    workbench.read_with(cx, |workbench, _| workbench.nav_state.index_progress())
}

/// Pauses the next build before it walks, until the sender goes.
fn pause_build(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> async_channel::Sender<()> {
    let (gate, wait) = async_channel::bounded(1);
    workbench.update(cx, |workbench, _| {
        workbench.nav_state.build_gate = Some(wait)
    });
    gate
}

/// The definitions picker's rows as `(rel, name range)`, and its status.
fn definitions(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<(PathBuf, Range<usize>)>, String) {
    workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.nav_state.definitions.read(cx);
        let rows = (picker.delegate.items().iter())
            .map(|item| (item.location.rel.clone(), item.location.range.clone()))
            .collect();
        (rows, picker.delegate.status_text())
    })
}

fn symbols_status(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |workbench, cx| {
        workbench.nav_state.symbols.read(cx).delegate.status_text()
    })
}

/// The symbols picker's rows as `(name, rel:row)`.
fn symbols(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    workbench.read_with(cx, |workbench, cx| {
        let delegate = &workbench.nav_state.symbols.read(cx).delegate;
        (delegate.matches().iter())
            .map(|found| {
                (
                    found.name.clone(),
                    crate::nav_picker::place(&found.location),
                )
            })
            .collect()
    })
}

fn helper_range() -> Range<usize> {
    let start = UTIL.find("helper").unwrap();
    start..start + "helper".len()
}

#[gpui::test]
fn goto_definition_jumps_across_files(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", UTIL)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    caret_on(&workbench, "helper", cx);

    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(
        at(&workbench, cx),
        (root.join("src/util.rs"), helper_range())
    );
    assert!(focused(&workbench, cx));
    assert_eq!(palette(&workbench, cx), None, "no picker");

    // The status bar names the source (C10).
    let drawn = workbench.read_with(cx, |workbench, cx| {
        workbench.status_bar().read(cx).drawn(StatusSide::Left)
    });
    assert!(
        drawn.contains(&(SOURCE_ITEM.into(), "by search".into())),
        "{drawn:?}"
    );
}

#[gpui::test]
fn ambiguous_definitions_open_a_picker(cx: &mut TestAppContext) {
    let main = "fn main() {\n    helper();\n    missing();\n}\n";
    let (_dir, root) = project(&[
        ("src/main.rs", main),
        ("src/a.rs", UTIL),
        ("src/b.rs", UTIL),
    ]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    // Built first, so the answer below is the complete index's.
    cx.simulate_keystrokes("cmd-t escape");
    settle(cx);
    assert_eq!(progress(&workbench, cx).map(|(_, done)| done), Some(true));
    caret_on(&workbench, "helper", cx);
    let request = workbench.read_with(cx, |workbench, cx| workbench.nav_request(cx).unwrap());
    let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
    let answer = navigation
        .definitions(&request, &AtomicBool::new(false))
        .unwrap();
    assert!(!answer.value.unambiguous);
    let tiers: Vec<_> = answer.value.items.iter().map(|item| item.tier).collect();
    assert_eq!(tiers, [Tier::Project, Tier::Project]);
    let expected: Vec<_> = (answer.value.items.iter())
        .map(|item| (item.location.rel.clone(), item.location.range.clone()))
        .collect();
    assert_eq!(expected.len(), 2);

    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Definitions));
    let (rows, status) = definitions(&workbench, cx);
    assert_eq!(rows, expected, "in the answer's order");
    assert_eq!(status, "2 definitions of `helper` (by search)");

    cx.simulate_keystrokes("down enter");
    settle(cx);
    assert_eq!(palette(&workbench, cx), None);
    let (rel, range) = expected[1].clone();
    assert_eq!(at(&workbench, cx), (root.join(rel), range));
    assert!(focused(&workbench, cx));

    // No definition: a picker with no rows that says so.
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    caret_on(&workbench, "missing", cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Definitions));
    let (rows, status) = definitions(&workbench, cx);
    assert!(rows.is_empty());
    assert_eq!(status, "No definition found for `missing` (by search)");
}

#[gpui::test]
fn works_before_the_index_is_built(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", UTIL)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    assert_eq!(builds(&workbench, cx), 0, "no build from Workbench::new");
    assert_eq!(progress(&workbench, cx), None);
    let gate = pause_build(&workbench, cx);
    caret_on(&workbench, "helper", cx);

    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(builds(&workbench, cx), 1);
    assert_eq!(progress(&workbench, cx), Some((0, false)), "paused");
    assert_eq!(
        at(&workbench, cx),
        (root.join("src/util.rs"), helper_range())
    );
    // That was the fallback's answer, from a partial set.
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    let request = workbench.read_with(cx, |workbench, cx| workbench.nav_request(cx).unwrap());
    let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
    let answer = navigation
        .definitions(&request, &AtomicBool::new(false))
        .unwrap();
    assert!(answer.value.partial);
    assert_eq!(answer.value.items.len(), 1);
    assert_eq!(answer.value.items[0].location.rel, Path::new("src/util.rs"));

    // A second command builds nothing more.
    cx.simulate_keystrokes("cmd-t");
    settle(cx);
    assert_eq!(builds(&workbench, cx), 1);
    drop(gate);
    settle(cx);
    assert_eq!(progress(&workbench, cx), Some((2, true)));
    assert_eq!(builds(&workbench, cx), 1);
}

#[gpui::test]
fn back_and_forward(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", UTIL)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let main = root.join("src/main.rs");
    let util = root.join("src/util.rs");
    let before = caret_on(&workbench, "helper", cx);
    let buttons = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let nav = workbench.pane().read(cx).toolbar().nav;
            (nav.back, nav.forward)
        })
    };
    assert_eq!(buttons(cx), (false, false));

    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(at(&workbench, cx), (util.clone(), helper_range()));
    assert_eq!(buttons(cx), (true, false));

    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(at(&workbench, cx), (main.clone(), before..before));
    assert!(focused(&workbench, cx));
    assert_eq!(buttons(cx), (false, true));

    cx.simulate_keystrokes("ctrl-shift--");
    settle(cx);
    let end = helper_range().end;
    assert_eq!(at(&workbench, cx), (util.clone(), end..end));
    assert_eq!(buttons(cx), (true, false));

    // A new jump after going back clears forward.
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(buttons(cx), (false, true));
    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(at(&workbench, cx), (util.clone(), helper_range()));
    assert_eq!(buttons(cx), (true, false));
    cx.simulate_keystrokes("ctrl-shift--");
    settle(cx);
    assert_eq!(
        at(&workbench, cx),
        (util, helper_range()),
        "nothing forward"
    );

    // At most 100 places, back and forward together.
    let mut history = NavHistory::default();
    for offset in 0..HISTORY_LIMIT + 20 {
        history.push(Some(Place {
            path: main.clone(),
            offset,
        }));
    }
    assert_eq!(history.len(), HISTORY_LIMIT);
    let here = |offset| {
        Some(Place {
            path: main.clone(),
            offset,
        })
    };
    let back = history.back(here(1000)).unwrap();
    assert_eq!(back.offset, HISTORY_LIMIT + 19);
    assert_eq!(history.len(), HISTORY_LIMIT);
    assert_eq!(history.forward(here(5)).unwrap().offset, 1000);
    assert_eq!(history.len(), HISTORY_LIMIT);
}

#[gpui::test]
fn workspace_symbols_picker(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", UTIL)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let gate = pause_build(&workbench, cx);

    cx.simulate_keystrokes("cmd-t");
    settle(cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Symbols));
    assert_eq!(symbols_status(&workbench, cx), "Indexing… 0 files");
    drop(gate);
    settle(cx);

    let queries = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            workbench.nav_state.symbols.read(cx).delegate.queries
        })
    };
    let before = queries(cx);
    // Typed a char at a time, the executor not running in between: the
    // first char's query runs, and "hlp" replaces "hl" while it waits.
    cx.simulate_input("hlp");
    assert_eq!(queries(cx) - before, 2);
    let rows = symbols(&workbench, cx);
    assert_eq!(rows, [("helper".to_owned(), "src/util.rs:1".to_owned())]);
    let highlights = workbench.read_with(cx, |workbench, cx| {
        let delegate = &workbench.nav_state.symbols.read(cx).delegate;
        delegate.label(0).unwrap().highlights
    });
    assert_eq!(highlights, [0..1, 2..4]);
    assert_eq!(symbols_status(&workbench, cx), "1 result (by search)");

    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(palette(&workbench, cx), None);
    assert_eq!(
        at(&workbench, cx),
        (root.join("src/util.rs"), helper_range())
    );
    assert!(focused(&workbench, cx));
}

#[gpui::test]
fn watcher_batches_reach_the_nav_index(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", UTIL)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let panel = panel(&workbench, cx);
    let fresh = root.join("src/fresh.rs");
    let deliver = |cx: &mut VisualTestContext| {
        let changes = [FsChange::Path(fresh.clone())];
        panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
        settle(cx);
    };
    cx.simulate_keystrokes("cmd-t escape");
    settle(cx);
    assert_eq!(progress(&workbench, cx), Some((2, true)));

    fs::write(&fresh, "fn fresh() {}\n").unwrap();
    deliver(cx);
    cx.simulate_keystrokes("cmd-t");
    cx.simulate_input("fresh");
    assert_eq!(
        symbols(&workbench, cx),
        [("fresh".to_owned(), "src/fresh.rs:1".to_owned())]
    );

    fs::remove_file(&fresh).unwrap();
    deliver(cx);
    assert_eq!(symbols(&workbench, cx), [], "requeried once applied");
    assert_eq!(progress(&workbench, cx), Some((2, true)));
}

#[gpui::test]
fn stale_requests_are_dropped(cx: &mut TestAppContext) {
    let main = "fn main() {\n    alpha();\n    beta();\n}\n";
    let (_dir, root) = project(&[
        ("src/main.rs", main),
        ("src/a.rs", "pub fn alpha() {}\n"),
        ("src/b.rs", "pub fn beta() {}\n"),
    ]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let (release, hold) = async_channel::bounded::<()>(1);
    workbench.update(cx, |workbench, _| {
        workbench.nav_state.hold_answer = Some(hold)
    });

    // The first answer is found, then held before it lands.
    caret_on(&workbench, "alpha", cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    assert_eq!(at(&workbench, cx).0, root.join("src/main.rs"), "held");
    caret_on(&workbench, "beta", cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    let beta = "pub fn ".len().."pub fn beta".len();
    assert_eq!(at(&workbench, cx), (root.join("src/b.rs"), beta.clone()));
    let cancels = workbench.read_with(cx, |workbench, _| workbench.nav_state.cancels.clone());
    assert_eq!(cancels.len(), 2);
    assert!(
        cancels[0].load(Ordering::Relaxed),
        "the first was cancelled"
    );
    assert!(!cancels[1].load(Ordering::Relaxed));

    drop(release);
    settle(cx);
    assert_eq!(at(&workbench, cx), (root.join("src/b.rs"), beta), "dropped");
}

/// Presses `keys` without running the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.dispatch_keystroke(gpui::Keystroke::parse(keys).unwrap(), cx);
    });
}

fn buttons(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, bool) {
    workbench.read_with(cx, |workbench, cx| {
        let nav = workbench.pane().read(cx).toolbar().nav;
        (nav.back, nav.forward)
    })
}

/// Closes the active pane's tab for `path`, then focuses the workbench.
fn close(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| {
            let ix = pane.position(path).unwrap();
            pane.activate(ix, window, cx);
            pane.close_active(window, cx);
        });
    });
    settle(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
}

#[gpui::test]
fn a_picker_jump_drops_a_running_goto_definition(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", UTIL)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let (release, hold) = async_channel::bounded::<()>(1);
    workbench.update(cx, |workbench, _| {
        workbench.nav_state.hold_answer = Some(hold)
    });
    caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("f12");
    settle(cx);

    // A symbol in the same file is picked while the answer is held.
    cx.simulate_keystrokes("cmd-t");
    cx.simulate_input("main");
    cx.simulate_keystrokes("enter");
    settle(cx);
    let main = MAIN.find("main").unwrap();
    let picked = (root.join("src/main.rs"), main..main + "main".len());
    assert_eq!(at(&workbench, cx), picked);

    drop(release);
    settle(cx);
    assert_eq!(at(&workbench, cx), picked, "the held answer was dropped");
}

#[gpui::test]
fn history_without_a_caret(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", UTIL)]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    let (main, util) = (root.join("src/main.rs"), root.join("src/util.rs"));
    caret_on(&workbench, "helper", cx);
    cx.simulate_keystrokes("f12");
    settle(cx);
    cx.simulate_keystrokes("ctrl--");
    settle(cx);
    assert_eq!(buttons(&workbench, cx), (false, true));

    // Going forward to a file still being read, then back: forward keeps
    // where it was going.
    close(&workbench, &util, cx);
    assert_eq!(at(&workbench, cx).0, main);
    press("ctrl-shift--", cx);
    press("ctrl--", cx);
    assert_eq!(buttons(&workbench, cx), (false, true));
    workbench.read_with(cx, |workbench, _| {
        assert_eq!(workbench.nav_state.history.len(), 1);
    });
    settle(cx);

    // A jump with no tab open still clears the way forward.
    for path in [&main, &util] {
        if workbench
            .read_with(cx, |workbench, cx| workbench.pane().read(cx).position(path))
            .is_some()
        {
            close(&workbench, path, cx);
        }
    }
    assert!(workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).is_none()));
    cx.simulate_keystrokes("cmd-t");
    cx.simulate_input("hlp");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(at(&workbench, cx).0, util);
    assert!(!buttons(&workbench, cx).1, "nothing forward");
}

#[gpui::test]
fn a_root_change_drops_a_held_symbols_answer(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[("src/main.rs", MAIN), ("src/util.rs", UTIL)]);
    let (_other_dir, other) = project(&[("lib.rs", "fn other() {}\n")]);
    let (workbench, cx) = opened(cx, &root, "src/main.rs");
    cx.simulate_keystrokes("cmd-t");
    settle(cx);
    let (release, hold) = async_channel::bounded::<()>(1);
    workbench.update(cx, |workbench, cx| {
        let picker = workbench.nav_state.symbols.clone();
        picker.update(cx, |picker, _| picker.delegate.hold = Some(hold));
    });
    cx.simulate_input("hlp");
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(other, window, cx))
    });
    drop(release);
    settle(cx);
    assert_eq!(symbols(&workbench, cx), []);
    let drawn = workbench.read_with(cx, |workbench, cx| {
        workbench.status_bar().read(cx).drawn(StatusSide::Left)
    });
    assert!(drawn.iter().all(|(id, _)| id != SOURCE_ITEM), "{drawn:?}");
}
