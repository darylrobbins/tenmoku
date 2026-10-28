use super::*;
use crate::EditorTheme;
use fe_buffer::Buffer;
use fe_editor::{EditorModel, commands::HeadlessHost};
use gpui::TestAppContext;

const RUST: &str = "\
fn first() {}

impl S {
    fn a(&self) {
        fn inner() {}
    }

    fn b(&self) {}
}

trait T {
    fn sig(&self);
}

fn last() {
    let x = |y| y;
}
";

#[gpui::test]
fn real_rust_view_lists_functions(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(RUST), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        let syntax = synced_syntax(&view.syntax, &view.model).expect("a parsed tree");
        let host = ViewHost {
            cx,
            page_rows: 20,
            syntax: Some(syntax),
            server_folds: None,
            language: loaded_language(&view.syntax),
            vim: false,
        };
        let found = host.function_bounds();
        assert_eq!(found, fe_syntax::function_bounds(syntax));
        let texts: Vec<_> = found.iter().map(|range| &RUST[range.clone()]).collect();
        // Functions and methods by start, nested ones after the one around
        // them; a closure isn't one.
        assert_eq!(
            texts,
            [
                "fn first() {}",
                "fn a(&self) {\n        fn inner() {}\n    }",
                "fn inner() {}",
                "fn b(&self) {}",
                "fn sig(&self);",
                "fn last() {\n    let x = |y| y;\n}",
            ]
        );
    });
    // The trait's default has none.
    assert!(HeadlessHost::new().function_bounds().is_empty());
    // A file with more lists the outline's first ones.
    capped_as_the_outline_is();
}

fn capped_as_the_outline_is() {
    let mut text = String::new();
    for ix in 0..fe_syntax::MAX_OUTLINE_ITEMS + 5 {
        text.push_str(&format!("fn f{ix}() {{}}\n"));
    }
    let buffer = Buffer::new(&text);
    let mut syntax = Syntax::new(
        fe_syntax::language_by_name("rust").unwrap(),
        buffer.current(),
    );
    assert!(syntax.parse_until(std::time::Duration::MAX, || false));
    let found = fe_syntax::function_bounds(&syntax);
    assert_eq!(found.len(), fe_syntax::MAX_OUTLINE_ITEMS);
    assert_eq!(&text[found[0].clone()], "fn f0() {}");
    let last = found.last().unwrap();
    assert_eq!(
        &text[last.clone()],
        format!("fn f{}() {{}}", fe_syntax::MAX_OUTLINE_ITEMS - 1)
    );
}
