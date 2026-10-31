//! A pane toolbar says `untitled` only for an editor buffer with no file,
//! not for the Settings or Keyboard Shortcuts tab.

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::EditorModel;
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{Pane, Workbench, open::ConfigDirOverride};

/// A workbench on an empty folder, focused, whose config directory is a
/// temporary one.
fn workbench<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(dir.path().join("config"))));
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    (workbench, cx)
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn crumbs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let pane: Entity<Pane> = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    pane.read_with(cx, |pane, _| {
        pane.drawn_crumbs()
            .iter()
            .map(|(label, ..)| label.clone())
            .collect()
    })
}

#[gpui::test]
fn settings_tab_has_no_untitled_crumb(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = workbench(cx, &dir);
    run("workbench.open_settings_ui", cx);
    assert_eq!(crumbs(&workbench, cx), ["Settings"]);
}

#[gpui::test]
fn keyboard_shortcuts_tab_has_none_either(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = workbench(cx, &dir);
    run("workbench.open_keyboard_shortcuts", cx);
    assert_eq!(crumbs(&workbench, cx), ["Keyboard Shortcuts"]);
}

#[gpui::test]
fn untitled_buffer_still_says_untitled(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = workbench(cx, &dir);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|_, cx| {
        pane.update(cx, |pane, cx| {
            pane.add(EditorModel::new(Buffer::new(""), None), cx)
        })
    });
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    assert_eq!(crumbs(&workbench, cx), ["untitled"]);
}
