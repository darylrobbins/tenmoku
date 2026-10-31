//! Filtering the project tree by name with vim's `/`, on a temporary
//! folder: the box taking typing and the rows narrowing on each key, Enter
//! keeping the filter with j and k stepping through the matches, escape
//! and shift-f clearing it, and the tree's operations on a filtered list.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_gpui::{EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::filter::NO_MATCHES;
use crate::{ProjectPanel, Workbench};

/// The project the tests filter: `unlisted` is never expanded, so never
/// listed.
const FILES: &[&str] = &[
    "README.md",
    "docs/guide.md",
    "src/lib.rs",
    "src/main.rs",
    "src/util/Main_helpers.rs",
    "tests/main_test.rs",
    "unlisted/main_hidden.rs",
    "web/objects.js",
];

/// The files of [`FILES`] under `root`, each holding its path.
fn write_files(root: &Path) {
    for rel in FILES {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("{rel}\n")).unwrap();
    }
}

/// A workbench rooted at `root`, vim on, in an active window (so the
/// box's blur is dispatched), with every folder but `unlisted` listed and
/// collapsed again, and the panel focused.
fn open_root<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<ProjectPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = crate::tests::open_root(cx, root);
    cx.update(|window, cx| {
        window.activate_window();
        set_vim_enabled(cx, true);
    });
    cx.run_until_parked();
    let panel = focus_panel(&workbench, cx);
    for rel in [
        "docs/guide.md",
        "src/util/Main_helpers.rs",
        "tests/main_test.rs",
        "web/objects.js",
    ] {
        select(&panel, root, rel, cx);
    }
    press("shift-w", cx);
    assert_eq!(
        list(&panel, cx),
        rows(&[
            (0, "docs"),
            (0, "src"),
            (0, "tests"),
            (0, "unlisted"),
            (0, "web"),
            (0, "README.md"),
        ])
    );
    (workbench, panel, cx)
}

/// Focuses the workbench, as the app does, then the panel through
/// `dock.show_project`.
fn focus_panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectPanel> {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-e");
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    assert!(panel_focused(&panel, cx), "dock.show_project focuses it");
    panel
}

fn panel_focused(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
}

fn box_focused(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| panel.read(cx).filter_box_focused(window, cx))
}

/// Whether the key context where focus is carries the panel's `vim` flag.
fn vim_keys(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, _| {
        let stack = window.context_stack();
        stack.iter().any(|context| context.contains("vim"))
    })
}

/// Presses `keys`, then runs the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// Types `text` into what has focus, a character at a time.
fn type_text(text: &str, cx: &mut VisualTestContext) {
    for ch in text.chars() {
        cx.simulate_input(&ch.to_string());
        cx.run_until_parked();
    }
}

fn rows(rows: &[(usize, &str)]) -> Vec<(usize, String)> {
    (rows.iter())
        .map(|&(depth, name)| (depth, name.to_owned()))
        .collect()
}

/// The rows the list shows as `(depth, name)`.
fn list(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
    panel.read_with(cx, |panel, _| panel.list_names())
}

/// The names of the rows the panel drew, once it's drawn again, each once
/// (the list builds its first row twice, to measure it).
fn drawn(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let mut drawn: Vec<String> = panel.read_with(cx, |panel, _| {
        (panel.drawn_status().into_iter())
            .map(|(name, _)| name)
            .collect()
    });
    drawn.dedup();
    drawn
}

/// The names of the expanded folders, in the tree's order.
fn expanded(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        (tree.rows(0..tree.row_count()))
            .filter(|row| row.expanded)
            .map(|row| row.name.to_string_lossy().into_owned())
            .collect()
    })
}

/// The selected row's name.
fn selected(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| {
        let ix = panel.selected_row()?;
        let tree = panel.tree()?;
        let row = tree.rows(ix..ix + 1).next()?;
        Some(row.name.to_string_lossy().into_owned())
    })
}

/// Selects the entry at `rel`, expanding the folders above it.
fn select(panel: &Entity<ProjectPanel>, root: &Path, rel: &str, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| panel.reveal(root.join(rel), cx));
    cx.run_until_parked();
    let name = rel.rsplit('/').next().unwrap();
    assert_eq!(selected(panel, cx).as_deref(), Some(name));
}

fn filter_text(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| panel.filter_text().map(str::to_owned))
}

/// The hidden row the line above the rows names, if any.
fn hides(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| panel.filter_hides().map(str::to_owned))
}

fn message(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<&'static str> {
    panel.read_with(cx, |panel, _| panel.list_message())
}

/// The rows "main" leaves: the matches, and the folders above them.
fn main_rows() -> Vec<(usize, String)> {
    rows(&[
        (0, "src"),
        (1, "util"),
        (2, "Main_helpers.rs"),
        (1, "main.rs"),
        (0, "tests"),
        (1, "main_test.rs"),
    ])
}

/// Typing in the box narrows the rows on each key, to the names holding
/// what's typed, ignoring case, among the listed rows, with their folders
/// expanded; vim's tree keys don't see it. Nothing matching shows a row
/// saying so.
#[gpui::test]
fn live(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path());
    let root = dir.path();
    let (_workbench, panel, cx) = open_root(cx, root);
    select(&panel, root, "README.md", cx);
    assert!(vim_keys(cx));

    // `/` opens the box, focused, and the panel's vim keys go.
    press("/", cx);
    assert!(box_focused(&panel, cx));
    assert!(!vim_keys(cx));
    assert_eq!(filter_text(&panel, cx).as_deref(), Some(""));
    // Nothing typed yet: every row shows.
    assert_eq!(list(&panel, cx).len(), 6);

    // `j` is typed, not the tree's select-next: the rows are those holding
    // a "j", with the folder above, and the match is selected.
    press("j", cx);
    assert_eq!(filter_text(&panel, cx).as_deref(), Some("j"));
    assert_eq!(list(&panel, cx), rows(&[(0, "web"), (1, "objects.js")]));
    assert_eq!(selected(&panel, cx).as_deref(), Some("objects.js"));
    assert_eq!(drawn(&panel, cx), ["web", "objects.js"]);
    // `k` too: no name holds "jk".
    press("k", cx);
    assert_eq!(filter_text(&panel, cx).as_deref(), Some("jk"));
    assert_eq!(list(&panel, cx), []);
    assert_eq!(message(&panel, cx), Some(NO_MATCHES));
    assert_eq!(drawn(&panel, cx), Vec::<String>::new());
    press("backspace backspace", cx);
    assert_eq!(filter_text(&panel, cx).as_deref(), Some(""));
    assert_eq!(list(&panel, cx).len(), 6);
    assert_eq!(expanded(&panel, cx), Vec::<String>::new());

    // Each key narrows the rows, ignoring case. The listed folders are
    // searched, collapsed or not, and expanded to show their matches;
    // `unlisted`, never listed, isn't searched.
    type_text("M", cx);
    assert!(list(&panel, cx).contains(&(0, "README.md".to_owned())));
    type_text("A", cx);
    assert_eq!(list(&panel, cx), main_rows());
    type_text("iN", cx);
    assert_eq!(filter_text(&panel, cx).as_deref(), Some("MAiN"));
    assert_eq!(list(&panel, cx), main_rows());
    assert_eq!(expanded(&panel, cx), ["src", "util", "tests"]);
    assert_eq!(
        drawn(&panel, cx),
        [
            "src",
            "util",
            "Main_helpers.rs",
            "main.rs",
            "tests",
            "main_test.rs"
        ]
    );
    assert_eq!(selected(&panel, cx).as_deref(), Some("Main_helpers.rs"));
    assert!(box_focused(&panel, cx));
    assert_eq!(message(&panel, cx), None);

    // A folder the filter expanded collapses once nothing under it
    // matches.
    type_text("_t", cx);
    assert_eq!(list(&panel, cx), rows(&[(0, "tests"), (1, "main_test.rs")]));
    assert_eq!(expanded(&panel, cx), ["tests"]);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main_test.rs"));

    // A name matching nowhere leaves the one row saying so.
    type_text("zz", cx);
    assert_eq!(list(&panel, cx), []);
    assert_eq!(message(&panel, cx), Some(NO_MATCHES));
    assert_eq!(drawn(&panel, cx), Vec::<String>::new());
    assert!(NO_MATCHES.starts_with("No matches"));
}

/// Enter keeps the filter, giving the tree its keys back, and j and k (gg
/// and G) step through the matching rows only. Escape in the box, or
/// shift-f, clears it, restoring the folders expanded and the row selected
/// before.
#[gpui::test]
fn keep_and_clear(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path());
    let root = dir.path();
    let (_workbench, panel, cx) = open_root(cx, root);
    select(&panel, root, "docs/guide.md", cx);
    let before = list(&panel, cx);
    assert_eq!(expanded(&panel, cx), ["docs"]);

    press("/", cx);
    type_text("main", cx);
    press("enter", cx);
    assert!(panel_focused(&panel, cx));
    assert!(vim_keys(cx));
    assert_eq!(filter_text(&panel, cx).as_deref(), Some("main"));
    assert_eq!(list(&panel, cx), main_rows());
    assert_eq!(selected(&panel, cx).as_deref(), Some("Main_helpers.rs"));

    // j and k pass over the folders: only matches are selected.
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main.rs"));
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main_test.rs"));
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main_test.rs"));
    press("k", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main.rs"));
    press("k k", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("Main_helpers.rs"));
    press("shift-g", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main_test.rs"));
    press("g g", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("Main_helpers.rs"));
    // From a folder (h selects the parent), j goes on to the next match.
    press("h", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("util"));
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("Main_helpers.rs"));
    // `/` again opens the box on the filter's text.
    press("/", cx);
    assert!(box_focused(&panel, cx));
    assert_eq!(filter_text(&panel, cx).as_deref(), Some("main"));
    press("enter", cx);
    assert_eq!(filter_text(&panel, cx).as_deref(), Some("main"));
    assert_eq!(list(&panel, cx), main_rows());

    // shift-f clears it: the rows, expansion and selection are back.
    press("shift-f", cx);
    assert_eq!(filter_text(&panel, cx), None);
    assert_eq!(list(&panel, cx), before);
    assert_eq!(expanded(&panel, cx), ["docs"]);
    assert_eq!(selected(&panel, cx).as_deref(), Some("guide.md"));
    assert!(panel_focused(&panel, cx));
    assert_eq!(drawn(&panel, cx).len(), before.len());

    // So does escape in the box.
    press("/", cx);
    type_text("tes", cx);
    assert_eq!(list(&panel, cx), rows(&[(0, "tests"), (1, "main_test.rs")]));
    press("escape", cx);
    assert_eq!(filter_text(&panel, cx), None);
    assert!(panel_focused(&panel, cx));
    assert!(vim_keys(cx));
    assert_eq!(list(&panel, cx), before);
    assert_eq!(expanded(&panel, cx), ["docs"]);
    assert_eq!(selected(&panel, cx).as_deref(), Some("guide.md"));

    // Enter on an empty box clears it too.
    press("/", cx);
    press("enter", cx);
    assert_eq!(filter_text(&panel, cx), None);
    assert_eq!(list(&panel, cx), before);
}

/// Whether the active pane's editor is of `path`.
fn active_path(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    let editor: Entity<EditorView> =
        workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())?;
    editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
}

/// With the filter kept, o, Enter and r act on the selected match as they
/// do unfiltered, and a rename taking the last match away leaves the
/// filter set, saying nothing matches.
#[gpui::test]
fn ops(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path());
    let root = &fs::canonicalize(dir.path()).unwrap();
    let (workbench, panel, cx) = open_root(cx, root);

    press("/", cx);
    type_text("main", cx);
    press("enter", cx);
    // o and Enter open the selected match.
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main.rs"));
    press("o", cx);
    assert_eq!(active_path(&workbench, cx), Some(root.join("src/main.rs")));
    focus_panel(&workbench, cx);
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main_test.rs"));
    press("enter", cx);
    assert_eq!(
        active_path(&workbench, cx),
        Some(root.join("tests/main_test.rs"))
    );
    focus_panel(&workbench, cx);
    // o on a folder collapses it, as unfiltered, hiding its matches.
    press("h", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("tests"));
    press("o", cx);
    assert_eq!(expanded(&panel, cx), ["src", "util"]);
    assert_eq!(list(&panel, cx), main_rows()[..5]);
    press("o", cx);
    assert_eq!(list(&panel, cx), main_rows());

    // r renames the selected match, the filter staying.
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main_test.rs"));
    press("r", cx);
    let field = panel.read_with(cx, |panel, cx| panel.name_field(cx));
    assert_eq!(field, Some(("main_test.rs".to_owned(), None)));
    cx.simulate_input("main_spec");
    press("enter", cx);
    assert!(root.join("tests/main_spec.rs").is_file());
    assert_eq!(filter_text(&panel, cx).as_deref(), Some("main"));
    assert_eq!(selected(&panel, cx).as_deref(), Some("main_spec.rs"));
    assert_eq!(
        list(&panel, cx).last(),
        Some(&(1, "main_spec.rs".to_owned()))
    );

    // A rename taking the last match away leaves the filter set, with
    // nothing matching.
    press("shift-f", cx);
    press("/", cx);
    type_text("guide", cx);
    press("enter", cx);
    assert_eq!(list(&panel, cx), rows(&[(0, "docs"), (1, "guide.md")]));
    assert_eq!(selected(&panel, cx).as_deref(), Some("guide.md"));
    press("r", cx);
    cx.simulate_input("intro");
    press("enter", cx);
    assert!(root.join("docs/intro.md").is_file());
    assert_eq!(filter_text(&panel, cx).as_deref(), Some("guide"));
    assert_eq!(list(&panel, cx), []);
    assert_eq!(message(&panel, cx), Some(NO_MATCHES));
    assert_eq!(drawn(&panel, cx), Vec::<String>::new());
    // The renamed row, hidden, isn't one the tree's keys act on; the line
    // above the rows names it.
    assert_eq!(selected(&panel, cx), None);
    assert_eq!(hides(&panel, cx).as_deref(), Some("intro.md"));
    // Cleared, the folders it expanded collapse again, the renamed file in
    // one of them.
    press("shift-f", cx);
    assert_eq!(filter_text(&panel, cx), None);
    assert_eq!(expanded(&panel, cx), Vec::<String>::new());
    select(&panel, root, "docs/intro.md", cx);
}

/// A temporary project for the trash tests, its name a prefix of their own
/// files' names, to find them in the test trash by.
fn trashable_project() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("fe-workbench-trash-")
        .tempdir()
        .unwrap()
}

/// The one entry named `name` in the test trash, where every trash of a
/// test build goes (`fe_project::fs_ops::testing`).
fn in_test_trash(name: &str) -> PathBuf {
    let found = fe_project::fs_ops::testing::trashed(name);
    assert_eq!(found.len(), 1, "{name} in the test trash: {found:?}");
    found.into_iter().next().unwrap()
}

/// With the filter kept, d moves the selected match to the Trash as it
/// does unfiltered, selecting the next match; moving the last match leaves
/// the filter set, saying nothing matches.
#[gpui::test]
fn ops_delete(cx: &mut TestAppContext) {
    let dir = trashable_project();
    // Names of the test's own, to find them in the trash.
    let unique = (dir.path().file_name().unwrap().to_string_lossy()).into_owned();
    let root = &fs::canonicalize(dir.path()).unwrap();
    write_files(root);
    let first = format!("{unique}-main.rs");
    let last = format!("{unique}-only.txt");
    fs::write(root.join("src").join(&first), "").unwrap();
    fs::write(root.join("web").join(&last), "").unwrap();
    let (_workbench, panel, cx) = open_root(cx, root);

    press("/", cx);
    type_text(&format!("{unique}-"), cx);
    press("enter", cx);
    assert_eq!(
        list(&panel, cx),
        [
            (0, "src".to_owned()),
            (1, first.clone()),
            (0, "web".to_owned()),
            (1, last.clone()),
        ]
    );
    assert_eq!(selected(&panel, cx), Some(first.clone()));

    // d asks, then moves the match; the next match is selected.
    press("d", cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert!(!root.join("src").join(&first).exists());
    in_test_trash(&first);
    assert_eq!(list(&panel, cx), [(0, "web".to_owned()), (1, last.clone())]);
    assert_eq!(selected(&panel, cx), Some(last.clone()));

    // Moving the last leaves the filter set, with nothing matching.
    press("d", cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert!(!root.join("web").join(&last).exists());
    in_test_trash(&last);
    assert_eq!(filter_text(&panel, cx), Some(format!("{unique}-")));
    assert_eq!(list(&panel, cx), []);
    assert_eq!(message(&panel, cx), Some(NO_MATCHES));
    assert_eq!(drawn(&panel, cx), Vec::<String>::new());
    // No row the list doesn't show is selected in its place.
    assert_eq!(selected(&panel, cx), None);
    press("d", cx);
    assert!(!cx.has_pending_prompt());
}

/// A row the filter hides takes none of the tree's keys: with nothing
/// matching, o, Enter, r and d do nothing to the row selected before; a
/// reveal of a hidden file selects nothing the keys act on, and the line
/// above the rows names it.
#[gpui::test]
fn hidden_rows_take_no_ops(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path());
    let root = &fs::canonicalize(dir.path()).unwrap();
    let (workbench, panel, cx) = open_root(cx, root);
    select(&panel, root, "README.md", cx);

    press("/", cx);
    type_text("zzz", cx);
    press("enter", cx);
    assert_eq!(message(&panel, cx), Some(NO_MATCHES));
    assert_eq!(selected(&panel, cx), None);
    for keys in ["o", "enter", "l"] {
        press(keys, cx);
        assert_eq!(active_path(&workbench, cx), None, "{keys}");
    }
    press("r", cx);
    assert_eq!(panel.read_with(cx, |panel, cx| panel.name_field(cx)), None);
    press("d", cx);
    assert!(!cx.has_pending_prompt());
    assert!(root.join("README.md").is_file());

    // Revealed, a file the filter hides isn't selected for the keys.
    press("shift-f", cx);
    press("/", cx);
    type_text("main", cx);
    press("enter", cx);
    panel.update(cx, |panel, cx| panel.reveal(root.join("docs/guide.md"), cx));
    cx.run_until_parked();
    assert_eq!(selected(&panel, cx), None);
    assert_eq!(hides(&panel, cx).as_deref(), Some("guide.md"));
    assert_eq!(list(&panel, cx), main_rows());
    press("o", cx);
    assert_eq!(active_path(&workbench, cx), None);
    // j goes to the first match from there.
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("Main_helpers.rs"));
    // Changing the filter's text forgets it.
    press("/", cx);
    type_text("_", cx);
    assert_eq!(hides(&panel, cx), None);
}

/// A tree with no row selected before the filter has none once it's
/// cleared, by escape or by shift-f.
#[gpui::test]
fn clear_restores_no_selection(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path());
    let (workbench, cx) = crate::tests::open_root(cx, dir.path());
    cx.update(|window, cx| {
        window.activate_window();
        set_vim_enabled(cx, true);
    });
    cx.run_until_parked();
    let panel = focus_panel(&workbench, cx);
    assert_eq!(selected(&panel, cx), None);
    let before = list(&panel, cx);

    for clear in ["escape", "enter shift-f"] {
        press("/", cx);
        type_text("readme", cx);
        assert_eq!(list(&panel, cx), rows(&[(0, "README.md")]));
        assert_eq!(selected(&panel, cx).as_deref(), Some("README.md"));
        press(clear, cx);
        assert_eq!(filter_text(&panel, cx), None, "{clear}");
        assert_eq!(panel.read_with(cx, |panel, _| panel.selected), None);
        assert_eq!(list(&panel, cx), before, "{clear}");
    }
}

/// Clearing drops what waits for a listing to expand since the filter
/// was set: a folder whose listing lands afterwards stays collapsed.
#[gpui::test]
fn clear_cancels_pending_listings(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path());
    let root = dir.path();
    let (_workbench, panel, cx) = open_root(cx, root);
    select(&panel, root, "README.md", cx);
    press("/", cx);
    type_text("unlisted", cx);
    press("enter", cx);
    assert_eq!(list(&panel, cx), rows(&[(0, "unlisted")]));
    assert_eq!(selected(&panel, cx).as_deref(), Some("unlisted"));

    // o asks for its listing, and shift-f comes before it lands.
    cx.update(|window, cx| {
        window.dispatch_keystroke(gpui::Keystroke::parse("o").unwrap(), cx);
        let waiting = panel.read_with(cx, |panel, _| {
            let tree = panel.tree().unwrap();
            let id = tree.find("unlisted").unwrap();
            tree.listing(id).is_some() && panel.expanding.contains(&id)
        });
        assert!(waiting, "o waits for the listing");
        window.dispatch_keystroke(gpui::Keystroke::parse("shift-f").unwrap(), cx);
    });
    assert_eq!(filter_text(&panel, cx), None);
    cx.run_until_parked();
    assert_eq!(expanded(&panel, cx), Vec::<String>::new());
    assert!(!list(&panel, cx).contains(&(1, "main_hidden.rs".to_owned())));
    assert_eq!(selected(&panel, cx).as_deref(), Some("README.md"));
}

/// A match the tree gains under a listed, collapsed folder (a watcher
/// batch) has its folders expanded, so j reaches it; a folder collapsed
/// under the filter stays collapsed when nothing new matches below it.
#[gpui::test]
fn new_matches_expand_their_folders(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path());
    let root = &fs::canonicalize(dir.path()).unwrap();
    let (_workbench, panel, cx) = open_root(cx, root);
    press("/", cx);
    type_text("main", cx);
    press("enter", cx);
    assert_eq!(list(&panel, cx), main_rows());
    // `tests` collapsed under the filter.
    press("shift-g h o", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("tests"));
    assert_eq!(expanded(&panel, cx), ["src", "util"]);

    // `docs`, listed and collapsed, gains a match.
    let new = root.join("docs/main_notes.md");
    fs::write(&new, "").unwrap();
    panel.update(cx, |panel, cx| {
        panel.apply_fs_changes(&[fe_project::FsChange::Path(new)], cx)
    });
    cx.run_until_parked();
    assert_eq!(expanded(&panel, cx), ["docs", "src", "util"]);
    assert_eq!(
        list(&panel, cx),
        rows(&[
            (0, "docs"),
            (1, "main_notes.md"),
            (0, "src"),
            (1, "util"),
            (2, "Main_helpers.rs"),
            (1, "main.rs"),
            (0, "tests"),
        ])
    );
    press("g g", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("main_notes.md"));
    press("j", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("Main_helpers.rs"));

    // Cleared, `docs` collapses again with the rest.
    press("shift-f", cx);
    assert_eq!(expanded(&panel, cx), Vec::<String>::new());
}
