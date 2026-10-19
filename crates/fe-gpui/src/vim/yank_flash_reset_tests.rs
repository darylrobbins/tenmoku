//! A yank's flash after vim is turned off and on again, and after a
//! command that yanks and then edits.

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::{EditorModel, decorations::DecorationKind};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, commands::RunCommand, init};

fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn highlights(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Vec<std::ops::Range<usize>> {
    view.read_with(cx, |view, _| {
        let len = view.model().text().len();
        view.model()
            .decorations(0..len)
            .into_iter()
            .filter(|decoration| decoration.kind == DecorationKind::Highlight)
            .map(|decoration| decoration.range)
            .collect()
    })
}

#[gpui::test]
fn a_new_vim_state_flashes_its_first_yank(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two");
    cx.simulate_keystrokes("y i w");
    assert_eq!(highlights(&view, cx), vec![0..3]);
    // Turning vim off ends the flash.
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    assert_eq!(highlights(&view, cx), Vec::<std::ops::Range<usize>>::new());
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    // The new state's first yank has the serial the old one's had.
    cx.simulate_keystrokes("w y i w");
    assert_eq!(highlights(&view, cx), vec![4..7]);
}

#[gpui::test]
fn a_command_that_yanks_then_edits_flashes_nothing(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "one two");
    cx.simulate_keystrokes(":");
    let invocation = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": "normal yiwx" }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.model().text().to_string()),
        "ne two"
    );
    assert_eq!(highlights(&view, cx), Vec::<std::ops::Range<usize>>::new());
    // A plain yank after it still flashes.
    cx.simulate_keystrokes("y i w");
    assert_eq!(highlights(&view, cx), vec![0..2]);
}
