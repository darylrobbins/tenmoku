//! Function names in the bold face at their definition (T674): the outline's
//! name ranges, laid over the highlight spans, keep their colour and gain
//! the face the theme gives `function.definition`.

use fe_buffer::Buffer;
use fe_config::{EditorPalette, FontStyle};
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, SpanStyle, init};
use gpui::{TestAppContext, TestDispatcher, px, size};

fn bold_definitions() -> EditorPalette {
    let mut palette = EditorPalette::fastedit_dark().clone();
    let bold = FontStyle {
        italic: false,
        bold: true,
        strikethrough: false,
    };
    palette.styles = vec![("function.definition".into(), bold)];
    palette
}

/// A window showing `text` as the file `name`, with the tree parsed.
fn with_view<R>(
    test: &'static str,
    palette: &EditorPalette,
    name: &'static str,
    text: String,
    check: impl FnOnce(&EditorView, &str) -> R,
) -> R {
    let mut cx = TestAppContext::build(TestDispatcher::new(0), Some(test));
    cx.update(init);
    let theme = cx.update(|cx| EditorTheme::from_palette(palette, &EditorSettings::default(), cx));
    let shown = text.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&shown), Some(name.into()));
        EditorView::new(model, theme, cx)
    });
    cx.simulate_resize(size(px(600.), px(300.)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| check(view, &text))
}

/// The style of the span covering `word`'s first byte, if any does.
fn style_of(view: &EditorView, text: &str, word: &str, nth: usize) -> Option<SpanStyle> {
    let at = text.match_indices(word).nth(nth).unwrap().0;
    (view.styled_spans(0..text.len()).into_iter())
        .find(|(range, _)| range.contains(&at))
        .map(|(_, style)| style)
}

#[test]
fn function_names_are_bold_over_their_colour() {
    let text = "struct S;\nimpl S {\n    fn method(&self) {}\n}\nfn main() {}\n";
    with_view(
        "bold",
        &bold_definitions(),
        "main.rs",
        text.into(),
        |view, text| {
            let main = style_of(view, text, "main", 0).expect("main is styled");
            let method = style_of(view, text, "method", 0).expect("method is styled");
            assert!(main.bold && method.bold);
            // The colour is the one the span had without the face.
            let fn_keyword = style_of(view, text, "fn", 0).expect("fn is coloured");
            assert!(!fn_keyword.bold);
            let whole = view.styled_spans(0..text.len());
            let at = text.find("main").unwrap();
            let covering: Vec<_> = whole
                .iter()
                .filter(|(r, _)| r.start <= at && at < r.end)
                .collect();
            assert_eq!(covering.len(), 1, "{whole:?}");
            assert_eq!(covering[0].0, at..at + 4);
        },
    );
    // Without the bold, the colour is the same.
    let mut plain = EditorPalette::fastedit_dark().clone();
    plain.styles.clear();
    let bold = with_view(
        "bold_colour",
        &bold_definitions(),
        "main.rs",
        text.into(),
        |v, t| style_of(v, t, "main", 0).unwrap().color,
    );
    let none = with_view("no_bold", &plain, "main.rs", text.into(), |v, t| {
        style_of(v, t, "main", 0).map(|style| (style.color, style.bold))
    });
    assert_eq!(none, Some((bold, false)));
}

#[test]
fn call_sites_are_not_bold() {
    let text = "fn helper() {}\nfn main() {\n    helper();\n}\n";
    with_view(
        "calls",
        &bold_definitions(),
        "main.rs",
        text.into(),
        |view, text| {
            assert!(style_of(view, text, "helper", 0).unwrap().bold);
            let call = style_of(view, text, "helper", 1);
            assert!(call.is_none_or(|style| !style.bold));
        },
    );
}

#[test]
fn a_truncated_outline_styles_what_it_lists() {
    let count = fe_syntax::MAX_OUTLINE_ITEMS + 5;
    let text: String = (0..count).map(|i| format!("fn f{i}() {{}}\n")).collect();
    with_view(
        "truncated",
        &bold_definitions(),
        "main.rs",
        text,
        |view, text| {
            let first = style_of(view, text, "f0(", 0).unwrap();
            assert!(first.bold);
            let last_name = format!("f{}(", count - 1);
            assert!(style_of(view, text, &last_name, 0).unwrap().bold);
        },
    );
}

#[test]
fn a_language_without_an_outline_query_is_unchanged() {
    let text = "fn main() {}\n".to_string();
    let bold = with_view(
        "txt_bold",
        &bold_definitions(),
        "notes.txt",
        text.clone(),
        |v, t| v.styled_spans(0..t.len()),
    );
    let mut unstyled = EditorPalette::fastedit_dark().clone();
    unstyled.styles.clear();
    let plain = with_view("txt_plain", &unstyled, "notes.txt", text, |v, t| {
        v.styled_spans(0..t.len())
    });
    assert_eq!(bold, plain);
    assert!(bold.iter().all(|(_, style)| !style.bold));
}

#[test]
fn rows_outside_the_viewport_cost_nothing() {
    let text: String = (0..10_000).map(|i| format!("fn f{i}() {{}}\n")).collect();
    let len = text.len();
    let (count, range) = with_view("viewport", &bold_definitions(), "main.rs", text, |_, _| {
        EditorView::definition_lookups()
    });
    assert!(count > 0, "the outline was never asked");
    assert!(range.end < len / 10, "asked over {range:?} of {len} bytes");
}
