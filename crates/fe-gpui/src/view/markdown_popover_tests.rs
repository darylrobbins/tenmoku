//! The hover popover and the completion documentation drawn through a
//! host-set [`MarkdownRenderer`], and as text without one.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{BufferId, Version};
use fe_editor::completion::{CompletionItem, CompletionList, CompletionTrigger};

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement as _, Styled as _, TestAppContext,
    Window, div, px,
};

use super::{TEXT, calls, editor, painted, rest, word};
use crate::{
    hover::{Block, UI_FONT},
    *,
};

/// Records each call, and draws a div with a debug selector.
#[derive(Default)]
struct TestRenderer {
    calls: RefCell<Vec<(String, gpui::Pixels)>>,
}

impl MarkdownRenderer for TestRenderer {
    fn render(
        &self,
        markdown: &str,
        _: &EditorTheme,
        max_width: gpui::Pixels,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        self.calls.borrow_mut().push((markdown.into(), max_width));
        div()
            .debug_selector(|| "test-markdown".into())
            .size(px(10.))
            .into_any_element()
    }
}

#[gpui::test]
fn a_set_renderer_draws_the_hover_and_the_docs(cx: &mut TestAppContext) {
    let hovered = "# T\n`x`";
    let (view, provider, cx) = editor(0, cx);
    *provider.reply.borrow_mut() = Some(HoverContent {
        text: hovered.into(),
        range: None,
    });
    let at = super::at(&view, word("alpha").start + 1, cx);

    // Without one: text and code, as ever.
    rest(at, cx);
    assert_eq!(calls(&provider).len(), 1);
    let as_text = vec![(Block::Text("# T\n`x`".into()), UI_FONT.into())];

    assert_eq!(painted(&view, cx), as_text);
    assert!(cx.debug_bounds("test-markdown").is_none());

    // With one: it draws the markdown once, and the popover holds its
    // element.
    let renderer = Rc::new(TestRenderer::default());
    view.update(cx, |view, cx| {
        view.set_markdown_renderer(Some(renderer.clone()));
        assert!(view.markdown_renderer().is_some());
        cx.notify();
    });
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        view.hide_popover(cx);
        cx.notify();
    });
    rest(super::at(&view, word("beta").start + 1, cx), cx);
    let drawn = renderer.calls.borrow().clone();
    assert!(!drawn.is_empty());
    assert!(drawn.iter().all(|(markdown, _)| markdown == hovered));
    assert!(
        drawn
            .iter()
            .all(|(_, width)| *width > px(0.) && *width < px(560.))
    );
    let popover = cx.debug_bounds("hover-popover").expect("the popover");
    let element = cx
        .debug_bounds("test-markdown")
        .expect("the renderer's element");
    assert!(popover.contains(&element.origin));
    let text = view.read_with(cx, |view, _| view.popover_text());
    assert_eq!(text.as_deref(), Some("# T\n`x`"));

    // The completion docs pane too, through the same renderer; as text
    // without one.
    view.update(cx, |view, cx| {
        view.hide_popover(cx);
        view.set_completion_provider(Some(Rc::new(Documented)));
        cx.notify();
    });
    cx.run_until_parked();
    renderer.calls.borrow_mut().clear();
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(TEXT.find("alpha").unwrap() + 2);
        cx.notify();
    });
    cx.simulate_keystrokes("ctrl-space");
    cx.run_until_parked();
    let docs = view.read_with(cx, |view, _| view.painted_completion.clone());
    assert_eq!(docs.unwrap().documentation.as_deref(), Some("The *al*."));
    let drawn = renderer.calls.borrow().clone();
    assert!(!drawn.is_empty());
    assert!(drawn.iter().all(|(markdown, _)| markdown == "The *al*."));
    assert!(cx.debug_bounds("test-markdown").is_some());

    cx.simulate_keystrokes("escape");
    view.update(cx, |view, cx| {
        view.set_markdown_renderer(None);
        cx.notify();
    });
    renderer.calls.borrow_mut().clear();
    cx.simulate_keystrokes("ctrl-space");
    cx.run_until_parked();
    assert!(renderer.calls.borrow().is_empty());
    assert!(cx.debug_bounds("test-markdown").is_none());
}

/// Offers one item, documented from the start.
struct Documented;

impl CompletionProvider for Documented {
    fn trigger_characters(&self, _: BufferId) -> Vec<String> {
        Vec::new()
    }

    fn completions(
        &self,
        _: BufferId,
        _: Version,
        offset: usize,
        _: CompletionTrigger,
    ) -> CompletionFuture {
        let item = CompletionItem {
            label: "alpha".into(),
            documentation: Some("The *al*.".into()),
            edit: (offset - 2..offset, "alpha".into()),
            ..CompletionItem::default()
        };
        Box::pin(async move {
            Some(CompletionList {
                items: vec![item],
                is_incomplete: false,
            })
        })
    }

    fn resolve(&self, item: CompletionItem) -> ResolveFuture {
        Box::pin(async move { item })
    }
}
