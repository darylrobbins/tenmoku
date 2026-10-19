//! The compact form: a document's blocks in a narrow column, with no state.

use std::{collections::HashMap, path::Path, rc::Rc};

use fe_markdown::Document;
use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, RenderOnce,
    Styled as _, Window, div, relative,
};

use crate::{
    MarkdownStyle, blocks,
    view::{LinkHandler, RenderCx, Sink},
};

/// `document`'s blocks as [`crate::MarkdownView`] draws them, in a column at
/// most `max_width` wide with no padding, scroll or focus, for a hover
/// popover or completion docs. Headings are at most 1.25 em, images are
/// never loaded (their alt text is shown), and a link click calls `on_link`
/// with its destination as written. Pass `style.compact()` for VS Code's
/// hover text size. Test builds give each block the `debug_selector`
/// `md-block-<ix>`.
pub fn markdown_element(
    document: &Document,
    style: &MarkdownStyle,
    max_width: Pixels,
    on_link: Option<LinkHandler>,
) -> AnyElement {
    Compact {
        document: document.clone(),
        style: style.clone(),
        max_width,
        on_link,
    }
    .into_any_element()
}

/// Built when drawn, which is when the window is at hand to shape text.
#[derive(IntoElement)]
struct Compact {
    document: Document,
    style: MarkdownStyle,
    max_width: Pixels,
    on_link: Option<LinkHandler>,
}

impl RenderOnce for Compact {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = &self.style;
        let mut images = HashMap::new();
        let mut rcx = RenderCx {
            style,
            doc_path: Path::new(""),
            root: None,
            images: &mut images,
            sink: Sink::Callback(self.on_link.clone()),
            compact: true,
            tracked: Rc::default(),
            font: window.text_style().font(),
            window,
            app: cx,
            hovered: Vec::new(),
            list_depth: 0,
            links: 0,
            texts: 0,
            images_seen: 0,
            tasks: 0,
            codes: 0,
        };
        let blocks: Vec<_> = (self.document.blocks.iter().enumerate())
            .map(|(ix, block)| {
                div()
                    .debug_selector(|| format!("md-block-{ix}"))
                    .flex_none()
                    .w_full()
                    .child(blocks::render(block, ix == 0, &mut rcx))
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .w_full()
            .max_w(self.max_width)
            .gap(MarkdownStyle::BLOCK_GAP)
            .text_color(style.foreground)
            .text_size(style.text_size)
            .line_height(relative(style.line_height))
            .children(blocks)
    }
}
