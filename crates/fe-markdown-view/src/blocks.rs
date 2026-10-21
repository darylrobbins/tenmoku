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
//! and raw HTML.

use std::sync::Arc;

use fe_markdown::{Block, BlockKind, ListItem};
use gpui::{
    AnyElement, Div, FontWeight, Hsla, Image, ImageFormat, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, StyledText, div, img,
    prelude::FluentBuilder as _, px, relative,
};

use crate::{
    MarkdownStyle,
    inlines::{self, Base},
    table,
    view::RenderCx,
};

const BULLETS: [&str; 5] = ["•", "◦", "▪", "‣", "⁃"];

const CHECK_SVG_LIGHT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="m3.25 8.25 3 3 6.5-7" stroke="white" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;
const CHECK_SVG_DARK: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none"><path d="m3.25 8.25 3 3 6.5-7" stroke="black" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"#;

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
            code, highlights, ..
        } => {
            let code = code.strip_suffix('\n').unwrap_or(code);
            let runs = style.code_runs(code, highlights);
            let id = cx.next_code();
            div()
                .id(("md-code", id))
                .w_full()
                .overflow_x_scroll()
                .rounded(px(4.))
                .bg(style.code_background)
                .p(px(16.))
                .font_family(style.code_font_family.clone())
                .font_features(style.code_font_features.clone())
                .text_size(style.code_font_size)
                .line_height(relative(style.code_line_height))
                .whitespace_nowrap()
                .child(StyledText::new(code.to_owned()).with_runs(runs))
                .into_any_element()
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
