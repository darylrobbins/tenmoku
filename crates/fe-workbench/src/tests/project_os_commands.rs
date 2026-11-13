//! The project tree's Reveal in Finder, Open in Terminal and Find in Folder
//! on the selected row: what each hands to its seam, a file's folder being
//! its parent, and the three items of the row's menu.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{App, Entity, TestAppContext, VisualTestContext};

use super::{open_root, panel};
use crate::{ProjectPanel, Workbench, path_commands::RevealSeam};

static REVEALED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn record(path: &Path, _: &mut App) {
    REVEALED.lock().unwrap().push(path.to_owned());
}

/// A workbench on `src/main.rs` and `a.txt`, revealing into [`REVEALED`] and
/// starting no terminal process, and its panel.
fn workbench(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<ProjectPanel>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "").unwrap();
    fs::write(dir.path().join("a.txt"), "").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| {
        window.activate_window();
        cx.set_global(RevealSeam(record));
    });
    workbench
        .read_with(cx, |workbench, _| workbench.terminal().clone())
        .update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.run_until_parked();
    REVEALED.lock().unwrap().clear();
    let panel = panel(&workbench, cx);
    (dir, workbench, panel, cx)
}

/// Selects row `name` with a right click, as its menu opens, and closes the
/// menu: the panel keeps focus, as a click on a file wouldn't leave it.
fn select(panel: &Entity<ProjectPanel>, name: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let ix = (super::rows(panel, cx).iter())
        .position(|(_, row)| row == name)
        .unwrap_or_else(|| panic!("no row {name}"));
    let at = panel.read_with(cx, |panel, _| panel.row_bounds(ix).unwrap().center());
    cx.simulate_mouse_down(at, gpui::MouseButton::Right, gpui::Modifiers::none());
    cx.simulate_mouse_up(at, gpui::MouseButton::Right, gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(crate::project_panel::KEY_CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
}

/// The folders the terminal's sessions were started in.
fn started_in(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.terminal().read(cx).started_options.iter())
            .map(|options| options.cwd.clone())
            .collect()
    })
}

#[gpui::test]
fn reveal_records_the_selected_path_through_the_seam(cx: &mut TestAppContext) {
    let (dir, _, panel, cx) = workbench(cx);
    select(&panel, "src", cx);
    run("project_panel.reveal_in_os", cx);
    assert_eq!(*REVEALED.lock().unwrap(), [dir.path().join("src")]);
}

#[gpui::test]
fn file_row_reveals_its_file(cx: &mut TestAppContext) {
    let (dir, _, panel, cx) = workbench(cx);
    select(&panel, "a.txt", cx);
    run("project_panel.reveal_in_os", cx);
    assert_eq!(*REVEALED.lock().unwrap(), [dir.path().join("a.txt")]);
}

#[gpui::test]
fn open_in_terminal_uses_the_folder_or_parent(cx: &mut TestAppContext) {
    let (dir, workbench, panel, cx) = workbench(cx);
    let root = dir.path().to_owned();
    select(&panel, "src", cx);
    run("project_panel.open_in_terminal", cx);
    select(&panel, "a.txt", cx);
    run("project_panel.open_in_terminal", cx);
    let dirs = started_in(&workbench, cx);
    let canon = |path: &PathBuf| fs::canonicalize(path).unwrap();
    assert_eq!(
        dirs.iter().map(canon).collect::<Vec<_>>(),
        [canon(&root.join("src")), canon(&root)]
    );
}

#[gpui::test]
fn find_in_folder_scopes_to_the_folder(cx: &mut TestAppContext) {
    let (_dir, workbench, panel, cx) = workbench(cx);
    select(&panel, "src", cx);
    run("project_panel.find_in_folder", cx);
    let search = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    let (details, include) = search.read_with(cx, |search, cx| {
        (
            search.details_shown(),
            search.include_input().read(cx).text().to_owned(),
        )
    });
    assert!(details);
    assert_eq!(include, "src/**");
    let active = workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).active());
    assert_eq!(active, crate::DockPanel::Search);

    // A file's folder is its parent: the root, here, which scopes nothing.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.run_dock_op(crate::dock::DockOp::ShowProject, window, cx)
    });
    select(&panel, "a.txt", cx);
    run("project_panel.find_in_folder", cx);
    let include = search.read_with(cx, |search, cx| {
        search.include_input().read(cx).text().to_owned()
    });
    assert_eq!(include, "");
}

#[gpui::test]
fn menu_lists_the_three_items(cx: &mut TestAppContext) {
    let (_dir, _, panel, cx) = workbench(cx);
    for name in ["src", "a.txt"] {
        let ix = (super::rows(&panel, cx).iter())
            .position(|(_, row)| row == name)
            .unwrap();
        let at = panel.read_with(cx, |panel, _| panel.row_bounds(ix).unwrap().center());
        cx.simulate_mouse_down(at, gpui::MouseButton::Right, gpui::Modifiers::none());
        cx.simulate_mouse_up(at, gpui::MouseButton::Right, gpui::Modifiers::none());
        cx.run_until_parked();
        let menu = panel
            .read_with(cx, |panel, _| panel.menu_commands())
            .unwrap();
        let tail: Vec<_> = menu.iter().rev().skip(2).take(3).rev().collect();
        assert_eq!(
            tail,
            [
                &Some("project_panel.reveal_in_os"),
                &Some("project_panel.open_in_terminal"),
                &Some("project_panel.find_in_folder"),
            ],
            "{name}"
        );
        let titles = panel
            .read_with(cx, |panel, cx| panel.menu_drawn(cx))
            .unwrap();
        let titles: Vec<_> = titles.iter().map(|(title, _)| title.as_str()).collect();
        let reveal = if cfg!(target_os = "macos") {
            "Reveal in Finder"
        } else {
            "Reveal in File Manager"
        };
        assert!(titles.contains(&reveal), "{titles:?}");
        assert!(titles.contains(&"Open in Terminal") && titles.contains(&"Find in Folder…"));
        cx.simulate_keystrokes("escape");
    }
}
