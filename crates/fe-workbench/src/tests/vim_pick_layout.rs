//! The `:marks` and `:registers` pickers share the `:ls` layout (T1337
//! keeps T1319's full-width rows for every vim picker): the popover's list
//! keeps its width (it never hugged the rows, so T1319 did not change it)
//! and each row spans it, so the selection highlight and click area match.

use gpui::{Focusable as _, TestAppContext, px};

use super::open_root;

/// The popover list's width in the test window, as before T1319 (it never
/// hugged its rows).
const LIST_WIDTH: f32 = 598.;

fn draw(cx: &mut gpui::VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Widths of the list and of row 0 after running `command` in vim.
fn widths(cx: &mut TestAppContext, setup: &str, command: &str) -> (gpui::Pixels, gpui::Pixels) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = dir.path().join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    draw(cx);
    cx.simulate_keystrokes("escape");
    if !setup.is_empty() {
        cx.simulate_keystrokes(setup);
    }
    cx.simulate_keystrokes(":");
    cx.simulate_input(command);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let picker = workbench
        .read_with(cx, |workbench, cx| workbench.vim_pick(cx))
        .expect("the picker shows");
    picker.read_with(cx, |picker, _| {
        let row = picker.row_bounds(0).expect("row 0 was painted");
        (picker.list_bounds().size.width, row.size.width)
    })
}

#[gpui::test]
fn marks_rows_span_the_list(cx: &mut TestAppContext) {
    let (list, row) = widths(cx, "m a", "marks");
    assert_eq!(list, px(LIST_WIDTH));
    assert_eq!(row, list);
}

#[gpui::test]
fn registers_rows_span_the_list(cx: &mut TestAppContext) {
    let (list, row) = widths(cx, "y y", "registers");
    assert_eq!(list, px(LIST_WIDTH));
    assert_eq!(row, list);
}
