//! `vim.ignorecase`, `vim.smartcase` and `vim.incsearch` reaching the
//! registers every editor's vim shares.

use fe_command::Invocation;

use super::*;
use crate::{commands::RunCommand, set_vim_incsearch, set_vim_search_case};

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.model().primary_selection().head())
}

/// Searches `/hello` from the buffer's start and returns where the caret
/// lands, back at the start afterwards.
fn search_hello(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    cx.simulate_keystrokes("g g /");
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": "hello" }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(submit)), cx));
    cx.run_until_parked();
    let at = caret(view, cx);
    cx.simulate_keystrokes("g g");
    at
}

#[gpui::test]
fn a_live_change_reaches_open_editors(cx: &mut TestAppContext) {
    cx.update(init);
    let (view, cx) = open(cx, "x Hello\nhello");
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();

    // Case matters by default: the lowercase one on the second line.
    assert_eq!(search_hello(&view, cx), 8);
    cx.update(|_, cx| set_vim_search_case(cx, true, false));
    assert_eq!(search_hello(&view, cx), 2, "finds Hello");
    cx.update(|_, cx| set_vim_search_case(cx, false, false));
    assert_eq!(search_hello(&view, cx), 8);

    cx.update(|_, cx| {
        assert!(vim_registers(cx).unwrap().incsearch());
        set_vim_incsearch(cx, false);
        assert!(!vim_registers(cx).unwrap().incsearch());
        set_vim_incsearch(cx, true);
        assert!(vim_registers(cx).unwrap().incsearch());
    });
}
