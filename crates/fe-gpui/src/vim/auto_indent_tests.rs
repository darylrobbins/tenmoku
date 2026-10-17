//! Vim indents by the view's language in a headless window, through the
//! app's own keys: `Enter` in insert mode, `o`, `O` and `cc` open or empty
//! rows at the language's indentation, and a `}` typed lines its row up
//! with its opener's.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, init};

/// A focused Rust editor showing `text` with a caret at byte `caret`, vim
/// on, once its tree is parsed.
fn rust_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    caret: usize,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(caret);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_language(Some("rust"), cx);
        view
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

/// `keys` on `text_at` (text and caret), then `input` as text input, then
/// `after`, each once the tree has caught up: the text they leave.
fn typed(
    text_at: (&str, usize),
    keys: &str,
    input: &str,
    after: &str,
    cx: &mut TestAppContext,
) -> String {
    let (view, cx) = rust_editor(cx, text_at.0, text_at.1);
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
    if !input.is_empty() {
        cx.simulate_input(input);
        cx.run_until_parked();
    }
    if !after.is_empty() {
        cx.simulate_keystrokes(after);
        cx.run_until_parked();
    }
    text(&view, cx)
}

#[gpui::test]
fn enter_in_insert_mode_indents_by_language(cx: &mut TestAppContext) {
    assert_eq!(
        typed(("fn main() {\n}", 0), "A enter", "x", "escape", cx),
        "fn main() {\n    x\n}"
    );
}

#[gpui::test]
fn enter_left_empty_loses_its_indentation(cx: &mut TestAppContext) {
    assert_eq!(
        typed(("fn main() {\n}", 0), "A enter escape", "", "", cx),
        "fn main() {\n\n}"
    );
}

#[gpui::test]
fn o_and_capital_o_open_rows_by_language(cx: &mut TestAppContext) {
    assert_eq!(
        typed(("fn main() {\n}", 0), "o", "x", "escape", cx),
        "fn main() {\n    x\n}"
    );
    assert_eq!(
        typed(("fn main() {\n}", 12), "O", "x", "escape", cx),
        "fn main() {\n    x\n}"
    );
}

#[gpui::test]
fn cc_reindents_an_over_indented_row(cx: &mut TestAppContext) {
    assert_eq!(
        typed(
            ("fn main() {\n          x;\n}", 22),
            "c c",
            "y;",
            "escape",
            cx
        ),
        "fn main() {\n    y;\n}"
    );
}

#[gpui::test]
fn a_typed_closer_lines_up_with_its_opener(cx: &mut TestAppContext) {
    let block = "fn main() {\n    if x {\n        y();\n    }\n}";
    // `o` below `y();` opens at its level; `}` typed there goes out to the
    // `if`'s.
    let caret = block.find("y();").unwrap();
    assert_eq!(
        typed((block, caret), "o", "}", "escape", cx),
        "fn main() {\n    if x {\n        y();\n    }\n    }\n}"
    );
}
