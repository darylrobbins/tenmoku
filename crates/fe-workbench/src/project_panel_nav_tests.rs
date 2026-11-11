//! nvim-tree's tree keys with vim on, on a temporary folder: `o` and Enter
//! toggling a folder or opening a file, expanding and collapsing every
//! folder, toggling ignored entries and refreshing from disk.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_gpui::{EditorView, set_vim_enabled};
use fe_project::EntryKind;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::nav::EXPAND_ALL_LIMIT;
use crate::{ProjectPanel, Workbench};

/// Empty files at `paths` (relative), and the folders they're in, in a
/// temporary directory.
fn project(paths: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for rel in paths {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    dir
}

/// A workbench rooted at `root` with vim on, in an active window.
fn open_root<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = crate::tests::open_root(cx, root);
    cx.update(|window, cx| {
        window.activate_window();
        set_vim_enabled(cx, true);
    });
    cx.run_until_parked();
    (workbench, cx)
}

/// Focuses the workbench, as the app does, then the panel through
/// `dock.show_project`.
fn focus_panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectPanel> {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-e");
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    assert!(
        cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)),
        "dock.show_project focuses the panel"
    );
    panel
}

/// Presses `keys`, then runs the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// The panel's rows as `(depth, name)`.
fn rows(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
    panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        tree.rows(0..tree.row_count())
            .map(|row| (row.depth, row.name.to_string_lossy().into_owned()))
            .collect()
    })
}

/// The expanded folders' rows, as `(depth, name)`.
fn expanded(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
    panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        tree.rows(0..tree.row_count())
            .filter(|row| row.kind == EntryKind::Dir && row.expanded)
            .map(|row| (row.depth, row.name.to_string_lossy().into_owned()))
            .collect()
    })
}

/// The selected row, as `(depth, name)`.
fn selected(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<(usize, String)> {
    let ix = panel.read_with(cx, |panel, _| panel.selected_row())?;
    Some(rows(panel, cx)[ix].clone())
}

/// Selects `rel`'s row, expanding the folders it's in.
fn select(panel: &Entity<ProjectPanel>, root: &Path, rel: &str, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| panel.reveal(root.join(rel), cx));
    cx.run_until_parked();
    let name = rel.rsplit('/').next().unwrap();
    assert_eq!(
        selected(panel, cx).map(|(_, row)| row).as_deref(),
        Some(name),
        "{:?}",
        rows(panel, cx)
    );
}

fn active_editor(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<EditorView>> {
    workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
}

fn active_path(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    let editor = active_editor(workbench, cx)?;
    editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
}

fn status(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| panel.status_text())
}

#[gpui::test]
fn toggle(cx: &mut TestAppContext) {
    let dir = project(&["a/b.txt", "a/c/d.txt", "z.txt", "y.txt"]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root);
    let panel = focus_panel(&workbench, cx);
    assert_eq!(
        rows(&panel, cx),
        [(0, "a".into()), (0, "y.txt".into()), (0, "z.txt".into())]
    );
    press("j", cx);
    assert_eq!(selected(&panel, cx), Some((0, "a".into())));

    // `o` and Enter each expand a collapsed folder and collapse an expanded
    // one; the folder stays selected.
    for keys in ["o", "enter"] {
        press(keys, cx);
        assert_eq!(
            rows(&panel, cx),
            [
                (0, "a".into()),
                (1, "c".into()),
                (1, "b.txt".into()),
                (0, "y.txt".into()),
                (0, "z.txt".into()),
            ],
            "{keys} expands"
        );
        assert_eq!(selected(&panel, cx), Some((0, "a".into())), "{keys}");
        press(keys, cx);
        assert_eq!(rows(&panel, cx).len(), 3, "{keys} collapses");
        assert_eq!(selected(&panel, cx), Some((0, "a".into())), "{keys}");
    }

    // On a file each opens it in the editor.
    for (keys, file) in [("o", "y.txt"), ("enter", "z.txt")] {
        let panel = focus_panel(&workbench, cx);
        select(&panel, root, file, cx);
        press(keys, cx);
        assert_eq!(active_path(&workbench, cx), Some(root.join(file)), "{keys}");
    }

    // `l` and `h` keep their meaning: `l` toggles the folder as Enter does,
    // `h` collapses it, or on a row in it selects it.
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "a", cx);
    press("l", cx);
    assert!(rows(&panel, cx).contains(&(1, "b.txt".into())), "l expands");
    press("j j", cx);
    assert_eq!(selected(&panel, cx), Some((1, "b.txt".into())));
    press("h", cx);
    assert_eq!(
        selected(&panel, cx),
        Some((0, "a".into())),
        "h selects the parent"
    );
    press("h", cx);
    assert_eq!(rows(&panel, cx).len(), 3, "h collapses");

    // With vim off, `o` does nothing and Enter still toggles and opens.
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "a", cx);
    press("o", cx);
    assert_eq!(rows(&panel, cx).len(), 3, "o is no key with vim off");
    press("enter", cx);
    assert!(
        rows(&panel, cx).contains(&(1, "b.txt".into())),
        "enter expands"
    );
    assert_eq!(selected(&panel, cx), Some((0, "a".into())));
    press("enter", cx);
    assert_eq!(rows(&panel, cx).len(), 3, "enter collapses");
    select(&panel, root, "a/b.txt", cx);
    press("enter", cx);
    assert_eq!(active_path(&workbench, cx), Some(root.join("a/b.txt")));
}

#[gpui::test]
fn all(cx: &mut TestAppContext) {
    let dir = project(&["p/q/r/f.txt", "p/g.txt", "s/t.txt", "u.txt"]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root);
    let panel = focus_panel(&workbench, cx);
    press("j", cx);
    assert_eq!(selected(&panel, cx), Some((0, "p".into())));

    // `E` lists and expands every folder, a level at a time; the selection
    // stays.
    press("shift-e", cx);
    assert_eq!(
        rows(&panel, cx),
        [
            (0, "p".into()),
            (1, "q".into()),
            (2, "r".into()),
            (3, "f.txt".into()),
            (1, "g.txt".into()),
            (0, "s".into()),
            (1, "t.txt".into()),
            (0, "u.txt".into()),
        ]
    );
    assert_eq!(selected(&panel, cx), Some((0, "p".into())));
    assert_eq!(status(&panel, cx), None, "under the limit");

    // `W` collapses every folder, the selection moving to the root's child
    // it was in.
    select(&panel, root, "p/q/r/f.txt", cx);
    press("shift-w", cx);
    assert_eq!(
        rows(&panel, cx),
        [(0, "p".into()), (0, "s".into()), (0, "u.txt".into())]
    );
    assert_eq!(selected(&panel, cx), Some((0, "p".into())));
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.drawn_selected()),
        Some(0)
    );
    // Those below are collapsed too.
    press("l", cx);
    assert_eq!(
        rows(&panel, cx),
        [
            (0, "p".into()),
            (1, "q".into()),
            (1, "g.txt".into()),
            (0, "s".into()),
            (0, "u.txt".into()),
        ]
    );
    // A selection the collapse leaves shown stays.
    press("shift-e", cx);
    select(&panel, root, "s", cx);
    press("shift-w", cx);
    assert_eq!(rows(&panel, cx).len(), 3);
    assert_eq!(selected(&panel, cx), Some((0, "s".into())));
}

#[gpui::test]
fn expand_all_stops_at_its_limit(cx: &mut TestAppContext) {
    let names: Vec<String> = (0..EXPAND_ALL_LIMIT + 20)
        .map(|ix| format!("big/d{ix:03}/x.txt"))
        .collect();
    let paths: Vec<&str> = names.iter().map(String::as_str).chain(["a.txt"]).collect();
    let dir = project(&paths);
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = focus_panel(&workbench, cx);
    press("shift-e", cx);
    let shown = expanded(&panel, cx);
    assert_eq!(shown.len(), EXPAND_ALL_LIMIT);
    // Breadth-first: `big`, then its first folders.
    assert_eq!(shown[0], (0, "big".into()));
    assert_eq!(shown[1], (1, "d000".into()));
    assert_eq!(
        shown.last(),
        Some(&(1, format!("d{:03}", EXPAND_ALL_LIMIT - 2)))
    );
    assert_eq!(
        status(&panel, cx).as_deref(),
        Some("Expanded the first 300 folders: Expand All stops there")
    );
    // Collapsing them clears it.
    press("shift-w", cx);
    assert_eq!(status(&panel, cx), None);
    assert_eq!(rows(&panel, cx), [(0, "big".into()), (0, "a.txt".into())]);
}

#[gpui::test]
fn ignored_and_refresh(cx: &mut TestAppContext) {
    let dir = project(&["src/a.txt", "src/b/c.txt", "target/x.txt", "z.txt"]);
    let root = dir.path();
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "target\n").unwrap();
    let (workbench, cx) = open_root(cx, root);
    let panel = focus_panel(&workbench, cx);
    let lists_ignored = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.lists_ignored());
    assert!(!lists_ignored(cx));
    select(&panel, root, "src/b/c.txt", cx);
    let hidden = rows(&panel, cx);
    assert!(!hidden.contains(&(0, "target".into())), "{hidden:?}");

    // `H` lists the ignored entries, keeping the folders expanded and the
    // selected row; again, it stops.
    press("shift-h", cx);
    assert!(lists_ignored(cx));
    let shown = rows(&panel, cx);
    assert!(shown.contains(&(0, "target".into())), "{shown:?}");
    assert!(shown.contains(&(2, "c.txt".into())), "{shown:?}");
    assert_eq!(selected(&panel, cx), Some((2, "c.txt".into())));
    press("shift-h", cx);
    assert!(!lists_ignored(cx));
    assert_eq!(rows(&panel, cx), hidden);
    assert_eq!(selected(&panel, cx), Some((2, "c.txt".into())));

    // A selected row that's no longer listed isn't selected.
    press("shift-h", cx);
    select(&panel, root, "target", cx);
    press("shift-h", cx);
    assert!(!lists_ignored(cx));
    assert_eq!(selected(&panel, cx), None);

    // `g r` lists the tree again from disk, though no watcher event came:
    // what was created shows and what was deleted goes, the selection and
    // the expanded folders staying.
    select(&panel, root, "src/a.txt", cx);
    fs::write(root.join("src/b/new.txt"), "").unwrap();
    fs::write(root.join("new.txt"), "").unwrap();
    fs::remove_file(root.join("z.txt")).unwrap();
    assert_eq!(rows(&panel, cx), hidden, "no event, so nothing changed yet");
    press("g r", cx);
    assert_eq!(
        rows(&panel, cx),
        [
            (0, "src".into()),
            (1, "b".into()),
            (2, "c.txt".into()),
            (2, "new.txt".into()),
            (1, "a.txt".into()),
            (0, ".gitignore".into()),
            (0, "new.txt".into()),
        ]
    );
    assert_eq!(selected(&panel, cx), Some((1, "a.txt".into())));
}

/// Presses `keystroke` without running the executor, so listings and the
/// tree's opening stay pending.
fn press_now(keystroke: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.dispatch_keystroke(gpui::Keystroke::parse(keystroke).unwrap(), cx);
    });
}

#[gpui::test]
fn expand_all_waits_for_the_listings_it_starts(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let outer = dir.path().join("outer");
    let root = outer.join("project");
    fs::create_dir_all(root.join("p")).unwrap();
    fs::write(root.join("p/f.txt"), "").unwrap();
    let (workbench, cx) = open_root(cx, &root);
    let panel = focus_panel(&workbench, cx);
    select(&panel, &root, "p", cx);
    press("l", cx);
    assert_eq!(rows(&panel, cx), [(0, "p".into()), (1, "f.txt".into())]);

    // With a folder above the root away, a refresh's listings of the root
    // and of the expanded `p` fail, and wait for a change.
    let away = dir.path().join("away");
    fs::rename(&outer, &away).unwrap();
    press("g r", cx);
    let waiting = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().unwrap();
        let p = tree.find(root.join("p")).unwrap();
        [tree.root(), p].map(|id| panel.waits_to_retry(id))
    });
    assert_eq!(waiting, [true, true]);
    // Back, with folders no listing has seen, in the root and in `p`.
    fs::rename(&away, &outer).unwrap();
    for rel in ["p/q/g.txt", "r/h.txt"] {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    // `E` lists them again, and expands what they find.
    press("shift-e", cx);
    assert_eq!(
        rows(&panel, cx),
        [
            (0, "p".into()),
            (1, "q".into()),
            (2, "g.txt".into()),
            (1, "f.txt".into()),
            (0, "r".into()),
            (1, "h.txt".into()),
        ]
    );
    assert_eq!(selected(&panel, cx), Some((0, "p".into())));
}

#[gpui::test]
fn toggle_ignored_again_while_the_tree_opens(cx: &mut TestAppContext) {
    let dir = project(&["lib/d.txt", "src/a.txt", "src/b/c.txt", "target/x.txt"]);
    let root = dir.path();
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "target\n").unwrap();
    let (workbench, cx) = open_root(cx, root);
    let panel = focus_panel(&workbench, cx);
    // `lib` expanded too, though nothing in it is selected.
    select(&panel, root, "lib/d.txt", cx);
    select(&panel, root, "src/b/c.txt", cx);
    let before = rows(&panel, cx);
    assert!(before.contains(&(1, "d.txt".into())), "{before:?}");
    let lists_ignored = |cx: &mut VisualTestContext| panel.read_with(cx, |p, _| p.lists_ignored());

    // Twice before the first tree opens: the second still counts, and the
    // folders and the selection come back.
    press_now("shift-h", cx);
    assert!(
        panel.read_with(cx, |panel, _| panel.tree().is_none()),
        "opening"
    );
    press_now("shift-h", cx);
    cx.run_until_parked();
    assert!(!lists_ignored(cx));
    assert_eq!(rows(&panel, cx), before);
    assert_eq!(selected(&panel, cx), Some((2, "c.txt".into())));

    // Again once the tree is open but before its folders are listed: what
    // was still to expand, and to select, carries over.
    press_now("shift-h", cx);
    cx.run_until_parked();
    assert!(lists_ignored(cx));
    let shown = rows(&panel, cx);
    assert!(shown.contains(&(0, "target".into())), "{shown:?}");
    press_now("shift-h", cx);
    // Only until the new tree opens: `lib`, `src` and `b` are still to
    // list, and `c.txt` to select.
    while panel.read_with(cx, |panel, _| panel.tree().is_none()) {
        assert!(cx.executor().tick(), "the tree opens");
    }
    assert_eq!(
        rows(&panel, cx),
        [
            (0, "lib".into()),
            (0, "src".into()),
            (0, ".gitignore".into())
        ]
    );
    assert_eq!(selected(&panel, cx), None);
    press_now("shift-h", cx);
    cx.run_until_parked();
    assert!(lists_ignored(cx));
    assert_eq!(rows(&panel, cx), shown);
    assert_eq!(selected(&panel, cx), Some((2, "c.txt".into())));
}

#[gpui::test]
fn refresh_opens_a_root_that_failed_to_open(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("later");
    let (workbench, cx) = open_root(cx, &root);
    let panel = focus_panel(&workbench, cx);
    assert!(
        panel.read_with(cx, |panel, _| panel.tree().is_none()),
        "failed"
    );

    fs::create_dir(&root).unwrap();
    fs::write(root.join("a.txt"), "").unwrap();
    press("g r", cx);
    assert_eq!(rows(&panel, cx), [(0, "a.txt".into())]);
}
