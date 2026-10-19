//! The view: a document's blocks in a scrolling, centred column.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_markdown::Document;
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, Font, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, StatefulInteractiveElement as _, Styled as _,
    TextRun, WeakEntity, Window, div, relative,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkdownViewEvent {
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
        cx.notify();
        true
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

/// What rendering a block needs: the style, where images resolve, the
/// view for link events, and counters that give links, texts, images and
/// task items their places in the document.
pub(crate) struct RenderCx<'a> {
    pub(crate) style: &'a MarkdownStyle,
    pub(crate) doc_path: &'a Path,
    pub(crate) root: Option<&'a Path>,
    pub(crate) images: &'a mut HashMap<String, Resolved>,
    pub(crate) view: WeakEntity<MarkdownView>,
    pub(crate) window: &'a mut Window,
    pub(crate) app: &'a mut App,
    /// Body text's font: the window's default.
    pub(crate) font: Font,
    /// The links under the pointer, by their places among the document's.
    hovered: Vec<usize>,
    /// How deep in lists the block being rendered is.
    pub(crate) list_depth: usize,
    links: usize,
    texts: usize,
    images_seen: usize,
    tasks: usize,
    codes: usize,
}

impl RenderCx<'_> {
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
        let mut rcx = RenderCx {
            style,
            doc_path: &self.doc_path,
            root: self.root.as_deref(),
            images: &mut self.images,
            view: cx.weak_entity(),
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
        div()
            .id("markdown-view")
            .track_focus(&self.focus)
            .size_full()
            .bg(style.background)
            .text_color(style.foreground)
            .text_size(style.text_size)
            .line_height(relative(style.line_height))
            .child(
                div()
                    .id("markdown-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .items_center()
                    .p(MarkdownStyle::PADDING)
                    .gap(MarkdownStyle::BLOCK_GAP)
                    .children(blocks),
            )
    }
}
