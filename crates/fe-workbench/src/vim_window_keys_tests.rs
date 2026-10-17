//! Vim's `<C-w>` window keys, pressed in a vim-mode editor, run the pane
//! commands and leave vim's mode and pending keys alone.

use fe_buffer::Buffer;
use fe_gpui::{EditorSettings, EditorTheme, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_vim_enabled(cx, true);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("alpha\n").into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// Where the active editor is among the panes.
fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<usize> {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx)?;
        (workbench.pane_group(cx).panes().iter()).position(|pane| {
            let pane = pane.read(cx);
            (pane.active()).is_some_and(|ix| pane.tabs()[ix].editor() == Some(editor))
        })
    })
}

fn pane_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| workbench.pane_group(cx).panes().len())
}

fn widths(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<f32> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .map(|pane| f32::from(pane.read(cx).bounds().size.width))
            .collect()
    })
}

/// Vim's mode and pending keys, for comparing. Its message isn't: any key
/// ends it.
fn vim_state(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx).unwrap();
        let state = editor.read(cx).vim_state().unwrap();
        format!("{:?} {:?}", state.mode(), state.pending())
    })
}

#[gpui::test]
fn ctrl_w_keys_run_the_pane_commands_and_leave_vim_alone(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    let vim = vim_state(&workbench, cx);

    press("ctrl-w v", cx);
    assert_eq!(pane_count(&workbench, cx), 2);
    assert_eq!(active(&workbench, cx), Some(1));
    assert_eq!(vim_state(&workbench, cx), vim);

    press("ctrl-w h", cx);
    assert_eq!(active(&workbench, cx), Some(0));
    assert_eq!(vim_state(&workbench, cx), vim);

    let before = widths(&workbench, cx);
    press("ctrl-w >", cx);
    let after = widths(&workbench, cx);
    assert!(
        after[0] > before[0] && after[1] < before[1],
        "{before:?} -> {after:?}"
    );
    assert_eq!(vim_state(&workbench, cx), vim);

    press("ctrl-w =", cx);
    let evened = widths(&workbench, cx);
    assert!((evened[0] - evened[1]).abs() <= 1., "{evened:?}");
    assert_eq!(vim_state(&workbench, cx), vim);

    // The left pane's only tab moves into the right pane, closing the left.
    press("ctrl-w shift-l", cx);
    assert_eq!(pane_count(&workbench, cx), 1);
    assert_eq!(vim_state(&workbench, cx), vim);

    press("ctrl-w v", cx);
    assert_eq!(pane_count(&workbench, cx), 2);
    press("ctrl-w c", cx);
    assert_eq!(pane_count(&workbench, cx), 1);
    assert_eq!(vim_state(&workbench, cx), vim);
}
