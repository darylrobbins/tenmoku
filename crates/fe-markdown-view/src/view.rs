//! The view: a document's blocks in a scrolling, centred column.

use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use fe_markdown::Document;
use gpui::{
    App, Bounds, Context, Div, EventEmitter, FocusHandle, Focusable, Font, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, ScrollHandle, ScrollWheelEvent,
    StatefulInteractiveElement as _, Styled as _, TextRun, WeakEntity, Window, div, relative,
};

use crate::{MarkdownStyle, blocks, image::Resolved, inlines::LinkTo};

/// A Markdown document drawn as VS Code's preview draws it. It shows
/// what it's given and acts on nothing: a link click is a
/// [`MarkdownViewEvent`].
pub struct MarkdownView {
    document: Arc<Document>,
    doc_path: PathBuf,
    root: Option<PathBuf>,
    style: MarkdownStyle,
    scroll: ScrollHandle,
    focus: FocusHandle,
    /// The link under the pointer, by the text it's in: a link cut into
    /// pieces is one text per piece, which hear of a move in any order.
    hovers: HashMap<usize, LinkTo>,
    /// Where each image destination resolved to, for this document.
    images: HashMap<String, Resolved>,
    /// Where the last frame put the blocks.
    tracked: Rc<RefCell<Tracked>>,
    /// A source line to scroll to once a frame has placed the blocks.
    pending_line: Option<f32>,
    /// The scroll offset as of the last [`MarkdownViewEvent::Scrolled`] or
    /// programmatic scroll.
    reported_y: f32,
    /// Whether `reported_y` waits on the next frame, which applies a
    /// heading scroll, to settle.
    rebaseline: bool,
}

/// A block, list item or table row in the last frame: the source lines it
/// spans and where it is, in pixels from the top of the first top-level
/// block.
#[derive(Clone, Debug, PartialEq)]
struct Region {
    lines: Range<u32>,
    top: f32,
    bottom: f32,
}

/// What the last frame laid out, filled as it is painted.
#[derive(Default)]
pub(crate) struct Tracked {
    /// Nested regions in window coordinates, until the top-level blocks
    /// give the origin.
    raw: Vec<(Range<u32>, Bounds<Pixels>)>,
    /// Innermost first, as they were recorded.
    regions: Layout,
    /// Whether `regions` are of the document and style being shown.
    fresh: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownViewEvent {
    /// The user scrolled the view, with the wheel or a trackpad: not
    /// [`MarkdownView::scroll_to_source_line`] or
    /// [`MarkdownView::scroll_to_heading`]. Read the new place with
    /// [`MarkdownView::source_line_at_top`].
    Scrolled,
    /// A link was clicked; `dest` is as written (see
    /// [`fe_markdown::link_target`]).
    LinkClicked { dest: String },
}

impl EventEmitter<MarkdownViewEvent> for MarkdownView {}

impl MarkdownView {
    /// The view of `document`, the file at `doc_path`. Relative links and
    /// images resolve against its folder and rooted ones against `root`,
    /// the workspace folder; only images inside one of the two are shown.
    pub fn new(
        document: Arc<Document>,
        doc_path: PathBuf,
        root: Option<PathBuf>,
        style: MarkdownStyle,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            document,
            doc_path,
            root,
            style,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            hovers: HashMap::new(),
            images: HashMap::new(),
            tracked: Rc::default(),
            pending_line: None,
            reported_y: 0.,
            rebaseline: false,
        }
    }

    pub fn document(&self) -> &Arc<Document> {
        &self.document
    }

    /// Shows `document` instead, keeping the scroll position.
    pub fn set_document(&mut self, document: Arc<Document>, cx: &mut Context<Self>) {
        self.document = document;
        self.hovers.clear();
        self.images.clear();
        self.tracked.borrow_mut().fresh = false;
        cx.notify();
    }

    /// The file the document is, which relative links and images resolve
    /// against.
    pub fn doc_path(&self) -> &Path {
        &self.doc_path
    }

    /// The workspace folder rooted links and images resolve against.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Resolves links and images against `doc_path` and `root` instead,
    /// as [`MarkdownView::new`] does, once the file moves or the workspace
    /// folder changes.
    pub fn set_paths(&mut self, doc_path: PathBuf, root: Option<PathBuf>, cx: &mut Context<Self>) {
        if (&self.doc_path, &self.root) != (&doc_path, &root) {
            self.doc_path = doc_path;
            self.root = root;
            self.images.clear();
            cx.notify();
        }
    }

    pub fn style(&self) -> &MarkdownStyle {
        &self.style
    }

    pub fn set_style(&mut self, style: MarkdownStyle, cx: &mut Context<Self>) {
        if self.style != style {
            self.style = style;
            cx.notify();
        }
    }

    /// The handle of the scrolling column, whose children are the
    /// document's top-level blocks in order.
    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    /// Scrolls the heading `slug` names (see [`Document::heading`]) to the
    /// top, or the top-level block holding it, at the next frame. Whether
    /// there's such a heading.
    pub fn scroll_to_heading(&mut self, slug: &str, cx: &mut Context<Self>) -> bool {
        let Some(heading) = self.document.heading(slug) else {
            return false;
        };
        let Some(ix) = (self.document.blocks.iter())
            .position(|block| block.lines.contains(&heading.lines.start))
        else {
            return false;
        };
        self.scroll.scroll_to_top_of_item(ix);
        self.pending_line = None;
        self.rebaseline = true;
        cx.notify();
        true
    }

    /// The source line at the top of the view, fractional through a block
    /// by how far down it the top is, and between two blocks by how far
    /// between them. `None` before the first frame, or with no blocks.
    pub fn source_line_at_top(&self) -> Option<f32> {
        let tracked = self.tracked.borrow();
        if !tracked.fresh || tracked.regions.is_empty() {
            return None;
        }
        let y = -self.settled_y() - f32::from(MarkdownStyle::PADDING);
        Some(line_at(&tracked.regions, y))
    }

    /// Scrolls so `line`, fractional, is at the top, at the next frame
    /// if one hasn't placed the blocks yet; a line past the last block
    /// scrolls to the end. It doesn't emit [`MarkdownViewEvent::Scrolled`].
    pub fn scroll_to_source_line(&mut self, line: f32, cx: &mut Context<Self>) {
        if !self.tracked.borrow().fresh {
            self.pending_line = Some(line);
            return;
        }
        self.pending_line = None;
        let tracked = self.tracked.borrow();
        let Some(y) = offset_of(&tracked.regions, line) else {
            return;
        };
        drop(tracked);
        let max = f32::from(self.scroll.max_offset().y);
        let offset = match y {
            Some(y) => (y + f32::from(MarkdownStyle::PADDING)).clamp(0., max),
            None => max,
        };
        let mut position = self.scroll.offset();
        position.y = gpui::px(-offset);
        self.scroll.set_offset(position);
        self.reported_y = -offset;
        cx.notify();
    }

    /// Applies a line asked for before a frame placed the blocks.
    fn apply_pending(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.rebaseline) {
            self.reported_y = self.settled_y();
        }
        if let Some(line) = self.pending_line.take() {
            self.scroll_to_source_line(line, cx);
        }
    }

    /// The scroll offset as the next frame will clamp it: GPUI adds a
    /// wheel delta at once and clamps it only when it next paints.
    fn settled_y(&self) -> f32 {
        let max = f32::from(self.scroll.max_offset().y).max(0.);
        f32::from(self.scroll.offset().y).clamp(-max, 0.)
    }

    /// After a wheel event, the internal handler having run: tells whoever
    /// follows the scroll if the offset moved.
    fn wheeled(&mut self, cx: &mut Context<Self>) {
        let y = self.settled_y();
        if y != self.reported_y {
            self.reported_y = y;
            self.pending_line = None;
            cx.emit(MarkdownViewEvent::Scrolled);
        }
    }

    /// The destination of the link under the pointer, as written.
    pub fn hovered_link(&self) -> Option<&str> {
        self.hovers.values().next().map(|link| link.dest.as_str())
    }

    /// Records that the pointer is over `link` in the text `text`, or over
    /// no link in it.
    pub(crate) fn hover_text(&mut self, text: usize, link: Option<LinkTo>, cx: &mut Context<Self>) {
        let changed = match link {
            Some(link) => self.hovers.insert(text, link.clone()) != Some(link),
            None => self.hovers.remove(&text).is_some(),
        };
        if changed {
            cx.notify();
        }
    }
}

impl Focusable for MarkdownView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The regions of a frame and how they nest.
#[derive(Default)]
pub(crate) struct Layout {
    regions: Vec<Region>,
    /// Each region's children in document order, then, last, the
    /// top-level ones.
    children: Vec<Vec<usize>>,
}

/// What the mappings walk: a region, or the whole document.
struct Node<'a> {
    lines: Range<u32>,
    top: f32,
    bottom: f32,
    kids: &'a [usize],
}

impl Layout {
    fn new(regions: Vec<Region>) -> Self {
        let contains = |a: &Region, b: &Region| {
            a.lines.start <= b.lines.start
                && b.lines.end <= a.lines.end
                && a.top - 0.5 <= b.top
                && b.bottom <= a.bottom + 0.5
        };
        // Outer before inner; of identical regions, the later recorded
        // is the outer.
        let mut order: Vec<usize> = (0..regions.len()).collect();
        order.sort_by(|&a, &b| {
            let (a, b) = (&regions[a], &regions[b]);
            (a.lines.start.cmp(&b.lines.start))
                .then(b.lines.end.cmp(&a.lines.end))
                .then((b.bottom - b.top).total_cmp(&(a.bottom - a.top)))
        });
        // The stable sort keeps recorded order among identical regions,
        // where the later is the outer: flip each run of them.
        let mut ix = 0;
        while ix < order.len() {
            let mut end = ix + 1;
            while end < order.len() && regions[order[end]] == regions[order[ix]] {
                end += 1;
            }
            order[ix..end].reverse();
            ix = end;
        }
        let mut children = vec![Vec::new(); regions.len() + 1];
        let mut stack: Vec<usize> = Vec::new();
        for &ix in &order {
            while let Some(&top) = stack.last() {
                if contains(&regions[top], &regions[ix]) {
                    break;
                }
                stack.pop();
            }
            children[stack.last().copied().unwrap_or(regions.len())].push(ix);
            stack.push(ix);
        }
        Self { regions, children }
    }

    fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    fn node(&self, ix: usize) -> Node<'_> {
        let region = &self.regions[ix];
        Node {
            lines: region.lines.clone(),
            top: region.top,
            bottom: region.bottom,
            kids: &self.children[ix],
        }
    }

    fn root(&self) -> Node<'_> {
        let kids = &self.children[self.regions.len()];
        let all = || kids.iter().map(|&ix| &self.regions[ix]);
        Node {
            lines: all().map(|r| r.lines.start).min().unwrap_or(0)
                ..all().map(|r| r.lines.end).max().unwrap_or(0),
            top: all().map(|r| r.top).fold(f32::INFINITY, f32::min),
            bottom: all().map(|r| r.bottom).fold(f32::NEG_INFINITY, f32::max),
            kids,
        }
    }
}

/// Between `prev` and `next`, the neighbours inside `node` or the
/// node's own edges: the lines and pixels the gap spans.
fn gap(node: &Node, prev: Option<&Region>, next: Option<&Region>) -> (f32, f32, f32, f32) {
    let end = node.lines.start + span(&node.lines);
    (
        prev.map_or(node.lines.start, |r| r.lines.end) as f32,
        next.map_or(end, |r| r.lines.start) as f32,
        prev.map_or(node.top, |r| r.bottom),
        next.map_or(node.bottom, |r| r.top),
    )
}

/// The source line at `y` pixels from the top of the first block. Nested
/// regions win over their parents; what they leave uncovered, between
/// neighbours or at a parent's edges, is interpolated.
fn line_at(layout: &Layout, y: f32) -> f32 {
    let mut node = layout.root();
    if y < node.top {
        return node.lines.start as f32;
    }
    if y >= node.bottom {
        return node.lines.end as f32;
    }
    loop {
        let kids = || node.kids.iter().map(|&ix| (ix, &layout.regions[ix]));
        if let Some((ix, _)) = kids().find(|(_, r)| r.top <= y && y < r.bottom) {
            node = layout.node(ix);
            continue;
        }
        let prev = kids().rfind(|(_, r)| r.bottom <= y).map(|(_, r)| r);
        let next = kids().find(|(_, r)| r.top > y).map(|(_, r)| r);
        let (from, to, top, bottom) = gap(&node, prev, next);
        if bottom <= top {
            return from;
        }
        return from + (y - top) / (bottom - top) * (to - from).max(0.);
    }
}

/// Where `line` is, in pixels from the top of the first block: `None` past
/// the last block, and the outer `None` with no blocks. At or before the
/// first block's line it is above the first block, where the view's own
/// padding is.
fn offset_of(layout: &Layout, line: f32) -> Option<Option<f32>> {
    if layout.is_empty() {
        return None;
    }
    let mut node = layout.root();
    if line <= node.lines.start as f32 {
        return Some(Some(f32::NEG_INFINITY));
    }
    if line >= node.lines.end as f32 {
        return Some(None);
    }
    loop {
        let kids = || node.kids.iter().map(|&ix| (ix, &layout.regions[ix]));
        let spans = |r: &Region| r.lines.start as f32 <= line && line < r.lines.end as f32;
        if let Some((ix, _)) = kids().find(|(_, r)| spans(r)) {
            node = layout.node(ix);
            continue;
        }
        let prev = kids()
            .rfind(|(_, r)| r.lines.end as f32 <= line)
            .map(|(_, r)| r);
        let next = kids()
            .find(|(_, r)| r.lines.start as f32 > line)
            .map(|(_, r)| r);
        let (from, to, top, bottom) = gap(&node, prev, next);
        let fraction = if to > from {
            (line - from) / (to - from)
        } else {
            0.
        };
        return Some(Some(top + fraction * (bottom - top)));
    }
}

fn span(lines: &Range<u32>) -> u32 {
    lines.end.saturating_sub(lines.start).max(1)
}

/// Where a rendered link's click and hover go: the view that draws the
/// document, or a host's callback, which has no hover.
#[derive(Clone)]
pub(crate) enum Sink {
    View(WeakEntity<MarkdownView>),
    Callback(Option<LinkHandler>),
}

pub type LinkHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

impl Sink {
    pub(crate) fn clicked(&self, dest: String, window: &mut Window, cx: &mut App) {
        match self {
            Sink::View(view) => {
                view.update(cx, |_, cx| cx.emit(MarkdownViewEvent::LinkClicked { dest }))
                    .ok();
            }
            Sink::Callback(handler) => {
                if let Some(handler) = handler {
                    handler(&dest, window, cx);
                }
            }
        }
    }

    pub(crate) fn hovered(&self, text: usize, link: Option<LinkTo>, cx: &mut App) {
        if let Sink::View(view) = self {
            view.update(cx, |view, cx| view.hover_text(text, link, cx))
                .ok();
        }
    }
}

/// What rendering a block needs: the style, where images resolve, the
/// view for link events, and counters that give links, texts, images and
/// task items their places in the document.
pub(crate) struct RenderCx<'a> {
    pub(crate) style: &'a MarkdownStyle,
    pub(crate) doc_path: &'a Path,
    pub(crate) root: Option<&'a Path>,
    pub(crate) images: &'a mut HashMap<String, Resolved>,
    /// Where link clicks and hovers go.
    pub(crate) sink: Sink,
    /// Whether this is [`crate::markdown_element`]'s layout: headings
    /// capped, images shown as their alt text.
    pub(crate) compact: bool,
    pub(crate) tracked: Rc<RefCell<Tracked>>,
    pub(crate) window: &'a mut Window,
    pub(crate) app: &'a mut App,
    /// Body text's font: the window's default.
    pub(crate) font: Font,
    /// The links under the pointer, by their places among the document's.
    pub(crate) hovered: Vec<usize>,
    /// How deep in lists the block being rendered is.
    pub(crate) list_depth: usize,
    pub(crate) links: usize,
    pub(crate) texts: usize,
    pub(crate) images_seen: usize,
    pub(crate) tasks: usize,
    pub(crate) codes: usize,
}

impl RenderCx<'_> {
    /// `container`, recording where its children are, which span `lines`
    /// in order.
    pub(crate) fn track(&self, container: Div, lines: Vec<Range<u32>>) -> Div {
        let tracked = self.tracked.clone();
        container.on_children_prepainted(move |bounds, _, _| {
            let mut tracked = tracked.borrow_mut();
            tracked.raw.extend(lines.iter().cloned().zip(bounds));
        })
    }

    pub(crate) fn is_hovered(&self, link: usize) -> bool {
        self.hovered.contains(&link)
    }

    /// How wide `text` is drawn with `runs` at the body text's size, on one
    /// line.
    pub(crate) fn shaped_width(&mut self, text: String, runs: &[TextRun]) -> f32 {
        let size = self.style.text_size;
        let line = (self.window.text_system()).shape_line(text.into(), size, runs, None);
        f32::from(line.width)
    }

    /// How wide `text` is in body text, on one line.
    pub(crate) fn text_width(&mut self, text: &str) -> f32 {
        let run = TextRun {
            len: text.len(),
            font: self.font.clone(),
            color: self.style.foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        self.shaped_width(text.to_owned(), &[run])
    }

    pub(crate) fn next_link(&mut self) -> usize {
        post_increment(&mut self.links)
    }

    pub(crate) fn next_text(&mut self) -> usize {
        post_increment(&mut self.texts)
    }

    pub(crate) fn next_image(&mut self) -> usize {
        post_increment(&mut self.images_seen)
    }

    pub(crate) fn next_task(&mut self) -> usize {
        post_increment(&mut self.tasks)
    }

    pub(crate) fn next_code(&mut self) -> usize {
        post_increment(&mut self.codes)
    }
}

fn post_increment(counter: &mut usize) -> usize {
    *counter += 1;
    *counter - 1
}

impl Render for MarkdownView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let style = &self.style;
        {
            let mut tracked = self.tracked.borrow_mut();
            tracked.raw.clear();
            tracked.fresh = false;
        }
        let mut rcx = RenderCx {
            style,
            doc_path: &self.doc_path,
            root: self.root.as_deref(),
            images: &mut self.images,
            sink: Sink::View(cx.weak_entity()),
            compact: false,
            tracked: self.tracked.clone(),
            font: window.text_style().font(),
            window,
            app: cx,
            hovered: self.hovers.values().map(|link| link.ordinal).collect(),
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
                    .max_w(MarkdownStyle::MAX_WIDTH)
                    .child(blocks::render(block, ix == 0, &mut rcx))
            })
            .collect();
        let lines: Vec<_> = self
            .document
            .blocks
            .iter()
            .map(|b| b.lines.clone())
            .collect();
        let (tracked, view, scroll) = (self.tracked.clone(), cx.weak_entity(), self.scroll.clone());
        let column = div()
            .on_children_prepainted(move |_, _, cx| {
                // A scrolling div hands its children's bounds to its handle.
                let bounds: Vec<_> = (0..lines.len())
                    .map_while(|ix| scroll.bounds_for_item(ix))
                    .collect();
                {
                    let mut tracked = tracked.borrow_mut();
                    let origin = bounds.first().map_or(Pixels::ZERO, |first| first.top());
                    let rel = |bounds: Bounds<Pixels>| {
                        (
                            f32::from(bounds.top() - origin),
                            f32::from(bounds.bottom() - origin),
                        )
                    };
                    let mut regions: Vec<Region> = std::mem::take(&mut tracked.raw)
                        .into_iter()
                        .map(|(lines, bounds)| {
                            // Nested bounds are where the scrolled column
                            // paints them.
                            let (top, bottom) = rel(bounds);
                            let scrolled = f32::from(scroll.offset().y);
                            Region {
                                lines,
                                top: top - scrolled,
                                bottom: bottom - scrolled,
                            }
                        })
                        .collect();
                    for (lines, bounds) in lines.iter().zip(&bounds) {
                        let (top, bottom) = rel(*bounds);
                        regions.push(Region {
                            lines: lines.clone(),
                            top,
                            bottom,
                        });
                    }
                    tracked.regions = Layout::new(regions);
                    tracked.fresh = true;
                }
                view.update(cx, |view, cx| view.apply_pending(cx)).ok();
            })
            .id("markdown-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .on_scroll_wheel(cx.listener(|_, _: &ScrollWheelEvent, window, cx| {
                // Once the wheel has scrolled the column.
                let view = cx.weak_entity();
                window.defer(cx, move |_, cx| {
                    view.update(cx, |view, cx| view.wheeled(cx)).ok();
                });
            }))
            .flex()
            .flex_col()
            .items_center()
            .p(MarkdownStyle::PADDING)
            .gap(MarkdownStyle::BLOCK_GAP)
            .children(blocks);
        div()
            .id("markdown-view")
            .track_focus(&self.focus)
            .size_full()
            .bg(style.background)
            .text_color(style.foreground)
            .text_size(style.text_size)
            .line_height(relative(style.line_height))
            .child(column)
    }
}
