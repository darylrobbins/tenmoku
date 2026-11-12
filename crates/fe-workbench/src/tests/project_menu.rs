//! The project tree's right-click menu: what a right click selects, what
//! each kind of row's menu and the empty space's offer, the items running
//! the panel's commands, and closing and choosing with the keys.

use std::{fs, path::Path};

use fe_gpui::set_vim_enabled;
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};

use super::{open_root, panel, rows};
use crate::{ProjectPanel, Workbench};

/// Files at `paths` (relative), empty, in a temporary directory.
fn project(paths: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for rel in paths {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
    dir
}

/// A workbench on a `src` folder (holding `main.rs`) and `a.txt`, in an
/// active window, vim on if `vim`, and its panel.
fn workbench(
    cx: &mut TestAppContext,
    vim: bool,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<ProjectPanel>,
    &mut VisualTestContext,
) {
    let dir = project(&["src/main.rs", "a.txt"]);
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| {
        window.activate_window();
        set_vim_enabled(cx, vim);
    });
    cx.run_until_parked();
    let panel = panel(&workbench, cx);
    assert_eq!(
        rows(&panel, cx),
        [(0, "src".to_owned()), (0, "a.txt".to_owned())]
    );
    (dir, workbench, panel, cx)
}

/// Where row `name` was laid out in the last frame: its centre.
fn row_center(
    panel: &Entity<ProjectPanel>,
    name: &str,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    let ix = (rows(panel, cx).iter())
        .position(|(_, row)| row == name)
        .unwrap_or_else(|| panic!("no row {name}"));
    panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn").center())
}

/// A point in the list below its last row.
fn empty_space(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Point<Pixels> {
    let list = panel.read_with(cx, |panel, _| panel.list_bounds());
    point(list.center().x, list.bottom() - px(10.))
}

/// A right click at `at`, then a run of the executor.
fn right_click_at(at: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
}

/// A right click on row `name`.
fn right_click(panel: &Entity<ProjectPanel>, name: &str, cx: &mut VisualTestContext) {
    let at = row_center(panel, name, cx);
    right_click_at(at, cx);
}

/// The open menu's entries by command, `None` for a separator.
fn menu(
    panel: &Entity<ProjectPanel>,
    cx: &mut VisualTestContext,
) -> Option<Vec<Option<&'static str>>> {
    panel.read_with(cx, |panel, _| panel.menu_commands())
}

/// Clicks the open menu's item titled `title`, as drawn.
fn choose(title: &str, cx: &mut VisualTestContext) {
    let selector: &'static str = Box::leak(format!("menu-item:{title}").into_boxed_str());
    let bounds = (cx.debug_bounds(selector)).unwrap_or_else(|| panic!("{title} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

/// The selected row's name.
fn selected(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    let ix = panel.read_with(cx, |panel, _| panel.selected_row())?;
    Some(rows(panel, cx)[ix].1.clone())
}

fn panel_focused(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| panel.focus_handle(cx).is_focused(window))
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard()?.text())
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// The number of tabs open in the workbench's active pane.
fn tab_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len())
}

const FOLDER_MENU: [Option<&str>; 14] = [
    Some("project_panel.new_file"),
    Some("project_panel.new_folder"),
    None,
    Some("project_panel.rename"),
    Some("project_panel.duplicate"),
    Some("project_panel.delete"),
    None,
    Some("project_panel.cut"),
    Some("project_panel.copy"),
    None,
    Some("project_panel.copy_path"),
    Some("project_panel.copy_relative_path"),
    None,
    Some("project_panel.collapse_all"),
];

#[gpui::test]
fn right_click_selects_row_and_opens_menu(cx: &mut TestAppContext) {
    let (dir, workbench, panel, cx) = workbench(cx, false);
    let tabs = tab_count(&workbench, cx);

    // A file: selected, the panel focused, and not opened.
    right_click(&panel, "a.txt", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("a.txt"));
    assert!(menu(&panel, cx).is_some(), "the menu opens");
    assert!(panel_focused(&panel, cx), "the tree takes focus");
    assert_eq!(tab_count(&workbench, cx), tabs, "nothing opens");
    assert!(
        cx.debug_bounds("project-tree-menu").is_some(),
        "the menu is drawn"
    );

    // A folder, the menu open on another row: selected, and not expanded.
    right_click(&panel, "src", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("src"));
    assert_eq!(menu(&panel, cx).as_deref(), Some(&FOLDER_MENU[..]));
    assert_eq!(rows(&panel, cx).len(), 2, "the folder stays collapsed");

    // While a name field is open, a right click opens nothing and commits
    // nothing: the field keeps its text and focus.
    press("escape", cx);
    panel.update_in(cx, |panel, window, cx| {
        panel.run_op(crate::project_panel::PanelOp::NewFile, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_input("typed.txt");
    let before = rows(&panel, cx);
    right_click(&panel, "a.txt", cx);
    right_click_at(empty_space(&panel, cx), cx);
    assert_eq!(menu(&panel, cx), None, "no menu while naming");
    let field = panel.read_with(cx, |panel, cx| panel.name_field(cx));
    assert_eq!(
        field,
        Some(("typed.txt".to_owned(), None)),
        "the field stays"
    );
    assert!(!panel_focused(&panel, cx), "the field keeps focus");
    assert_eq!(rows(&panel, cx), before);
    assert!(!dir.path().join("typed.txt").exists(), "nothing made");
}

#[gpui::test]
fn folder_menu_lists_new_and_delete(cx: &mut TestAppContext) {
    let (dir, _workbench, panel, cx) = workbench(cx, false);
    right_click(&panel, "src", cx);
    // Paste is left out with nothing cut or copied.
    assert_eq!(menu(&panel, cx).as_deref(), Some(&FOLDER_MENU[..]));
    let drawn = panel
        .read_with(cx, |panel, cx| panel.menu_drawn(cx))
        .unwrap();
    let titles: Vec<&str> = drawn.iter().map(|(title, _)| title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "New File…",
            "New Folder…",
            "Rename…",
            "Duplicate",
            "Move to Trash",
            "Cut",
            "Copy",
            "Copy Path",
            "Copy Relative Path",
            "Collapse All Folders",
        ],
        "the commands' titles"
    );
    let key = |title: &str| {
        (drawn.iter().find(|(drawn, _)| drawn == title)).and_then(|(_, key)| key.clone())
    };
    assert_eq!(
        key("Rename…").as_deref(),
        Some("F2"),
        "its key, right-aligned"
    );
    assert!(key("Cut").is_some(), "{drawn:?}");
    assert_eq!(key("New File…"), None, "no key runs it without vim");
    for title in titles {
        let selector: &'static str = Box::leak(format!("menu-item:{title}").into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some(), "{title} is drawn");
    }

    // With something copied, Paste goes into the folder.
    press("escape", cx);
    right_click(&panel, "a.txt", cx);
    choose("Copy", cx);
    right_click(&panel, "src", cx);
    let menu = menu(&panel, cx).unwrap();
    let at = menu
        .iter()
        .position(|item| *item == Some("project_panel.copy"))
        .unwrap();
    assert_eq!(menu[at + 1], Some("project_panel.paste"));
    choose("Paste", cx);
    assert!(
        dir.path().join("src/a.txt").is_file(),
        "pasted into the folder"
    );
}

#[gpui::test]
fn file_menu_has_no_new_items(cx: &mut TestAppContext) {
    let (dir, _workbench, panel, cx) = workbench(cx, false);
    right_click(&panel, "a.txt", cx);
    let menu_now = menu(&panel, cx).unwrap();
    assert_eq!(menu_now.as_slice(), &FOLDER_MENU[3..]);

    // Something copied: a file's Paste goes beside it.
    choose("Copy", cx);
    right_click(&panel, "a.txt", cx);
    let menu_now = menu(&panel, cx).unwrap();
    assert!(!menu_now.contains(&Some("project_panel.new_file")));
    assert!(!menu_now.contains(&Some("project_panel.new_folder")));
    assert!(menu_now.contains(&Some("project_panel.paste")));
    choose("Paste", cx);
    assert!(dir.path().join("a copy.txt").is_file(), "pasted beside it");

    // A cut that would land where it is: Paste is left out.
    right_click(&panel, "a.txt", cx);
    choose("Cut", cx);
    right_click(&panel, "a.txt", cx);
    let menu_now = menu(&panel, cx).unwrap();
    assert!(
        !menu_now.contains(&Some("project_panel.paste")),
        "{menu_now:?}"
    );
}

#[gpui::test]
fn empty_space_menu_offers_new_and_paste(cx: &mut TestAppContext) {
    let (dir, _workbench, panel, cx) = workbench(cx, false);
    right_click(&panel, "a.txt", cx);
    press("escape", cx);
    right_click_at(empty_space(&panel, cx), cx);
    assert_eq!(selected(&panel, cx), None, "nothing is selected");
    assert_eq!(
        menu(&panel, cx),
        Some(vec![
            Some("project_panel.new_file"),
            Some("project_panel.new_folder"),
            None,
            Some("project_panel.refresh"),
        ])
    );

    // Something copied: Paste, into the root.
    press("escape", cx);
    right_click(&panel, "src", cx);
    choose("Copy", cx);
    right_click_at(empty_space(&panel, cx), cx);
    assert_eq!(
        menu(&panel, cx),
        Some(vec![
            Some("project_panel.new_file"),
            Some("project_panel.new_folder"),
            None,
            Some("project_panel.paste"),
            None,
            Some("project_panel.refresh"),
        ])
    );
    choose("Paste", cx);
    assert!(
        dir.path().join("src copy/main.rs").is_file(),
        "pasted in the root"
    );

    // New Folder: in the root.
    right_click_at(empty_space(&panel, cx), cx);
    choose("New Folder…", cx);
    cx.simulate_input("made");
    press("enter", cx);
    assert!(dir.path().join("made").is_dir());
}

#[gpui::test]
fn menu_items_run_the_panel_commands(cx: &mut TestAppContext) {
    let (dir, _workbench, panel, cx) = workbench(cx, false);
    let root = dir.path();

    right_click(&panel, "a.txt", cx);
    choose("Copy Path", cx);
    assert_eq!(
        clipboard(cx),
        Some(root.join("a.txt").to_string_lossy().into_owned())
    );
    assert_eq!(menu(&panel, cx), None, "choosing closes it");

    right_click(&panel, "a.txt", cx);
    choose("Copy Relative Path", cx);
    assert_eq!(clipboard(cx).as_deref(), Some("a.txt"));

    right_click(&panel, "a.txt", cx);
    choose("Duplicate", cx);
    assert!(root.join("a copy.txt").is_file());

    // New File on a folder: named in it.
    right_click(&panel, "src", cx);
    choose("New File…", cx);
    cx.simulate_input("lib.rs");
    press("enter", cx);
    assert!(root.join("src/lib.rs").is_file());
    assert!(
        rows(&panel, cx).contains(&(1, "main.rs".to_owned())),
        "the folder expanded"
    );

    // Collapse All Folders.
    right_click(&panel, "main.rs", cx);
    choose("Collapse All Folders", cx);
    assert!(!rows(&panel, cx).iter().any(|(depth, _)| *depth > 0));

    // Rename: the field opens on the row, from its name.
    right_click(&panel, "a.txt", cx);
    choose("Rename…", cx);
    let field = panel.read_with(cx, |panel, cx| panel.name_field(cx));
    assert_eq!(field, Some(("a.txt".to_owned(), None)));
    press("escape", cx);

    // Cut, then Refresh from the empty space.
    right_click(&panel, "a.txt", cx);
    choose("Cut", cx);
    let cut = panel.read_with(cx, |panel, _| panel.clipboard_entry());
    assert_eq!(cut, Some((true, Path::new("a.txt").to_owned())));
    fs::write(root.join("late.txt"), "").unwrap();
    right_click_at(empty_space(&panel, cx), cx);
    choose("Refresh", cx);
    assert!(rows(&panel, cx).contains(&(0, "late.txt".to_owned())));
}

#[gpui::test]
fn escape_and_outside_click_close_it(cx: &mut TestAppContext) {
    // With vim on, Escape outside the menu would focus the editor.
    let (_dir, _workbench, panel, cx) = workbench(cx, true);
    right_click(&panel, "a.txt", cx);
    assert!(menu(&panel, cx).is_some());
    press("escape", cx);
    assert_eq!(menu(&panel, cx), None, "Escape closes it");
    assert!(panel_focused(&panel, cx), "and only that");
    assert_eq!(selected(&panel, cx).as_deref(), Some("a.txt"));
    assert!(cx.debug_bounds("project-tree-menu").is_none());

    // A tree key works again once it's closed.
    press("k", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("src"));

    // A left click outside it, on another row: closed, and the click
    // selects that row as ever.
    right_click(&panel, "src", cx);
    assert!(menu(&panel, cx).is_some());
    let at = row_center(&panel, "a.txt", cx);
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(menu(&panel, cx), None, "a click outside closes it");
    assert_eq!(selected(&panel, cx).as_deref(), Some("a.txt"));

    // Its row going closes it.
    right_click(&panel, "a.txt", cx);
    let path = panel.read_with(cx, |panel, _| {
        panel.tree().unwrap().root_path().join("a.txt")
    });
    fs::remove_file(&path).unwrap();
    panel.update(cx, |panel, cx| {
        panel.apply_fs_changes(&[fe_project::FsChange::Path(path)], cx)
    });
    cx.run_until_parked();
    assert_eq!(menu(&panel, cx), None, "its row went");
}

#[gpui::test]
fn arrows_and_enter_choose_an_item(cx: &mut TestAppContext) {
    let (dir, _workbench, panel, cx) = workbench(cx, false);
    right_click(&panel, "a.txt", cx);
    let highlighted =
        |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.menu_highlighted());
    assert_eq!(highlighted(cx), None);

    press("down", cx);
    assert_eq!(highlighted(cx), Some("project_panel.rename"));
    press("up", cx);
    assert_eq!(
        highlighted(cx),
        Some("project_panel.collapse_all"),
        "wrapping"
    );
    press("down", cx);
    assert_eq!(highlighted(cx), Some("project_panel.rename"));
    // Past the separators.
    for _ in 0..5 {
        press("down", cx);
    }
    assert_eq!(highlighted(cx), Some("project_panel.copy_path"));
    assert_eq!(
        selected(&panel, cx).as_deref(),
        Some("a.txt"),
        "the tree's selection stays"
    );

    press("enter", cx);
    assert_eq!(menu(&panel, cx), None);
    assert_eq!(
        clipboard(cx),
        Some(dir.path().join("a.txt").to_string_lossy().into_owned())
    );

    // Hovering an item highlights it too.
    right_click(&panel, "a.txt", cx);
    let bounds = cx.debug_bounds("menu-item:Duplicate").expect("drawn");
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(highlighted(cx), Some("project_panel.duplicate"));
}
