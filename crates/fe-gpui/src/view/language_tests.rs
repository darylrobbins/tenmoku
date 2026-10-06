use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::*;

/// An editor on `text` at `path`, once its language has loaded and parsed.
fn editor<'a>(
    text: &str,
    path: Option<&str>,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (text, path) = (text.to_owned(), path.map(Into::into));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), path);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    (view, cx)
}

/// The language the view highlights as, and the highlight of the first
/// occurrence of `word`.
fn highlighted(
    view: &Entity<EditorView>,
    word: &str,
    cx: &mut VisualTestContext,
) -> Option<(&'static str, Option<&'static str>)> {
    cx.run_until_parked();
    view.update(cx, |view, _| {
        let syntax = view.syntax()?;
        let text = view
            .model()
            .text()
            .slice_to_string(0..view.model().text().len());
        let start = text.find(word).expect("word in text");
        let highlight = syntax
            .highlights(0..text.len())
            .into_iter()
            .find(|span| span.range == (start..start + word.len()))
            .map(|span| span.highlight.name());
        Some((syntax.language().name(), highlight))
    })
}

#[gpui::test]
fn set_language_highlights_an_untitled_buffer(cx: &mut TestAppContext) {
    let (view, cx) = editor("def f(): pass\n", None, cx);
    assert_eq!(highlighted(&view, "def", cx), None);
    view.update(cx, |view, cx| view.set_language(Some("python"), cx));
    assert_eq!(
        highlighted(&view, "def", cx),
        Some(("python", Some("keyword")))
    );
    view.update(cx, |view, cx| view.set_language(None, cx));
    assert_eq!(highlighted(&view, "def", cx), None);
}

#[gpui::test]
fn switching_language_mid_parse_discards_the_old_parse(cx: &mut TestAppContext) {
    // Loaded up front so the view parses as soon as it opens.
    let rust = fe_syntax::language_by_name("rust").expect("rust loads");
    let (view, cx) = editor("fn f() {}\n", Some("main.rs"), cx);
    view.update(cx, |view, cx| {
        // A clock that moves a second on at each read is always over the sync
        // budget, so the Rust parse goes to the background whatever the load.
        let start = std::time::Instant::now();
        let elapsed = std::cell::Cell::new(std::time::Duration::ZERO);
        let clock = move || {
            elapsed.set(elapsed.get() + std::time::Duration::from_secs(1));
            start + elapsed.get()
        };
        view.syntax =
            Some(crate::view::syntax_driver(rust, view.model.snapshot()).with_clock(clock));
        let text = "fn f() {}\n".repeat(200);
        view.model.select_all();
        view.model.insert(&text);
        view.sync_syntax(cx);
        assert!(view.syntax.as_ref().expect("rust loaded").is_parsing());
        view.set_language(Some("python"), cx);
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        let syntax = view.syntax().expect("python loaded");
        assert_eq!(syntax.language().name(), "python");
        let tree = syntax.tree().expect("parsed");
        assert_eq!(tree.root_node().kind(), "module");
    });
}

#[gpui::test]
fn a_shebang_picks_the_language_at_open(cx: &mut TestAppContext) {
    let (view, cx) = editor("#!/usr/bin/env python3\ndef f(): pass\n", Some("tool"), cx);
    assert_eq!(
        highlighted(&view, "def", cx),
        Some(("python", Some("keyword")))
    );
}

#[gpui::test]
fn hosts_read_the_parsed_tree(cx: &mut TestAppContext) {
    let (view, cx) = editor("fn f() {}\n", Some("main.rs"), cx);
    view.update(cx, |view, cx| {
        view.model.set_cursor(0);
        view.model.insert("struct S;\n");
        view.changed(cx);
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        let syntax = view.syntax().expect("rust loaded");
        assert_eq!(syntax.version(), view.model().snapshot().version());
        assert!(!syntax.needs_parse());
        let root = syntax.tree().expect("parsed").root_node();
        let kinds: Vec<_> = (0..root.named_child_count())
            .filter_map(|i| root.named_child(i as u32))
            .map(|node| node.kind())
            .collect();
        assert_eq!(kinds, ["struct_item", "function_item"]);
    });
}
