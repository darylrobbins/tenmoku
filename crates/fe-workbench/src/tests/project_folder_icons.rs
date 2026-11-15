use std::{fs, path::Path, sync::Arc};

use fe_project::FsChange;
use fe_ui::{Icon, IconRef, TintedIcons};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::{click_row, open_root, panel};
use crate::ProjectPanel;

/// The glyph the last frame drew folder row `name` with.
fn glyph(panel: &Entity<ProjectPanel>, name: &str, cx: &mut VisualTestContext) -> Icon {
    let drawn = panel.read_with(cx, |panel, _| panel.drawn_folders());
    let icon = (drawn.iter())
        .find(|(row, _)| row == name)
        .map(|(_, icon)| icon.clone())
        .unwrap_or_else(|| panic!("no folder row {name} in {drawn:?}"));
    match icon {
        IconRef::Tinted(icon, _) => icon,
        other => panic!("{name}: {other:?}"),
    }
}

/// Hands the panel a watcher batch naming `paths` under `root`, and runs
/// until it's drawn again and its probes have landed.
fn changed(panel: &Entity<ProjectPanel>, root: &Path, paths: &[&str], cx: &mut VisualTestContext) {
    let changes: Vec<_> = (paths.iter())
        .map(|path| FsChange::Path(root.join(path)))
        .collect();
    panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
    cx.run_until_parked();
}

#[gpui::test]
fn build_file_changes_redraw_folder_icons(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    for rel in ["core/src/main/java", "ws/crates", "app/src"] {
        fs::create_dir_all(root.join(rel)).unwrap();
    }
    fs::write(root.join("ws/Cargo.toml"), "[package]\nname = \"ws\"\n").unwrap();
    fs::write(root.join("app/build.gradle"), "").unwrap();
    let (workbench, cx) = open_root(cx, &root);
    cx.update(|_, cx| fe_ui::set_icon_set(Arc::new(TintedIcons), cx));
    cx.run_until_parked();
    let panel = panel(&workbench, cx);

    // Collapsed folders take their roles from their probes, off the frame.
    assert_eq!(glyph(&panel, "core", cx), Icon::Folder);
    assert_eq!(glyph(&panel, "ws", cx), Icon::FolderRoot);
    assert_eq!(glyph(&panel, "app", cx), Icon::FolderRoot);

    // A build file made in a collapsed folder: the tree's rows don't change,
    // but the folder is drawn again, a module.
    fs::write(root.join("core/build.gradle"), "").unwrap();
    changed(&panel, &root, &["core/build.gradle"], cx);
    assert_eq!(glyph(&panel, "core", cx), Icon::FolderRoot);
    // Expanded, the module's `src` holds `main`: their container, a plain
    // folder.
    click_row(&panel, "core", cx);
    cx.run_until_parked();
    assert_eq!(glyph(&panel, "src", cx), Icon::Folder);
    // Removed, it's a plain folder again.
    fs::remove_file(root.join("core/build.gradle")).unwrap();
    changed(&panel, &root, &["core/build.gradle"], cx);
    assert_eq!(glyph(&panel, "core", cx), Icon::FolderOpen);
    assert_eq!(glyph(&panel, "src", cx), Icon::FolderCode);

    // A `Cargo.toml` made a workspace by its text alone: a solution, in the
    // folder's tint.
    let ws = |cx: &mut VisualTestContext| {
        let drawn = panel.read_with(cx, |panel, _| panel.drawn_folders());
        drawn.into_iter().find(|(row, _)| row == "ws").unwrap().1
    };
    let crate_ = ws(cx);
    fs::write(root.join("ws/Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    changed(&panel, &root, &["ws/Cargo.toml"], cx);
    let workspace = ws(cx);
    assert_eq!(
        (&crate_, &workspace),
        (
            &IconRef::Tinted(Icon::FolderRoot, "file_icon.folder_source"),
            &IconRef::Tinted(Icon::FolderRoot, "file_icon.folder")
        )
    );

    cx.update(|_, cx| fe_ui::reset_icon_set(cx));
}
