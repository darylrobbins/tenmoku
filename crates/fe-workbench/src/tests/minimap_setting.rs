//! `[editor.minimap]` in the workbench: file editors show the minimap by
//! default, a reloaded settings file hides, shows and narrows it in the
//! editors already open, and the commit message box has none.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::settings::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px, size};

use crate::{Launch, Workbench};

/// A workbench on `root` in a 1400 × 600 window, active, as the app's is.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.simulate_resize(size(px(1400.), px(600.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .rfind(|editor| editor.read(cx).model().path() == Some(path))
            .expect("open")
    })
}

/// The width the editor's last frame drew its minimap, if it did.
fn minimap_width(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Pixels> {
    editor.read_with(cx, |editor, _| {
        editor.minimap_bounds().map(|bounds| bounds.size.width)
    })
}

/// Writes `toml` as the user's settings file `user` and applies the reload,
/// as the host does when the file changes.
fn reload(
    store: &mut SettingsStore,
    user: &Path,
    toml: &str,
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) {
    fs::write(user, toml).unwrap();
    let event = store.reload(&[user.to_owned()]).expect("changed");
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
}

#[gpui::test]
fn file_editors_show_the_minimap_from_the_settings_applied_live(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.rs"), dir.path().join("b.rs"));
    fs::write(&a, "fn main() {}\n").unwrap();
    fs::write(&b, "fn other() {}\n").unwrap();
    let user = dir.path().join("settings.toml");
    let mut store = SettingsStore::load(Some(user.clone()), None);
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &a, cx);
    assert_eq!(minimap_width(&editor, cx), Some(px(120.)), "on by default");

    reload(
        &mut store,
        &user,
        "[editor.minimap]\nenabled = false\n",
        &workbench,
        cx,
    );
    assert_eq!(
        minimap_width(&editor, cx),
        None,
        "hidden in the open editor"
    );
    // And in one opened since.
    let other = open_file(&workbench, &b, cx);
    assert_eq!(minimap_width(&other, cx), None);

    reload(
        &mut store,
        &user,
        "[editor.minimap]\nenabled = true\n",
        &workbench,
        cx,
    );
    assert_eq!(minimap_width(&other, cx), Some(px(120.)), "back");
    reload(
        &mut store,
        &user,
        "[editor.minimap]\nmax_column = 40\n",
        &workbench,
        cx,
    );
    assert_eq!(minimap_width(&other, cx), Some(px(40.)), "narrowed");
}

#[gpui::test]
fn the_commit_message_box_has_no_minimap(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let init = Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init", "-q"])
        .current_dir(dir.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .expect("run git");
    assert!(init.success());
    let (workbench, cx) = open(cx, dir.path());
    let run = RunCommand(
        Invocation::of("git.show_changes")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    });
    let commit_box = panel.read_with(cx, |panel, _| panel.commit_box().cloned());
    let commit_box = commit_box.expect("the panel's drawn with its box");
    assert_eq!(commit_box.read_with(cx, |editor, _| editor.minimap()), None);
    assert_eq!(minimap_width(&commit_box, cx), None);
}

/// Every editor the workbench shows, and whether its last frame drew the
/// minimap.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(gpui::EntityId, bool)> {
    let editors = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx));
    (editors.iter())
        .map(|editor| (editor.entity_id(), minimap_width(editor, cx).is_some()))
        .collect()
}

#[gpui::test]
fn split_and_restored_editors_show_the_minimap(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.rs");
    fs::write(&a, "fn main() {}\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &a, cx);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let split = shown(&workbench, cx);
    assert_eq!(split.len(), 2, "split");
    assert!(split.iter().all(|&(_, minimap)| minimap), "{split:?}");

    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.restore_layout(&entry, window, cx)
        })
    });
    draw(cx);
    let restored = shown(&workbench, cx);
    assert!(
        restored
            .iter()
            .any(|(id, _)| !split.iter().any(|(old, _)| old == id)),
        "restored afresh: {restored:?} after {split:?}"
    );
    assert!(restored.iter().all(|&(_, minimap)| minimap), "{restored:?}");
}
