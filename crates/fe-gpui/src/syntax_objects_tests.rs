use super::*;
use crate::EditorTheme;
use fe_buffer::Buffer;
use fe_editor::{EditorModel, commands::HeadlessHost};
use gpui::TestAppContext;

const RUST: &str = "impl S {\n    fn f(&self) {\n        g();\n    }\n}\n";

#[gpui::test]
fn host_maps_definition_objects(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(RUST), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    view.update(cx, |view, cx| view.set_language(Some("rust"), cx));
    cx.run_until_parked();
    let offset = RUST.find("g()").unwrap();
    view.update(cx, |view, cx| {
        let syntax = synced_syntax(&view.syntax, &view.model).expect("a parsed tree");
        let expected: Vec<_> = (fe_syntax::definition_objects(syntax, offset).into_iter())
            .map(|object| SyntaxObject {
                function: object.kind == fe_syntax::DefinitionKind::Function,
                outer: object.outer,
                inner: object.inner,
            })
            .collect();
        let host = ViewHost {
            cx,
            page_rows: 20,
            syntax: Some(syntax),
            server_folds: None,
            language: loaded_language(&view.syntax),
            vim: false,
        };
        let found = host.definition_objects(offset);
        assert_eq!(found, expected);
        let texts: Vec<_> = (found.iter())
            .map(|object| (object.function, &RUST[object.outer.clone()]))
            .collect();
        assert_eq!(
            texts,
            [
                (true, "fn f(&self) {\n        g();\n    }"),
                (false, RUST.trim_end()),
            ]
        );
        assert_eq!(
            found[0].inner.clone().map(|inner| &RUST[inner]),
            Some("g();")
        );
    });
    // The trait's default has none.
    assert_eq!(HeadlessHost::new().definition_objects(offset), []);
}
