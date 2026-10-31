//! Code blocks scroll sideways when a line is wider than the block.

use std::{path::PathBuf, rc::Rc, sync::Arc};

use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
use fe_markdown::Document;
use fe_markdown_view::{CodeHeader, ElementOptions, MarkdownStyle, MarkdownView, document_element};
use gpui::{
    AnyElement, Bounds, Context, IntoElement, Modifiers, ParentElement as _, Pixels, Render,
    ScrollDelta, ScrollWheelEvent, Styled as _, TestAppContext, TouchPhase, VisualTestContext,
    Window, div, point, px,
};
use kiln_ui::UiTheme;

struct Host {
    document: Arc<Document>,
    style: MarkdownStyle,
    header: bool,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let code_header = self.header.then(|| {
            Rc::new(
                |block: CodeHeader<'_>, _: &mut Window, _: &mut gpui::App| -> AnyElement {
                    div()
                        .h(px(20.))
                        .child(block.info.to_owned())
                        .into_any_element()
                },
            ) as fe_markdown_view::CodeHeaderFn
        });
        let options = ElementOptions {
            code_header,
            ..ElementOptions::default()
        };
        div().w(px(320.)).flex().flex_col().child(document_element(
            &self.document,
            &self.style,
            options,
        ))
    }
}

fn style(cx: &mut TestAppContext) -> MarkdownStyle {
    cx.update(|cx| {
        let editor = EditorTheme::from_palette(
            EditorPalette::fastedit_dark(),
            &EditorSettings::default(),
            cx,
        );
        MarkdownStyle::new(&editor, &UiTheme::default_dark())
    })
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn bounds(cx: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    cx.debug_bounds(selector.to_owned().leak())
        .unwrap_or_else(|| panic!("no {selector}"))
}

fn wheel(cx: &mut VisualTestContext, at: gpui::Point<Pixels>, dx: f32, dy: f32) {
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    settle(cx);
}

fn fence(line: &str) -> String {
    format!("```rust\n{line}\n```\n")
}

fn long() -> String {
    "x".repeat(200)
}

fn open_element<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
    header: bool,
) -> &'a mut VisualTestContext {
    let host = Host {
        document: Arc::new(fe_markdown::parse(markdown)),
        style: style(cx),
        header,
    };
    let (_, cx) = cx.add_window_view(|_, _| host);
    settle(cx);
    cx
}

/// Checks the scroll behavior of the code block drawn in `cx`.
fn check_scrolls(cx: &mut VisualTestContext) {
    let block = bounds(cx, "md-code-0");
    let text = bounds(cx, "md-code-0-text");
    assert!(text.right() > block.right(), "{text:?} in {block:?}");
    let x = text.origin.x;
    let over = block.center();

    wheel(cx, over, -120., 0.);
    assert_eq!(bounds(cx, "md-code-0-text").origin.x, x - px(120.));

    // A vertical wheel leaves the text alone.
    let x = bounds(cx, "md-code-0-text").origin.x;
    wheel(cx, over, 0., -50.);
    assert_eq!(bounds(cx, "md-code-0-text").origin.x, x);

    // Far right: the text's end plus the padding meets the block's edge.
    wheel(cx, over, -5000., 0.);
    let block = bounds(cx, "md-code-0");
    let text = bounds(cx, "md-code-0-text");
    let gap = (text.right() + px(16.) - block.right()).abs();
    assert!(gap <= px(1.), "{text:?} in {block:?}");
}

#[gpui::test]
fn long_code_lines_scroll_sideways(cx: &mut TestAppContext) {
    let markdown = fence(&long());
    let with_header = open_element(cx, &markdown, true);
    check_scrolls(with_header);
    let without = open_element(cx, &markdown, false);
    check_scrolls(without);
}

#[gpui::test]
fn short_code_lines_do_not_scroll(cx: &mut TestAppContext) {
    let cx = open_element(cx, &fence("fn main() {}"), false);
    let block = bounds(cx, "md-code-0");
    let x = bounds(cx, "md-code-0-text").origin.x;
    wheel(cx, block.center(), -120., 0.);
    assert_eq!(bounds(cx, "md-code-0-text").origin.x, x);
}

#[gpui::test]
fn the_preview_scrolls_code_sideways(cx: &mut TestAppContext) {
    let style = style(cx);
    let document = Arc::new(fe_markdown::parse(&fence(&long())));
    let (_, cx) = cx.add_window_view(move |_, cx| {
        MarkdownView::new(document, PathBuf::from("/doc.md"), None, style, cx)
    });
    settle(cx);
    let block = bounds(cx, "md-code-0");
    let text = bounds(cx, "md-code-0-text");
    assert!(text.right() > block.right(), "{text:?} in {block:?}");
    let x = text.origin.x;
    wheel(cx, block.center(), -120., 0.);
    assert_eq!(bounds(cx, "md-code-0-text").origin.x, x - px(120.));
}
