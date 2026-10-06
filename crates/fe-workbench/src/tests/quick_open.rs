use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

use fe_project::{FinderIndex, FinderMatch, FsChange, fixture};
use gpui::{ClipboardItem, Entity, Focusable as _, Keystroke, TestAppContext, VisualTestContext};

use super::{open, open_root, panel};
use crate::{Launch, Picker, QuickOpen, Workbench};

/// A fixture project with a few named files, one of them under a
/// non-ASCII directory with a non-ASCII name.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 2, 3).unwrap();
    fs::create_dir(dir.path().join("dïr")).unwrap();
    for name in ["a.txt", "cafe.rs", "dïr/café.rs", "src.rs"] {
        fs::write(dir.path().join(name), "").unwrap();
    }
    dir
}

/// A workbench rooted at `root`, focused as the app focuses it.
fn workbench_at<'a>(
    cx: &'a mut TestAppContext,
    root: Option<&Path>,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = match root {
        Some(root) => open_root(cx, root),
        None => {
            let launch = Launch {
                root: None,
                file: None,
            };
            let (workbench, cx, _) = open(cx, launch);
            (workbench, cx)
        }
    };
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx)
}

fn picker(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Picker<QuickOpen>> {
    workbench.read_with(cx, |workbench, _| workbench.quick_open().clone())
}

fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| workbench.palette_open())
}

fn input_focused(picker: &Entity<Picker<QuickOpen>>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| picker.focus_handle(cx).is_focused(window))
}

/// The walks started and the queries run so far.
fn counters(picker: &Entity<Picker<QuickOpen>>, cx: &mut VisualTestContext) -> (usize, usize) {
    picker.read_with(cx, |picker, _| {
        (picker.delegate.walks, picker.delegate.queries)
    })
}

fn matches(picker: &Entity<Picker<QuickOpen>>, cx: &mut VisualTestContext) -> Vec<FinderMatch> {
    picker.read_with(cx, |picker, _| picker.delegate.matches().to_vec())
}

fn paths(picker: &Entity<Picker<QuickOpen>>, cx: &mut VisualTestContext) -> Vec<String> {
    let matches = matches(picker, cx);
    matches.into_iter().map(|found| found.path).collect()
}

/// What a complete index of `root` answers for `query`.
fn expected(root: &Path, query: &str) -> Vec<FinderMatch> {
    let mut index = FinderIndex::new(root);
    FinderIndex::walk(root, Arc::new(AtomicBool::new(false)), |batch| {
        index.extend(batch)
    });
    index.finish();
    index.query(query, 50)
}

/// Presses `keys` without running the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse(keys).unwrap(), cx);
    });
}

#[gpui::test]
fn the_index_is_built_on_first_open(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = workbench_at(cx, Some(dir.path()));
    let picker = picker(&workbench, cx);
    assert_eq!(counters(&picker, cx).0, 0, "no walk from Workbench::new");
    assert!(!shown(&workbench, cx));

    press("cmd-p", cx);
    assert!(shown(&workbench, cx));
    assert!(input_focused(&picker, cx));
    assert_eq!(counters(&picker, cx).0, 1);
    let status = picker.read_with(cx, |picker, _| picker.delegate.status_text());
    assert_eq!(status, "Indexing… 0 files");

    cx.run_until_parked();
    let files = expected(dir.path(), "").len();
    let progress = picker.read_with(cx, |picker, _| picker.delegate.index_progress());
    assert_eq!(progress, Some((files, true)));
    assert_eq!(
        paths(&picker, cx).len(),
        files,
        "the empty query lists them"
    );

    cx.simulate_keystrokes("escape");
    assert!(!shown(&workbench, cx));
    cx.simulate_keystrokes("cmd-p");
    assert!(shown(&workbench, cx));
    assert!(input_focused(&picker, cx));
    assert_eq!(counters(&picker, cx).0, 1, "no second walk");
    // Cmd-P toggles.
    cx.simulate_keystrokes("cmd-p");
    assert!(!shown(&workbench, cx));
}

#[gpui::test]
fn typing_filters_and_highlights(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = workbench_at(cx, Some(dir.path()));
    let picker = picker(&workbench, cx);
    cx.simulate_keystrokes("cmd-p");

    cx.simulate_input("caf");
    let expected = expected(dir.path(), "caf");
    assert_eq!(matches(&picker, cx), expected);
    let paths = paths(&picker, cx);
    assert!(paths.contains(&"dïr/café.rs".to_string()), "{paths:?}");
    let status = picker.read_with(cx, |picker, _| picker.delegate.status_text());
    assert_eq!(status, format!("{} results", expected.len()));

    for (ix, found) in expected.iter().enumerate() {
        let label = picker.read_with(cx, |picker, _| picker.delegate.label(ix).unwrap());
        assert_eq!(label.text.as_ref(), found.path);
        let highlighted: Vec<usize> = label
            .text
            .char_indices()
            .enumerate()
            .filter(|(_, (byte, _))| label.highlights.iter().any(|range| range.contains(byte)))
            .map(|(ix, _)| ix)
            .collect();
        assert_eq!(highlighted, found.indices, "{}", found.path);
    }
}

#[gpui::test]
fn input_commands_edit_the_query(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = workbench_at(cx, Some(dir.path()));
    let picker = picker(&workbench, cx);
    let input = picker.read_with(cx, |picker, _| picker.input().clone());
    let query =
        |cx: &mut VisualTestContext| input.read_with(cx, |input, _| input.text().to_owned());
    cx.simulate_keystrokes("cmd-p");

    cx.simulate_input("dir/fil");
    cx.simulate_keystrokes("backspace");
    assert_eq!(query(cx), "dir/fi");
    assert_eq!(matches(&picker, cx), expected(dir.path(), "dir/fi"));
    cx.simulate_keystrokes("alt-backspace");
    assert_eq!(query(cx), "dir/");
    cx.simulate_keystrokes("alt-backspace");
    assert_eq!(query(cx), "dir");
    cx.simulate_keystrokes("cmd-backspace");
    assert_eq!(query(cx), "");
    cx.write_to_clipboard(ClipboardItem::new_string("src\nsecond line".into()));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(query(cx), "src");
    assert_eq!(matches(&picker, cx), expected(dir.path(), "src"));
}

fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<PathBuf>, Option<usize>) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let paths = pane
            .tabs()
            .iter()
            .map(|tab| tab.path.clone().unwrap())
            .collect();
        (paths, pane.active())
    })
}

/// Whether the active tab's editor is focused.
fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("an editor shows");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn keys_move_and_confirm(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = workbench_at(cx, Some(root));
    let picker = picker(&workbench, cx);
    let selected = |cx: &mut VisualTestContext| picker.read_with(cx, |picker, _| picker.selected());

    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("rs");
    let rows = paths(&picker, cx);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert_eq!(selected(cx), 0);
    cx.simulate_keystrokes("up");
    assert_eq!(selected(cx), 0, "clamped at the top");
    cx.simulate_keystrokes("down down");
    assert_eq!(selected(cx), 2);
    cx.simulate_keystrokes("down");
    assert_eq!(selected(cx), 2, "clamped at the bottom");
    cx.simulate_keystrokes("up ctrl-n ctrl-p ctrl-p");
    assert_eq!(selected(cx), 0);

    // Enter opens the selected file, focused, and closes the palette.
    cx.simulate_keystrokes("down enter");
    let opened = root.join(&rows[1]);
    assert!(!shown(&workbench, cx));
    assert_eq!(tabs(&workbench, cx), (vec![opened.clone()], Some(0)));
    assert!(editor_focused(&workbench, cx));

    // Another file, then the first again: its tab activates.
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("a.txt");
    assert_eq!(paths(&picker, cx)[0], "a.txt");
    cx.simulate_keystrokes("enter");
    let a = root.join("a.txt");
    assert_eq!(
        tabs(&workbench, cx),
        (vec![opened.clone(), a.clone()], Some(1))
    );
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("rs");
    assert_eq!(paths(&picker, cx), rows);
    cx.simulate_keystrokes("down enter");
    assert_eq!(tabs(&workbench, cx), (vec![opened, a], Some(0)));
    assert!(editor_focused(&workbench, cx));

    // Escape gives focus back to the editor focused before.
    cx.simulate_keystrokes("cmd-p");
    assert!(input_focused(&picker, cx));
    cx.simulate_keystrokes("escape");
    assert!(!shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
}

#[gpui::test]
fn queries_stay_bounded_while_typing(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = workbench_at(cx, Some(dir.path()));
    let picker = picker(&workbench, cx);
    cx.simulate_keystrokes("cmd-p");
    let (_, before) = counters(&picker, cx);

    // simulate_input types every char, then runs the executor.
    cx.simulate_input("fil001");
    let (_, after) = counters(&picker, cx);
    assert!(after - before <= 2, "{} queries", after - before);
    assert_eq!(matches(&picker, cx), expected(dir.path(), "fil001"));
    assert!(!paths(&picker, cx).is_empty());
}

#[gpui::test]
fn watcher_batches_reach_the_index(cx: &mut TestAppContext) {
    let dir = project();
    let root = dir.path();
    let (workbench, cx) = workbench_at(cx, Some(root));
    let picker = picker(&workbench, cx);
    let panel = panel(&workbench, cx);
    let deliver = |changes: Vec<FsChange>, cx: &mut VisualTestContext| {
        panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
    };
    let held =
        |cx: &mut VisualTestContext| picker.read_with(cx, |picker, _| picker.delegate.held());
    let stats = |cx: &mut VisualTestContext| {
        picker.read_with(cx, |picker, _| picker.delegate.index_stats().unwrap())
    };

    // Before the index exists, a batch is dropped.
    deliver(vec![FsChange::Path(root.join("a.txt"))], cx);
    assert_eq!(held(cx), 0);

    // While the walk runs, a batch is held, then applied once it's done:
    // the apply walks the new directory once.
    press("cmd-p", cx);
    fs::create_dir(root.join("late")).unwrap();
    fs::write(root.join("late/held.rs"), "").unwrap();
    deliver(vec![FsChange::Path(root.join("late"))], cx);
    assert_eq!(held(cx), 1);
    cx.run_until_parked();
    assert_eq!(held(cx), 0);
    assert_eq!(stats(cx).walks, 1);
    cx.simulate_input("held");
    assert_eq!(paths(&picker, cx), ["late/held.rs"]);

    // After the walk, a created file is found and a deleted one isn't.
    fs::write(root.join("newfile.rs"), "").unwrap();
    deliver(vec![FsChange::Path(root.join("newfile.rs"))], cx);
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("newfile");
    assert_eq!(paths(&picker, cx), ["newfile.rs"]);
    fs::remove_file(root.join("late/held.rs")).unwrap();
    deliver(vec![FsChange::Path(root.join("late/held.rs"))], cx);
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("held");
    assert_eq!(paths(&picker, cx), Vec::<String>::new());
}

#[gpui::test]
fn no_folder_open(cx: &mut TestAppContext) {
    let (workbench, cx) = workbench_at(cx, None);
    let picker = picker(&workbench, cx);
    cx.simulate_keystrokes("cmd-p");
    assert!(shown(&workbench, cx));
    let status = picker.read_with(cx, |picker, _| picker.delegate.status_text());
    assert_eq!(status, "No folder open");
    cx.simulate_input("a");
    assert_eq!(counters(&picker, cx), (0, 0), "no walk and no query");
    assert!(paths(&picker, cx).is_empty());
}

#[test]
fn the_default_keymap_binds_known_commands() {
    let store = fe_keymap::KeymapStore::load(
        fe_keymap::DEFAULT_MACOS,
        None,
        Some(crate::command_catalog()),
    );
    assert_eq!(store.diagnostics(), []);
    let bound = |command: &str| {
        let id = fe_command::CommandId::parse(command).unwrap();
        !store.keymap().bindings_for(&id).is_empty()
    };
    for command in [
        "quick_open.toggle",
        "picker.select_prev",
        "picker.select_next",
        "picker.confirm",
        "picker.dismiss",
        "text_input.backspace",
        "text_input.delete_word_left",
        "text_input.clear",
        "text_input.paste",
    ] {
        assert!(bound(command), "{command}");
    }
}
