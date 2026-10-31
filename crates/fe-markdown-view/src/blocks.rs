// Adapted from longbridge/gpui-kit@2154ba8, crates/base/src/text/utils.rs and crates/base/src/text/node.rs; Apache-2.0.
// Commit 2154ba84afd2e771f117fd17e55c060dd5e03fd4.
// `list_item_prefix` and `ordered_list_ordinal` come from utils.rs, and
// `CHECK_SVG_LIGHT` and `CHECK_SVG_DARK` from node.rs. Changed: ordinals
// are u64, as fe-markdown's list starts are; nested ordered lists keep
// decimal numbers, as VS Code's preview does, instead of going to letters;
// the marker has no trailing space; the check mark's colour is picked by
// the fill it sits on rather than by the theme's appearance. The rest of
// this file is ours.

//! Blocks: paragraphs, headings, quotes, lists and task items, code, rules
//! and raw HTML; and [`document_element`], a document's blocks for a host
//! to place.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use fe_markdown::{Block, BlockKind, Document, ListItem};
use gpui::{
    AnyElement, App, Div, FontWeight, Hsla, Image, ImageFormat, InteractiveElement as _,
    IntoElement, ParentElement as _, RenderOnce, StatefulInteractiveElement as _, Styled as _,
    StyledText, Window, div, img, prelude::FluentBuilder as _, px, relative,
};

use crate::{
    MarkdownStyle,
    inlines::{self, Base},
    table,
    view::{LinkHandler, RenderCx, Sink},
};

const BULLETS: [&str; 5] = ["•", "◦", "▪", "‣", "⁃"];

const CHECK_SVG_LIGHT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="m3.25 8.25 3 3 6.5-7" stroke="white" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
const CHECK_SVG_DARK: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="m3.25 8.25 3 3 6.5-7" stroke="black" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;

/// How [`document_element`] draws a document's images, links and code
/// blocks.
#[derive(Clone)]
pub struct ElementOptions {
    pub images: Images,
    /// Called with a clicked link's destination, as written (see
    /// [`fe_markdown::link_target`]).
    pub on_link: Option<LinkHandler>,
    /// Draws the header above each code block, fenced or indented.
    pub code_header: Option<CodeHeaderFn>,
}

/// Draws a code block's header: see [`ElementOptions::code_header`].
pub type CodeHeaderFn = Rc<dyn Fn(CodeHeader<'_>, &mut Window, &mut App) -> AnyElement>;

/// Whether a document's images are drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Images {
    /// As [`crate::MarkdownView`] draws them: local files inside the folder
    /// of `doc_path`, the document's file, or inside `root`, the workspace
    /// folder, and placeholders for the rest.
    Load {
        doc_path: PathBuf,
        root: Option<PathBuf>,
    },
    /// Each image is its alt text, muted, and no file is read.
    AltText,
}

/// A code block, for its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeHeader<'a> {
    /// Its place among the document's code blocks, in document order.
    pub index: usize,
    /// The fence's info string; empty for an indented block.
    pub info: &'a str,
    /// The fe-syntax language the info string names.
    pub language: Option<&'static str>,
    /// The code, with `\n` line endings.
    pub code: &'a str,
}

/// `document`'s blocks as [`crate::MarkdownView`] draws them, in a column
/// as wide as its parent and as tall as they are: no scroll, padding,
/// width cap or focus, for a host that places documents in a scrolling
/// view of its own, such as a chat's replies. Images, link clicks and code
/// block headers are as `options` says; links aren't underlined on hover.
/// Test builds give each block the `debug_selector` `md-block-<ix>`.
pub fn document_element(
    document: &Arc<Document>,
    style: &MarkdownStyle,
    options: ElementOptions,
) -> AnyElement {
    DocumentElement {
        document: document.clone(),
        style: style.clone(),
        options,
    }
    .into_any_element()
}

/// Built when drawn, which is when the window is at hand to shape text.
#[derive(IntoElement)]
struct DocumentElement {
    document: Arc<Document>,
    style: MarkdownStyle,
    options: ElementOptions,
}

impl RenderOnce for DocumentElement {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = &self.style;
        let (doc_path, root) = match &self.options.images {
            Images::Load { doc_path, root } => (doc_path.as_path(), root.as_deref()),
            Images::AltText => (Path::new(""), None),
        };
        let mut images = HashMap::new();
        let mut rcx = RenderCx {
            style,
            doc_path,
            root,
            images: &mut images,
            sink: Sink::Callback(self.options.on_link.clone()),
            compact: false,
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
        let element = Element {
            alt_text: self.options.images == Images::AltText,
            code_header: self.options.code_header.clone(),
        };
        let blocks: Vec<_> = with_element(element, || {
            (self.document.blocks.iter().enumerate())
                .map(|(ix, block)| {
                    div()
                        .debug_selector(|| format!("md-block-{ix}"))
                        .flex_none()
                        .w_full()
                        .child(render(block, ix == 0, &mut rcx))
                })
                .collect()
        });
        div()
            .flex()
            .flex_col()
            .w_full()
            .gap(MarkdownStyle::BLOCK_GAP)
            .text_color(style.foreground)
            .text_size(style.text_size)
            .line_height(relative(style.line_height))
            .children(blocks)
    }
}

/// What [`document_element`] asks of the drawing that [`RenderCx`] doesn't
/// carry. It's set for the blocks that element draws, and read where it
/// matters, so that [`crate::MarkdownView`] and [`crate::markdown_element`]
/// draw through the same code without it: `RenderCx` is the view's, and
/// the view is left alone.
#[derive(Clone)]
pub(crate) struct Element {
    /// Images drawn as their alt text.
    pub(crate) alt_text: bool,
    code_header: Option<CodeHeaderFn>,
}

thread_local! {
    static ELEMENT: RefCell<Option<Element>> = const { RefCell::new(None) };
}

/// Runs `draw` with `element` as [`element`]'s, putting back what was
/// there before, even if `draw` panics.
fn with_element<R>(element: Element, draw: impl FnOnce() -> R) -> R {
    struct Restore(Option<Element>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ELEMENT.with(|current| *current.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(ELEMENT.with(|current| current.borrow_mut().replace(element)));
    draw()
}

/// What the [`document_element`] being drawn asks for; `None` while a
/// [`crate::MarkdownView`] or [`crate::markdown_element`] is drawn.
pub(crate) fn element() -> Option<Element> {
    ELEMENT.with(|current| current.borrow().clone())
}

/// An ordered-list item's ordinal, the list starting at one if it doesn't
/// say.
fn ordered_list_ordinal(start: Option<u64>, ix: usize) -> u64 {
    start
        .unwrap_or(1)
        .saturating_add(u64::try_from(ix).unwrap_or(u64::MAX))
}

/// A list item's marker: its number, or a bullet by how deep the list is.
fn list_item_prefix(ix: usize, start: Option<u64>, ordered: bool, depth: usize) -> String {
    if ordered {
        format!("{}.", ordered_list_ordinal(start, ix))
    } else {
        BULLETS[depth.min(BULLETS.len() - 1)].to_owned()
    }
}

/// Space between a container's blocks: none in a tight list's items.
fn gap(tight: bool) -> gpui::Pixels {
    if tight {
        px(0.)
    } else {
        MarkdownStyle::BLOCK_GAP
    }
}

/// `blocks` one under another.
pub(crate) fn render_blocks(blocks: &[Block], tight: bool, cx: &mut RenderCx) -> Div {
    let children: Vec<_> = (blocks.iter().enumerate())
        .map(|(ix, block)| render(block, ix == 0, cx))
        .collect();
    let container = div().flex().flex_col().gap(gap(tight)).children(children);
    cx.track(container, blocks.iter().map(|b| b.lines.clone()).collect())
}

/// One block. `first` says whether it starts its container, where a
/// heading needs no space above it.
pub(crate) fn render(block: &Block, first: bool, cx: &mut RenderCx) -> AnyElement {
    let style = cx.style;
    let body = Base {
        weight: FontWeight::NORMAL,
        color: style.foreground,
    };
    match &block.kind {
        BlockKind::Paragraph(content) => {
            let inlines = inlines::layout(content, body, cx);
            div()
                .flex()
                .flex_col()
                .children(inlines::render(inlines, cx))
                .into_any_element()
        }
        BlockKind::Heading { level, content, .. } => {
            let level = *level;
            let base = Base {
                weight: MarkdownStyle::HEADING_WEIGHT,
                color: style.heading_color(level),
            };
            let mut size = style.heading_size(level);
            if cx.compact {
                // Hovers shouldn't shout.
                size = size.min(style.text_size * 1.25);
            }
            let inlines = inlines::layout(content, base, cx);
            div()
                .flex()
                .flex_col()
                .text_size(size)
                .line_height(relative(1.25))
                .when(!first, |this| this.pt(px(8.)))
                .when(level <= 2, |this| {
                    this.pb(size * 0.3).border_b_1().border_color(style.border)
                })
                .children(inlines::render(inlines, cx))
                .into_any_element()
        }
        BlockKind::Quote(blocks) => div()
            .pl(px(12.))
            .border_l(px(4.))
            .border_color(style.border)
            .child(render_blocks(blocks, false, cx))
            .into_any_element(),
        BlockKind::List {
            start,
            tight,
            items,
        } => {
            let depth = cx.list_depth;
            cx.list_depth += 1;
            let rendered: Vec<_> = (items.iter().enumerate())
                .map(|(ix, item)| render_item(item, ix, *start, *tight, depth, cx))
                .collect();
            let list = div().flex().flex_col().gap(gap(*tight)).children(rendered);
            let list = cx
                .track(list, items.iter().map(|item| item.lines.clone()).collect())
                .into_any_element();
            cx.list_depth = depth;
            list
        }
        BlockKind::Code {
            info,
            language,
            code,
            highlights,
        } => {
            let id = cx.next_code();
            let element = element();
            let header = (element.as_ref())
                .and_then(|element| element.code_header.as_ref())
                .map(|header| {
                    let block = CodeHeader {
                        index: id,
                        info,
                        language: *language,
                        code,
                    };
                    header(block, cx.window, cx.app)
                });
            let code = code.strip_suffix('\n').unwrap_or(code);
            let runs = style.code_runs(code, highlights);
            crate::PaintedRuns::record(cx.app, code, &runs);
            let block = div()
                .id(("md-code", id))
                .debug_selector(|| format!("md-code-{id}"))
                .w_full()
                // A flex row, so the text below keeps its own width, as
                // wide as its longest line: a block child would be
                // stretched to this box's width and leave no range.
                .flex()
                .flex_row()
                .overflow_x_scroll()
                // In a host's scrolling view, a vertical wheel scrolls
                // the view, not the code sideways.
                .when(element.is_some(), |this| this.restrict_scroll_to_axis())
                .when(header.is_none(), |this| this.rounded(px(4.)))
                .bg(style.code_background)
                .p(px(16.))
                .font_family(style.code_font_family.clone())
                .font_features(style.code_font_features.clone())
                .text_size(style.code_font_size)
                .line_height(relative(style.code_line_height))
                .whitespace_nowrap()
                .child(
                    div()
                        .debug_selector(|| format!("md-code-{id}-text"))
                        .flex_none()
                        .child(StyledText::new(code.to_owned()).with_runs(runs)),
                );
            match header {
                None => block.into_any_element(),
                Some(header) => div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .rounded(px(4.))
                    .overflow_hidden()
                    .bg(style.code_background)
                    .child(header)
                    .child(block)
                    .into_any_element(),
            }
        }
        BlockKind::Table { align, head, rows } => {
            table::render(align, head, rows, &block.lines, cx)
        }
        BlockKind::Rule => div().w_full().h(px(1.)).bg(style.border).into_any_element(),
        // Shown as written: the preview doesn't render HTML.
        BlockKind::Html(html) => div()
            .child(html.trim_end_matches('\n').to_owned())
            .into_any_element(),
    }
}

fn render_item(
    item: &ListItem,
    ix: usize,
    start: Option<u64>,
    tight: bool,
    depth: usize,
    cx: &mut RenderCx,
) -> AnyElement {
    let style = cx.style;
    let line_height = style.text_size * style.line_height;
    let indent = style.text_size * 2.;
    let marker = match item.task {
        Some(checked) => checkbox(checked, line_height, cx),
        // The marker hangs in an indent column, right-aligned against the
        // text like an outside list marker, and steps back in colour so
        // the item's text leads.
        None => div()
            .flex_none()
            .min_w(indent)
            .pr(indent * 0.3)
            .text_right()
            .text_color(style.muted)
            .child(list_item_prefix(ix, start, start.is_some(), depth)),
    };
    div()
        .flex()
        .flex_row()
        .items_start()
        .child(marker)
        .child(render_blocks(&item.blocks, tight, cx).flex_1().min_w_0())
        .into_any_element()
}

/// A task item's read-only checkbox, filled when checked. Its
/// `debug_selector` is `md-task-<n>`, and its check mark's
/// `md-task-<n>-check`, `n` being its place among the document's task
/// items.
fn checkbox(checked: bool, line_height: gpui::Pixels, cx: &mut RenderCx) -> Div {
    let style = cx.style;
    let n = cx.next_task();
    let check = if is_light(style.link) {
        CHECK_SVG_DARK
    } else {
        CHECK_SVG_LIGHT
    };
    div()
        .flex()
        .flex_none()
        .min_w(style.text_size * 2.)
        .h(line_height)
        .items_center()
        .justify_center()
        .child(
            div()
                .debug_selector(|| format!("md-task-{n}"))
                .flex()
                .size(px(14.))
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border(px(1.5))
                .border_color(style.muted)
                .when(checked, |this| {
                    this.bg(style.link).border_color(style.link).child(
                        img(Arc::new(Image::from_bytes(
                            ImageFormat::Svg,
                            check.to_vec(),
                        )))
                        .debug_selector(|| format!("md-task-{n}-check"))
                        .size(px(11.)),
                    )
                }),
        )
}

/// Whether a dark mark reads better than a white one on `fill`.
fn is_light(fill: Hsla) -> bool {
    fill.l > 0.6
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_item_prefixes() {
        assert_eq!(list_item_prefix(0, Some(1), true, 0), "1.");
        assert_eq!(list_item_prefix(10, Some(1), true, 0), "11.");
        assert_eq!(list_item_prefix(1, Some(3), true, 1), "4.");
        assert_eq!(list_item_prefix(0, Some(0), true, 2), "0.");
        assert_eq!(
            list_item_prefix(2, Some(u64::MAX), true, 0),
            format!("{}.", u64::MAX)
        );
        assert_eq!(list_item_prefix(0, None, false, 0), "•");
        assert_eq!(list_item_prefix(0, None, false, 1), "◦");
        assert_eq!(list_item_prefix(0, None, false, 2), "▪");
        assert_eq!(list_item_prefix(0, None, false, 9), "⁃");
    }
}
