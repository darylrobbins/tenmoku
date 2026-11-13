//! The project panel's New menu: the `+` leftmost in the dock's header,
//! the menu it opens under itself (File, Folder, then typed files, those
//! the root's project markers detect first), a typed entry's name field
//! and template, the folder it names the entry in, and Escape.

use std::{fs, path::Path};

use gpui::{Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext};

use super::{open_root, panel, rows};
use crate::{
    ProjectPanel, Workbench,
    project_panel::{
        NEW_BUTTON,
        new_menu::NewItem,
        pane_actions,
        templates::{self, FileType},
    },
};

/// Files at `paths` (relative) with their text, in a temporary directory.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// Every entry under `root`, relative, sorted; folders end in `/`.
fn snapshot(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = fe_project::relative_path(root, &path);
            if path.is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &path, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on `files`, in an active window, drawn, and its panel.
fn workbench<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (
    Entity<Workbench>,
    Entity<ProjectPanel>,
    &'a mut VisualTestContext,
) {
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    draw(cx);
    let panel = panel(&workbench, cx);
    (workbench, panel, cx)
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    (cx.debug_bounds(selector)).unwrap_or_else(|| panic!("{selector} drawn"))
}

fn button(id: &str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    bounds(format!("icon-button:{id}").leak(), cx)
}

/// Clicks the `+`.
fn click_plus(cx: &mut VisualTestContext) {
    let plus = button(NEW_BUTTON, cx);
    cx.simulate_click(plus.center(), Modifiers::none());
    draw(cx);
}

/// The open New menu's entries.
fn items(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<Vec<Option<NewItem>>> {
    panel.read_with(cx, |panel, _| panel.new_menu_items())
}

/// Clicks the open menu's item titled `title`, as drawn.
fn choose(title: &str, cx: &mut VisualTestContext) {
    let selector: &'static str = Box::leak(format!("menu-item:{title}").into_boxed_str());
    let item = bounds(selector, cx);
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
}

/// The name field's text and its selection, while it's open.
fn field(
    panel: &Entity<ProjectPanel>,
    cx: &mut VisualTestContext,
) -> Option<(String, std::ops::Range<usize>)> {
    let input = panel.read_with(cx, |panel, _| panel.name_input())?;
    Some(input.read_with(cx, |input, _| {
        (input.value().to_owned(), input.selected_range())
    }))
}

fn active_path(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<std::path::PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx)?;
        editor.read(cx).model().path().map(Path::to_owned)
    })
}

/// The typed entries of `items`.
fn typed(items: &[Option<NewItem>]) -> Vec<FileType> {
    (items.iter())
        .filter_map(|item| match item {
            Some(NewItem::Typed(file_type)) => Some(*file_type),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn plus_is_the_first_pane_action(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "")]);
    let (workbench, panel, cx) = workbench(cx, &dir);
    let actions = pane_actions(&panel);
    let first = &actions[0];
    assert_eq!(
        (first.id, first.icon, first.title.as_ref()),
        (NEW_BUTTON, fe_ui::Icon::Plus, "New…")
    );
    assert!(first.anchored.is_some(), "it opens a menu under itself");
    let ids: Vec<_> = actions.iter().map(|action| action.id).collect();
    assert_eq!(ids, [NEW_BUTTON, "project-reveal", "project-collapse"]);

    // Drawn leftmost of the header's buttons.
    let controls = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).state().drawn().controls()
    });
    assert_eq!(
        controls.first().map(|(id, _)| id.as_str()),
        Some(NEW_BUTTON)
    );
    let plus = button(NEW_BUTTON, cx);
    for (id, _) in &controls[1..] {
        assert!(plus.right() <= button(id, cx).left(), "{id}");
    }

    // A click opens the menu under it, the panel focused for its keys; a
    // second closes it.
    click_plus(cx);
    assert!(items(&panel, cx).is_some());
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_anchor()),
        Some(plus)
    );
    let menu = bounds("project-new-menu", cx);
    assert!(menu.top() >= plus.bottom(), "{menu:?} under {plus:?}");
    assert!(
        (menu.left() - plus.left()).abs() <= gpui::px(4.),
        "{menu:?} {plus:?}"
    );
    assert!(cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
    click_plus(cx);
    assert_eq!(items(&panel, cx), None);
}

#[gpui::test]
fn menu_lists_file_folder_then_typed_files(cx: &mut TestAppContext) {
    // No project markers: Markdown, then the fallback set.
    let dir = project(&[("notes.txt", "")]);
    let (_workbench, panel, cx) = workbench(cx, &dir);
    click_plus(cx);
    use FileType::*;
    let typed_files = [Markdown, Html, Css, Json, Shell].map(|t| Some(NewItem::Typed(t)));
    let expected: Vec<_> = [Some(NewItem::File), Some(NewItem::Folder), None]
        .into_iter()
        .chain(typed_files)
        .collect();
    assert_eq!(items(&panel, cx), Some(expected));
    let titles: Vec<_> = (panel.read_with(cx, |panel, cx| panel.new_menu_drawn(cx)))
        .unwrap()
        .into_iter()
        .map(|(title, _)| title)
        .collect();
    assert_eq!(
        titles,
        [
            "File",
            "Folder",
            "Markdown File",
            "HTML File",
            "CSS File",
            "JSON File",
            "Shell Script"
        ]
    );
    // Each one drawn.
    for title in &titles {
        bounds(format!("menu-item:{title}").leak(), cx);
    }

    // Down from none highlights File; up wraps to the last.
    cx.simulate_keystrokes("down");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_highlighted()),
        Some(NewItem::File)
    );
    cx.simulate_keystrokes("up");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.new_menu_highlighted()),
        Some(NewItem::Typed(Shell))
    );

    // File opens the empty name field, as New File does; Folder too.
    choose("File", cx);
    assert_eq!(items(&panel, cx), None);
    assert_eq!(field(&panel, cx), Some((String::new(), 0..0)));
    cx.simulate_input("lib");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(dir.path().join("lib").is_file());
    click_plus(cx);
    choose("Folder", cx);
    cx.simulate_input("docs");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(dir.path().join("docs").is_dir());
}

#[gpui::test]
fn detected_languages_come_first(cx: &mut TestAppContext) {
    let dir = project(&[
        ("Cargo.toml", "[package]\n"),
        ("package.json", "{}\n"),
        ("pyproject.toml", ""),
    ]);
    let (_workbench, panel, cx) = workbench(cx, &dir);
    click_plus(cx);
    let items = items(&panel, cx).expect("open");
    assert_eq!(
        items[..3],
        [Some(NewItem::File), Some(NewItem::Folder), None]
    );
    use FileType::*;
    assert_eq!(
        typed(&items),
        [
            Rust, TypeScript, JavaScript, Python, Markdown, Html, Css, Json, Shell
        ]
    );

    // Every marker: six types at most, each once, and the rest after.
    let all = templates::typed_entries(|_| true);
    assert_eq!(
        all,
        [
            Rust, TypeScript, JavaScript, Python, Go, Java, Markdown, Html, Css, Json, Shell
        ]
    );
    let gradle = templates::typed_entries(|name| name == "build.gradle" || name == "go.mod");
    assert_eq!(gradle[..2], [Go, Java]);
}

#[gpui::test]
fn choosing_python_creates_a_py_file_with_the_template(cx: &mut TestAppContext) {
    let dir = project(&[("requirements.txt", "")]);
    let (workbench, panel, cx) = workbench(cx, &dir);
    click_plus(cx);
    assert_eq!(typed(&items(&panel, cx).unwrap())[0], FileType::Python);
    choose("Python File", cx);
    assert_eq!(
        field(&panel, cx).map(|(text, _)| text).as_deref(),
        Some("Untitled.py")
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    let made = dir.path().join("Untitled.py");
    let body = FileType::Python.template().body;
    assert_eq!(fs::read_to_string(&made).unwrap(), body);
    assert_eq!(field(&panel, cx), None);
    // It opens, as New File's does.
    let opened = active_path(&workbench, cx).expect("the new file opened");
    assert_eq!(
        fs::canonicalize(opened).unwrap(),
        fs::canonicalize(&made).unwrap()
    );

    // A type with a body starts with it.
    click_plus(cx);
    choose("Shell Script", cx);
    assert_eq!(
        field(&panel, cx).map(|(text, _)| text).as_deref(),
        Some("Untitled.sh")
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        fs::read_to_string(dir.path().join("Untitled.sh")).unwrap(),
        "#!/bin/sh\n"
    );
    let html = FileType::Html.template().body;
    assert!(html.starts_with("<!doctype html>\n"), "{html}");
}

#[gpui::test]
fn name_field_keeps_extension(cx: &mut TestAppContext) {
    let dir = project(&[("pyproject.toml", ""), ("Untitled.py", "x = 1\n")]);
    let (_workbench, panel, cx) = workbench(cx, &dir);
    click_plus(cx);
    choose("Python File", cx);
    // Untitled.py is taken: the next free name, its stem selected.
    let (text, selected) = field(&panel, cx).expect("the field is open");
    assert_eq!(text, "Untitled-2.py");
    assert_eq!(&text[selected], "Untitled-2");

    // Typing replaces the stem only.
    cx.simulate_input("main");
    assert_eq!(
        field(&panel, cx).map(|(text, _)| text).as_deref(),
        Some("main.py")
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(fs::read_to_string(dir.path().join("main.py")).unwrap(), "");
    assert_eq!(
        fs::read_to_string(dir.path().join("Untitled.py")).unwrap(),
        "x = 1\n"
    );

    // A name typed with another extension gets no template.
    click_plus(cx);
    choose("Shell Script", cx);
    let clear = fe_command::Invocation::of("text_input.clear")
        .unwrap()
        .context(
            fe_command::ContextStack::new()
                .with(fe_command::ContextFrame::new("workbench"))
                .with(fe_command::ContextFrame::new("project_panel"))
                .with(fe_command::ContextFrame::new("text_input")),
        );
    let clear = fe_gpui::commands::RunCommand(clear);
    cx.update(|window, cx| window.dispatch_action(Box::new(clear), cx));
    cx.simulate_input("run.txt");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(fs::read_to_string(dir.path().join("run.txt")).unwrap(), "");
}

#[gpui::test]
fn target_is_the_selected_folder(cx: &mut TestAppContext) {
    let dir = project(&[("Cargo.toml", ""), ("src/main.rs", "fn main() {}\n")]);
    let (_workbench, panel, cx) = workbench(cx, &dir);
    let root = fs::canonicalize(dir.path()).unwrap();

    // The selected folder.
    panel.update(cx, |panel, cx| panel.reveal(root.join("src"), cx));
    draw(cx);
    click_plus(cx);
    choose("Rust File", cx);
    let rows_now = rows(&panel, cx);
    assert!(
        rows_now.contains(&(1, "main.rs".into())),
        "src expanded: {rows_now:?}"
    );
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(
        root.join("src/Untitled.rs").is_file(),
        "{:?}",
        snapshot(&root)
    );

    // The selected file's folder.
    panel.update(cx, |panel, cx| panel.reveal(root.join("src/main.rs"), cx));
    draw(cx);
    click_plus(cx);
    choose("Markdown File", cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(
        root.join("src/Untitled.md").is_file(),
        "{:?}",
        snapshot(&root)
    );

    // Nothing selected, as a right click below the rows leaves it: the
    // root.
    let list = panel.read_with(cx, |panel, _| panel.list_bounds());
    let below = gpui::point(list.center().x, list.bottom() - gpui::px(10.));
    cx.simulate_mouse_down(below, gpui::MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(below, gpui::MouseButton::Right, Modifiers::none());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.selected_row()), None);
    click_plus(cx);
    choose("JSON File", cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        fs::read_to_string(root.join("Untitled.json")).unwrap(),
        "{}\n"
    );
}

#[gpui::test]
fn escape_closes_without_creating(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "a\n")]);
    let (workbench, panel, cx) = workbench(cx, &dir);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.open(dir.path().join("a.txt"), window, cx)
        })
    });
    draw(cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.expect("a.txt is open");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    let before = snapshot(dir.path());

    // Escape in the menu: closed, focus back in the editor, nothing made.
    click_plus(cx);
    assert!(items(&panel, cx).is_some());
    cx.simulate_keystrokes("down escape");
    draw(cx);
    assert_eq!(items(&panel, cx), None);
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    assert_eq!(snapshot(dir.path()), before);

    // Escape in a typed entry's field: nothing made either.
    click_plus(cx);
    choose("Markdown File", cx);
    assert!(field(&panel, cx).is_some());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(field(&panel, cx), None);
    assert_eq!(snapshot(dir.path()), before);

    // A click outside closes it too.
    click_plus(cx);
    let tree = panel.read_with(cx, |panel, _| panel.list_bounds());
    cx.simulate_click(tree.center(), Modifiers::none());
    draw(cx);
    assert_eq!(items(&panel, cx), None);
    assert_eq!(snapshot(dir.path()), before);
}
