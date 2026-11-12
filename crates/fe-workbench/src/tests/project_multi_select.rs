//! Selecting several rows of the project tree (T1752): shift-click,
//! cmd-click and shift-arrows select them, a plain click or a filter
//! selects one again, and delete, cut and paste, copy path and duplicate
//! act on every one, naming in one toast those that failed.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{open_root, panel, rows};
use crate::{ProjectPanel, Workbench};

/// Files at `paths` (relative) holding their own path, under a temporary
/// folder whose name starts `fe-workbench-trash-`, so what a test trashes
/// can be found in the test trash by the folder's name; and its canonical
/// path.
fn project(paths: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("fe-workbench-trash-")
        .tempdir()
        .unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for rel in paths {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, rel).unwrap();
    }
    (dir, root)
}

/// A name of the project's own, for the test trash: `<folder>-<name>`.
fn unique(root: &Path, name: &str) -> String {
    format!("{}-{name}", root.file_name().unwrap().to_string_lossy())
}

/// A workbench on `root`, in an active window, its project tree focused.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<ProjectPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("cmd-shift-e");
    cx.run_until_parked();
    let panel = panel(&workbench, cx);
    (workbench, panel, cx)
}

/// Clicks row `name` with `modifiers`, where the last frame laid it out.
fn click(
    panel: &Entity<ProjectPanel>,
    name: &str,
    modifiers: Modifiers,
    cx: &mut VisualTestContext,
) {
    let ix = (rows(panel, cx).iter())
        .position(|(_, row)| row == name)
        .unwrap_or_else(|| panic!("no row {name}"));
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
    cx.simulate_click(bounds.center(), modifiers);
    cx.run_until_parked();
}

fn shift() -> Modifiers {
    Modifiers::shift()
}

/// Cmd on macOS, ctrl elsewhere.
fn cmd() -> Modifiers {
    Modifiers::secondary_key()
}

/// The selected rows' names, in order, and the primary's.
fn selection(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> (Vec<String>, String) {
    let rows = rows(panel, cx);
    let (selected, primary) =
        panel.read_with(cx, |panel, _| (panel.selected_rows(), panel.selected_row()));
    let names = selected.iter().map(|&ix| rows[ix].1.clone()).collect();
    let primary = primary.map_or_else(String::new, |ix| rows[ix].1.clone());
    (names, primary)
}

/// The rows the panel drew selected, by name, once it's drawn again.
fn drawn(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let rows = rows(panel, cx);
    let drawn = panel.read_with(cx, |panel, _| panel.drawn_selection());
    drawn.iter().map(|&ix| rows[ix].1.clone()).collect()
}

fn announced(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    panel.read_with(cx, |panel, _| panel.announced())
}

/// Runs command `id` as a key bound in the panel would.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("project_panel"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// The toasts shown, each as its message and detail.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                (
                    toast.message.clone(),
                    toast.detail.clone().unwrap_or_default(),
                )
            })
            .collect()
    })
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[gpui::test]
fn shift_click_selects_a_range(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "b.txt", "c.txt"]);
    let (_, panel, cx) = open(cx, &root);
    assert_eq!(rows(&panel, cx).len(), 4);
    click(&panel, "a.txt", Modifiers::none(), cx);
    assert_eq!(selection(&panel, cx), (names(&["a.txt"]), "a.txt".into()));

    // From the anchor, the row clicked alone, to the row shift-clicked,
    // which is the primary; each drawn selected, the count said.
    click(&panel, "c.txt", shift(), cx);
    assert_eq!(
        selection(&panel, cx),
        (names(&["a.txt", "b.txt", "c.txt"]), "c.txt".into())
    );
    assert_eq!(drawn(&panel, cx), ["a.txt", "b.txt", "c.txt"]);
    assert_eq!(announced(&panel, cx).as_deref(), Some("3 items selected"));
    let primary = panel.read_with(cx, |panel, _| panel.drawn_selected());
    assert_eq!(primary, Some(3), "the primary is the row drawn active");

    // Again from the same anchor, upwards; a folder isn't expanded.
    click(&panel, "d", shift(), cx);
    assert_eq!(selection(&panel, cx), (names(&["d", "a.txt"]), "d".into()));
    assert_eq!(rows(&panel, cx).len(), 4, "d stays collapsed");
    assert_eq!(announced(&panel, cx).as_deref(), Some("2 items selected"));
}

#[gpui::test]
fn cmd_click_toggles(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "b.txt", "c.txt"]);
    let (_, panel, cx) = open(cx, &root);
    click(&panel, "a.txt", Modifiers::none(), cx);

    // One more: the primary and the anchor.
    click(&panel, "c.txt", cmd(), cx);
    assert_eq!(
        selection(&panel, cx),
        (names(&["a.txt", "c.txt"]), "c.txt".into())
    );
    assert_eq!(drawn(&panel, cx), ["a.txt", "c.txt"]);
    assert_eq!(announced(&panel, cx).as_deref(), Some("2 items selected"));
    // A folder too, which isn't expanded.
    click(&panel, "d", cmd(), cx);
    assert_eq!(
        selection(&panel, cx),
        (names(&["d", "a.txt", "c.txt"]), "d".into())
    );
    assert_eq!(rows(&panel, cx).len(), 4, "d stays collapsed");

    // One fewer: the primary moves to the nearest selected below it.
    click(&panel, "d", cmd(), cx);
    assert_eq!(
        selection(&panel, cx),
        (names(&["a.txt", "c.txt"]), "a.txt".into())
    );
    click(&panel, "c.txt", cmd(), cx);
    assert_eq!(selection(&panel, cx), (names(&["a.txt"]), "a.txt".into()));
    assert_eq!(announced(&panel, cx).as_deref(), Some("1 item selected"));
    // The last row selected, no longer: none is.
    click(&panel, "a.txt", cmd(), cx);
    assert_eq!(selection(&panel, cx), (Vec::new(), String::new()));
    assert_eq!(drawn(&panel, cx), Vec::<String>::new());
    assert_eq!(announced(&panel, cx).as_deref(), Some("0 items selected"));
    // From none, cmd-click selects its row alone, and shift-click too.
    click(&panel, "a.txt", cmd(), cx);
    assert_eq!(selection(&panel, cx), (names(&["a.txt"]), "a.txt".into()));
    click(&panel, "a.txt", cmd(), cx);
    click(&panel, "b.txt", shift(), cx);
    assert_eq!(selection(&panel, cx), (names(&["b.txt"]), "b.txt".into()));

    // Shift-click extends from the row cmd-clicked last.
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "c.txt", cmd(), cx);
    click(&panel, "d", shift(), cx);
    assert_eq!(
        selection(&panel, cx),
        (names(&["d", "a.txt", "b.txt", "c.txt"]), "d".into())
    );
}

#[gpui::test]
fn shift_arrows_extend(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "b.txt", "c.txt"]);
    let (_, panel, cx) = open(cx, &root);
    press("down down", cx);
    assert_eq!(selection(&panel, cx), (names(&["a.txt"]), "a.txt".into()));

    press("shift-down shift-down", cx);
    assert_eq!(
        selection(&panel, cx),
        (names(&["a.txt", "b.txt", "c.txt"]), "c.txt".into())
    );
    assert_eq!(drawn(&panel, cx), ["a.txt", "b.txt", "c.txt"]);
    assert_eq!(announced(&panel, cx).as_deref(), Some("3 items selected"));
    // Past the last row, nothing changes.
    press("shift-down", cx);
    assert_eq!(selection(&panel, cx).0.len(), 3);

    // Back towards the anchor, then past it.
    press("shift-up shift-up shift-up", cx);
    assert_eq!(selection(&panel, cx), (names(&["d", "a.txt"]), "d".into()));
    assert_eq!(announced(&panel, cx).as_deref(), Some("2 items selected"));

    // A plain arrow selects one again, from the primary.
    press("down", cx);
    assert_eq!(selection(&panel, cx), (names(&["a.txt"]), "a.txt".into()));
    assert_eq!(drawn(&panel, cx), ["a.txt"]);
}

#[gpui::test]
fn plain_click_resets(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "b.txt", "c.txt"]);
    let (_, panel, cx) = open(cx, &root);
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "c.txt", shift(), cx);
    assert_eq!(selection(&panel, cx).0.len(), 3);

    click(&panel, "b.txt", Modifiers::none(), cx);
    assert_eq!(selection(&panel, cx), (names(&["b.txt"]), "b.txt".into()));
    assert_eq!(drawn(&panel, cx), ["b.txt"]);
    // It's the anchor now.
    click(&panel, "c.txt", shift(), cx);
    assert_eq!(
        selection(&panel, cx),
        (names(&["b.txt", "c.txt"]), "c.txt".into())
    );

    // A plain click on a folder selects it alone, and toggles it.
    click(&panel, "d", Modifiers::none(), cx);
    assert_eq!(selection(&panel, cx), (names(&["d"]), "d".into()));
    assert!(rows(&panel, cx).contains(&(1, "e.txt".into())), "expanded");
}

#[gpui::test]
fn delete_asks_once_and_trashes_topmost_only(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[]);
    let (folder, file, kept) = (
        unique(&root, "d"),
        unique(&root, "a.txt"),
        unique(&root, "b.txt"),
    );
    for rel in [format!("{folder}/e.txt"), file.clone(), kept.clone()] {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    let (_, panel, cx) = open(cx, &root);
    // The folder, a file in it, and a file beside it.
    click(&panel, &folder, Modifiers::none(), cx);
    click(&panel, "e.txt", cmd(), cx);
    click(&panel, &file, cmd(), cx);
    assert_eq!(selection(&panel, cx).0, [&folder, "e.txt", &file]);

    // Asked once, of what goes: the folder takes its file with it.
    run("project_panel.delete", cx);
    let (question, _) = cx.pending_prompt().expect("asked");
    assert_eq!(question, "Move 2 items to the Trash?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());
    assert!(root.join(&folder).is_dir() && root.join(&file).is_file());

    run("project_panel.delete", cx);
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt(), "asked once");
    assert!(!root.join(&folder).exists());
    assert!(!root.join(&file).exists());
    assert_eq!(rows(&panel, cx), [(0, kept.clone())]);
    let trashed = fe_project::fs_ops::testing::trashed(&folder);
    assert_eq!(trashed.len(), 1, "{trashed:?}");
    assert!(
        trashed[0].join("e.txt").is_file(),
        "it went with its folder"
    );
    assert_eq!(fe_project::fs_ops::testing::trashed(&file).len(), 1);
    assert_eq!(selection(&panel, cx), (vec![kept.clone()], kept));
}

#[gpui::test]
fn cut_and_paste_moves_all(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "b.txt", "c.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "b.txt", shift(), cx);

    // Both cut, both dimmed.
    run("project_panel.cut", cx);
    let entries = panel.read_with(cx, |panel, _| panel.clipboard_entries());
    assert_eq!(entries, Some((true, vec!["a.txt".into(), "b.txt".into()])));
    panel.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let mut dimmed = panel.read_with(cx, |panel, _| panel.drawn_cut());
    dimmed.dedup();
    assert_eq!(dimmed, ["a.txt", "b.txt"]);

    // Pasted in the folder: both move, keeping their names and text, and
    // there's nothing left to paste.
    click(&panel, "d", Modifiers::none(), cx);
    run("project_panel.paste", cx);
    assert_eq!(fs::read_to_string(root.join("d/a.txt")).unwrap(), "a.txt");
    assert_eq!(fs::read_to_string(root.join("d/b.txt")).unwrap(), "b.txt");
    assert!(!root.join("a.txt").exists() && !root.join("b.txt").exists());
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.clipboard_entries()),
        None
    );
    assert!(rows(&panel, cx).contains(&(1, "a.txt".into())));
    assert_eq!(toasts(&workbench, cx), []);

    // Copied, then pasted twice beside them: each keeps its name, or takes
    // the ` copy` one, and the clipboard keeps them.
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "b.txt", shift(), cx);
    run("project_panel.copy", cx);
    run("project_panel.paste", cx);
    assert_eq!(
        fs::read_to_string(root.join("d/a copy.txt")).unwrap(),
        "a.txt"
    );
    assert_eq!(
        fs::read_to_string(root.join("d/b copy.txt")).unwrap(),
        "b.txt"
    );
    // Into the root, by c.txt's row: their own names.
    click(&panel, "c.txt", Modifiers::none(), cx);
    // The click opened c.txt in an editor, which took focus.
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    run("project_panel.paste", cx);
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "a.txt");
    assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "b.txt");
    let entries = panel.read_with(cx, |panel, _| panel.clipboard_entries());
    assert_eq!(
        entries,
        Some((false, vec!["d/a.txt".into(), "d/b.txt".into()]))
    );

    // A folder cut with a file is refused into itself, the file pasted.
    click(&panel, "c.txt", Modifiers::none(), cx);
    click(&panel, "d", cmd(), cx);
    run("project_panel.cut", cx);
    click(&panel, "e.txt", Modifiers::none(), cx);
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    run("project_panel.paste", cx);
    assert!(root.join("d/c.txt").is_file() && !root.join("c.txt").exists());
    assert_eq!(
        toasts(&workbench, cx),
        [(
            "“d” can’t be moved into itself or a folder inside it.".to_owned(),
            String::new()
        )]
    );
}

#[gpui::test]
fn copy_path_joins_lines(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "b.txt", "c.txt"]);
    let (_, panel, cx) = open(cx, &root);
    click(&panel, "d", Modifiers::none(), cx);
    click(&panel, "c.txt", cmd(), cx);
    click(&panel, "e.txt", cmd(), cx);

    // In the list's order, a line apiece.
    run("project_panel.copy_path", cx);
    let clipboard = cx.read_from_clipboard().and_then(|item| item.text());
    let absolute = |rel: &str| root.join(rel).to_string_lossy().into_owned();
    let lines = [absolute("d"), absolute("d/e.txt"), absolute("c.txt")];
    assert_eq!(clipboard, Some(lines.join("\n")));
    run("project_panel.copy_relative_path", cx);
    let clipboard = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(clipboard.as_deref(), Some("d\nd/e.txt\nc.txt"));

    // Duplicate copies each beside itself, the first copy selected.
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "e.txt", cmd(), cx);
    run("project_panel.duplicate", cx);
    assert_eq!(
        fs::read_to_string(root.join("a copy.txt")).unwrap(),
        "a.txt"
    );
    assert_eq!(
        fs::read_to_string(root.join("d/e copy.txt")).unwrap(),
        "d/e.txt"
    );
    assert_eq!(selection(&panel, cx).1, "e copy.txt");
}

#[gpui::test]
fn partial_failure_names_what_failed(cx: &mut TestAppContext) {
    let (_dir, root) = project(&[]);
    let [a, b, c] = ["a.txt", "b.txt", "c.txt"].map(|name| unique(&root, name));
    for name in [&a, &b, &c] {
        fs::write(root.join(name), "").unwrap();
    }
    let (workbench, panel, cx) = open(cx, &root);
    click(&panel, &a, Modifiers::none(), cx);
    click(&panel, &c, shift(), cx);
    run("project_panel.delete", cx);
    assert_eq!(
        cx.pending_prompt().map(|(question, _)| question).as_deref(),
        Some("Move 3 items to the Trash?")
    );
    // While it's asked, two are replaced: what's at their paths now isn't
    // what was asked about.
    for name in [&b, &c] {
        fs::rename(root.join(name), root.join(format!("{name}.old"))).unwrap();
        fs::write(root.join(name), "another\n").unwrap();
    }
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();

    assert!(!root.join(&a).exists(), "the rest went");
    assert_eq!(fs::read_to_string(root.join(&b)).unwrap(), "another\n");
    assert_eq!(fs::read_to_string(root.join(&c)).unwrap(), "another\n");
    let why = "It was moved or replaced since you were asked.";
    let detail = [&b, &c]
        .map(|name| format!("“{name}” couldn’t be moved to the Trash. {why}"))
        .join("\n");
    assert_eq!(
        toasts(&workbench, cx),
        [("2 items couldn’t be moved to the Trash.".to_owned(), detail)]
    );
    assert!(!cx.has_pending_prompt(), "a toast, not a dialog");

    // One alone is named in the toast's message.
    click(&panel, &b, Modifiers::none(), cx);
    click(&panel, &c, shift(), cx);
    run("project_panel.delete", cx);
    fs::remove_file(root.join(&c)).unwrap();
    fs::write(root.join(&c), "again\n").unwrap();
    cx.simulate_prompt_answer("Move to Trash");
    cx.run_until_parked();
    assert!(!root.join(&b).exists());
    let shown = toasts(&workbench, cx);
    assert_eq!(
        shown.last(),
        Some(&(
            format!("“{c}” couldn’t be moved to the Trash."),
            why.to_owned()
        ))
    );
}

#[gpui::test]
fn filter_clears_extra_selection(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt", "b.txt", "c.txt"]);
    let (_, panel, cx) = open(cx, &root);
    // Expanded, so the filter looks in it.
    click(&panel, "d", Modifiers::none(), cx);
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "c.txt", shift(), cx);
    assert_eq!(selection(&panel, cx).0.len(), 3);

    // Opening the filter's box selects the primary alone.
    run("project_panel.filter", cx);
    assert_eq!(selection(&panel, cx), (names(&["c.txt"]), "c.txt".into()));
    cx.simulate_input("txt");
    cx.run_until_parked();
    press("enter", cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.filter_text().map(str::to_owned)),
        Some("txt".into())
    );

    // In a filtered list, shift-click selects the rows it shows.
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "e.txt", shift(), cx);
    assert_eq!(
        selection(&panel, cx),
        (names(&["e.txt", "a.txt"]), "e.txt".into())
    );
    // Clearing the filter selects one again.
    run("project_panel.clear_filter", cx);
    assert_eq!(selection(&panel, cx).0.len(), 1);
}

/// T1750 F1: with a filter matching nothing, a right click on the tree's
/// empty space, under the line saying so, offers the empty space's menu.
#[gpui::test]
fn no_match_empty_space_opens_the_menu(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a.txt", "b.txt"]);
    let (_, panel, cx) = open(cx, &root);
    click(&panel, "a.txt", Modifiers::none(), cx);
    click(&panel, "b.txt", shift(), cx);
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    run("project_panel.filter", cx);
    cx.simulate_input("zzz");
    cx.run_until_parked();
    press("enter", cx);
    let message = panel.read_with(cx, |panel, _| panel.list_message());
    assert_eq!(message.as_deref(), Some("No matches in listed folders"));

    let tree = cx.debug_bounds("sidebar-panel").expect("drawn");
    let at = gpui::point(tree.center().x, tree.bottom() - gpui::px(10.));
    cx.simulate_mouse_down(at, gpui::MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, gpui::MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.menu_commands()),
        Some(vec![
            Some("project_panel.new_file"),
            Some("project_panel.new_folder"),
            None,
            Some("project_panel.refresh"),
        ])
    );
    assert_eq!(
        selection(&panel, cx).0,
        Vec::<String>::new(),
        "nothing is selected"
    );
}

#[gpui::test]
fn keys_that_move_select_one(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["d/e.txt", "a.txt"]);
    let (_, panel, cx) = open(cx, &root);
    // The folder expanded, its file selected, and the folder too.
    click(&panel, "d", Modifiers::none(), cx);
    click(&panel, "e.txt", Modifiers::none(), cx);
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    click(&panel, "d", cmd(), cx);
    assert_eq!(selection(&panel, cx), (names(&["d", "e.txt"]), "d".into()));

    // Left collapses the primary, selected alone; Right expands it, and
    // e.txt shows unselected.
    press("left", cx);
    assert_eq!(selection(&panel, cx), (names(&["d"]), "d".into()));
    press("right", cx);
    assert!(rows(&panel, cx).contains(&(1, "e.txt".into())), "expanded");
    assert_eq!(selection(&panel, cx), (names(&["d"]), "d".into()));
    assert_eq!(drawn(&panel, cx), ["d"]);

    // Enter toggles the primary's folder, selected alone.
    click(&panel, "a.txt", cmd(), cx);
    click(&panel, "d", cmd(), cx);
    click(&panel, "d", cmd(), cx);
    assert_eq!(selection(&panel, cx), (names(&["d", "a.txt"]), "d".into()));
    press("enter", cx);
    assert_eq!(selection(&panel, cx), (names(&["d"]), "d".into()));
}

#[gpui::test]
fn cut_entries_of_one_name_take_distinct_names(cx: &mut TestAppContext) {
    let (_dir, root) = project(&["a/x.txt", "b/x.txt", "c/X.TXT", "dst/x copy.txt"]);
    let (workbench, panel, cx) = open(cx, &root);
    for folder in ["a", "b", "c"] {
        click(&panel, folder, Modifiers::none(), cx);
    }
    click(&panel, "x.txt", Modifiers::none(), cx);
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    // Both x.txt rows and X.TXT, by clicking each.
    let rows_named = |panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext, name: &str| {
        (rows(panel, cx).iter().enumerate())
            .filter(|(_, (_, row))| row == name)
            .map(|(ix, _)| ix)
            .collect::<Vec<_>>()
    };
    let second = rows_named(&panel, cx, "x.txt")[1];
    let bounds = panel.read_with(cx, |panel, _| panel.row_bounds(second).expect("drawn"));
    cx.simulate_click(bounds.center(), cmd());
    click(&panel, "X.TXT", cmd(), cx);
    assert_eq!(selection(&panel, cx).0, ["x.txt", "x.txt", "X.TXT"]);
    run("project_panel.cut", cx);

    // Pasted in dst, holding `x copy.txt`: each takes its own name, none
    // replaced, names differing only in case kept apart.
    click(&panel, "dst", Modifiers::none(), cx);
    run("project_panel.paste", cx);
    let read = |rel: &str| fs::read_to_string(root.join(rel)).ok();
    assert_eq!(read("dst/x.txt").as_deref(), Some("a/x.txt"));
    assert_eq!(read("dst/x copy.txt").as_deref(), Some("dst/x copy.txt"));
    assert_eq!(read("dst/x copy 2.txt").as_deref(), Some("b/x.txt"));
    assert_eq!(read("dst/X copy 3.TXT").as_deref(), Some("c/X.TXT"));
    assert_eq!(toasts(&workbench, cx), []);
}

/// A rust-analyzer fake asked for its edits before a `*.rs` file's rename.
fn participant() -> fe_lsp::fake::Script {
    let mut result = fe_lsp::fake::initialize_result(None);
    let filters =
        serde_json::json!({"filters": [{"scheme": "file", "pattern": {"glob": "**/*.rs"}}]});
    result.capabilities.workspace = Some(
        serde_json::from_value(serde_json::json!({"fileOperations": {
            "didRename": filters, "willRename": filters,
        }}))
        .unwrap(),
    );
    fe_lsp::fake::Script {
        initialize: fe_lsp::fake::InitializeReply::Result(Box::new(result)),
        ..fe_lsp::fake::Script::default()
    }
}

/// Two modules cut together and pasted in a folder: the servers are asked
/// once, for both moves, and their answer, which edits a file open in an
/// editor and a closed one for both, lengths changing, is made whole, at
/// the files' new paths, once both have moved. The answer comes late, so
/// the paste waits for it.
#[gpui::test]
fn a_multi_cut_asks_once_and_makes_shared_edits(cx: &mut TestAppContext) {
    use crate::tests::lsp::Lsp;
    use fe_lsp::fake::{Record, Reply, Scripted};
    use fe_project::FsChange;
    use std::time::Duration;

    let mut s = Lsp::with_scripts(cx, participant(), fe_lsp::fake::Script::default());
    let edit = |line: u32, start: u32, end: u32, text: &str| {
        serde_json::json!({"range": {"start": {"line": line, "character": start},
            "end": {"line": line, "character": end}}, "newText": text})
    };
    let both = |prefix: u32| {
        serde_json::json!([
            edit(0, prefix, prefix + 4, "sub::util"),
            edit(1, prefix, prefix + 4, "sub::help")
        ])
    };
    let reply = Reply::Result(serde_json::json!({"changes": {
        s.uri("src/main.rs").as_str(): both(4),
        s.uri("src/lib.rs").as_str(): both(15),
    }}));
    let delay = Duration::from_millis(200);
    (s.fake).set_reply("workspace/willRenameFiles", Scripted { reply, delay });
    s.set_trust(true);
    let main = "use util;\nuse help;\nfn main() {}\n";
    fs::write(s.path("src/main.rs"), main).unwrap();
    fs::write(
        s.path("src/lib.rs"),
        "pub use crate::util;\npub use crate::help;\n",
    )
    .unwrap();
    fs::write(s.path("src/util.rs"), "pub fn u() {}\n").unwrap();
    fs::write(s.path("src/help.rs"), "pub fn h() {}\n").unwrap();
    fs::create_dir(s.path("src/sub")).unwrap();
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let changes: Vec<FsChange> = ["src/util.rs", "src/help.rs", "src/sub"]
        .map(|rel| FsChange::Path(s.path(rel)))
        .into();
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx)));
    s.open("src/main.rs");
    let uri = s.uri("src/main.rs");
    s.fake.wait_for_document(&uri, "didOpen", |r| r.is_some());
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();

    // util.rs and help.rs selected, cut, and pasted in sub.
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    s.cx.simulate_keystrokes("cmd-shift-e");
    let util = s.path("src/util.rs");
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(util, cx)));
    s.draw();
    let help = (rows(&panel, &mut s.cx).iter())
        .position(|(_, row)| row == "help.rs")
        .unwrap();
    let bounds = panel.read_with(&s.cx, |panel, _| panel.row_bounds(help).expect("drawn"));
    s.cx.simulate_click(bounds.center(), cmd());
    s.draw();
    assert_eq!(selection(&panel, &mut s.cx).0, ["help.rs", "util.rs"]);
    s.cx.simulate_keystrokes("cmd-x");
    let sub = s.path("src/sub");
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(sub, cx)));
    s.draw();
    s.cx.simulate_keystrokes("cmd-v");

    s.until("the moves", |s| {
        s.path("src/sub/util.rs").is_file() && s.path("src/sub/help.rs").is_file()
    });
    s.until("the edits", |s| {
        let text = editor.read_with(&s.cx, |view, _| view.model().text().to_string());
        text == "use sub::util;\nuse sub::help;\nfn main() {}\n"
    });
    s.until("the closed file's edits", |s| {
        fs::read_to_string(s.path("src/lib.rs")).unwrap()
            == "pub use crate::sub::util;\npub use crate::sub::help;\n"
    });
    // Asked once, of both.
    let asked: Vec<_> = (s.fake.records().into_iter())
        .filter_map(|record| match record {
            Record::Request { method, params, .. } if method == "workspace/willRenameFiles" => {
                Some(params["files"].as_array().map_or(0, Vec::len))
            }
            _ => None,
        })
        .collect();
    assert_eq!(asked, [2]);
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    let shown = layer.read_with(&s.cx, |layer, _| layer.queue().shown().len());
    assert_eq!(shown, 0);
}
