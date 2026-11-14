//! The editor's hover popover: what the text under a still pointer says,
//! shown after [`HOVER_DELAY`] beside it, or at the caret on
//! `editor.show_hover` (docs/decisions/lsp-client.md §7). It holds the
//! message of each diagnostic there, with its related locations and its
//! code's link, then what the view's [`HoverProvider`] (a language
//! server's hover, say) says about the word there, once it answers.
//!
//! - **Asking.** The provider is asked once the delay is up, once per word
//!   rested on: moves within the word don't ask again. Leaving the word
//!   drops the request, which cancels it.
//! - **The card.** Diagnostics first, then the provider's Markdown laid
//!   out as the design's hover card ([`CardText`]): its leading fences are
//!   the module path, in faint small code, and the signature, in its syntax
//!   colours; the rest is the docs, drawn by the host's
//!   [`MarkdownRenderer`], or else as text with fenced code in the editor's
//!   font. A footer under the provider's answer says where the provider
//!   says it's defined ([`HoverProvider::definition`]) and, with vim on,
//!   `gd definition`. It's a floating surface: the popover colour (the
//!   design's raised surface), a hairline, radius 6 and the float shadow.
//! - **Hiding.** Moving off what it shows (the word asked about, or the
//!   range the provider answered for) hides it [`HOVER_HIDE_DELAY`] later,
//!   unless the pointer reaches the popover or comes back first: it stays
//!   while the pointer is over it, and leaving it starts the same wait.
//!   Once that's up, the still pointer is taken as a fresh move. Leaving
//!   the window, scrolling the editor (the wheel's, or the layout's to
//!   reveal the caret), an edit or `escape` hides it at once, wherever the
//!   pointer is, and so does any change to the diagnostics where it was
//!   found. One shown at the caret goes when the caret moves, and stays
//!   while the pointer moves over nothing to show. One only waiting out
//!   the delay goes at once. An answer for a buffer version the view has
//!   moved on from is dropped.
//! - **Reaching it.** The popover takes the pointer: the text under it
//!   sees no moves or clicks, and the wheel over it scrolls its content,
//!   which is at most [`MAX_POPOVER_ROWS`] of the editor's lines tall.
//! - **The host's.** A host shows its own text at the caret
//!   ([`EditorView::show_popover`]), as `editor.show_hover` shows the
//!   provider's, or as the pointer rests on an after-line decoration's text
//!   ([`EditorView::set_after_line_popover`]): line blame's commit, say.
//!   Neither asks the provider; each hides as the provider's would, and the
//!   pointer's once the pointer leaves the decoration. Resting on a gutter
//!   annotation shows the host's text for it the same way
//!   ([`crate::GutterAnnotations::hover`]), until the pointer leaves the
//!   entry.

use std::{
    cell::Cell, future::Future, ops::Range, path::PathBuf, pin::Pin, rc::Rc, sync::Arc,
    time::Duration,
};

use fe_buffer::{Anchor, Bias, Buffer, BufferId, Version};
use fe_editor::{
    decorations::DecorationKind,
    diagnostics::{DiagnosticEntry, RelatedInfo},
};
use fe_syntax::{HighlightSpan, Syntax, fence_language, language_by_name};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, BoxShadow, ClipboardItem, Context, Div, Font,
    FontWeight, Global, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Point, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, StyledText,
    Subscription, Task, TextRun, Window, anchored, canvas, deferred, div, font, hsla, point, px,
};
use kiln_l10n::tr;

use crate::{EditorTheme, EditorView};

/// How long the pointer rests on a range before its popover shows.
pub(crate) const HOVER_DELAY: Duration = Duration::from_millis(300);

/// How long a shown popover stays once the pointer has left both it and
/// what it shows: time to travel to it across other text (VS Code's
/// `editor.hover.hidingDelay`).
pub(crate) const HOVER_HIDE_DELAY: Duration = Duration::from_millis(300);

/// The most of the editor's lines the popover's content is tall; more
/// scrolls.
pub(crate) const MAX_POPOVER_ROWS: f32 = 16.;

/// The least room the popover leaves above and below it in the window.
const WINDOW_MARGIN: Pixels = px(8.);

/// GPUI's name for the platform's UI font, which the popover's prose is
/// set in when the host sets no UI family ([`crate::ui_font_family`]).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const UI_FONT: &str = ".SystemUIFont";

/// What a [`HoverProvider`] says about the text at an offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoverContent {
    /// Markdown, shown as text with fenced code in the editor's font.
    pub text: String,
    /// What it's about, in bytes of the version asked about. The popover
    /// stays while the pointer is in it; without one, while it's on the
    /// word asked about.
    pub range: Option<Range<usize>>,
}

/// A [`HoverProvider`]'s answer to come: `None` for nothing to show,
/// whatever the reason. Dropping it should cancel the work.
pub type HoverFuture = Pin<Box<dyn Future<Output = Option<HoverContent>>>>;

/// Where what a hover is about is defined, as its [`HoverProvider`] says:
/// the card's footer shows it, unless it's where the hover is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoverDefinition {
    /// The file it's in; `None` for the buffer asked about.
    pub path: Option<PathBuf>,
    /// The zero-based row it starts on.
    pub row: u32,
}

/// A [`HoverProvider`]'s definition to come: `None` for not known.
pub type DefinitionFuture = Pin<Box<dyn Future<Output = Option<HoverDefinition>>>>;

/// A [`HoverProvider`]'s caller count to come: `None` for none to show (no
/// call hierarchy, say), else how many call what the hover is about, at most
/// [`CALLER_COUNT_CAP`].
pub type CallerCountFuture = Pin<Box<dyn Future<Output = Option<usize>>>>;

/// The most callers a count tells: [`CALLER_COUNT_CAP`] or more shows as
/// "99+".
pub const CALLER_COUNT_CAP: usize = 99;

/// Where a view's hover text comes from, set by its host
/// ([`EditorView::set_hover_provider`]): fe-gpui knows nothing of language
/// servers.
pub trait HoverProvider {
    /// What to show about byte `offset` of `buffer`, at its `version`.
    fn hover(&self, buffer: BufferId, version: Version, offset: usize) -> HoverFuture;

    /// Where what's at byte `offset` of `buffer`, at its `version`, is
    /// defined, for the card's footer: asked once the hover has answered
    /// with something to show. Not known, by default.
    fn definition(&self, buffer: BufferId, version: Version, offset: usize) -> DefinitionFuture {
        let _ = (buffer, version, offset);
        Box::pin(std::future::ready(None))
    }

    /// How many callers what's at byte `offset` of `buffer`, at its
    /// `version`, has, for the card's footer: asked once the hover has
    /// answered with something to show, and filled in when it arrives. None
    /// by default. Dropped (so cancelled) when the pointer leaves.
    fn caller_count(&self, buffer: BufferId, version: Version, offset: usize) -> CallerCountFuture {
        let _ = (buffer, version, offset);
        Box::pin(std::future::ready(None))
    }
}

/// The popover: waiting out the delay, asking, or shown.
#[derive(Default)]
pub(crate) struct HoverPopover {
    current: Option<Current>,
    /// The host's text for the pointer resting on an after-line
    /// decoration's label ([`EditorView::set_after_line_popover`]).
    after_line: Option<(Arc<str>, HoverContent)>,
    /// Where the last frame drew it, in window coordinates; `None` if it
    /// didn't.
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Hides it on `escape` ([`hide_on_escape`]).
    _escape: Option<Subscription>,
}

/// Has `escape` hide `view`'s popover, wherever it's pressed, before any
/// binding takes it: a binding's command (vim's `escape` in insert mode,
/// say) would stop it reaching the view's own key listeners.
pub(crate) fn hide_on_escape(view: &mut EditorView, cx: &mut Context<EditorView>) {
    let this = cx.weak_entity();
    view.popover._escape = Some(cx.intercept_keystrokes(move |event, _, cx| {
        if event.keystroke.key == "escape" {
            this.update(cx, |view, cx| view.close_popover(cx)).ok();
        } else if event.keystroke.key == "c" && event.keystroke.modifiers.secondary() {
            this.update(cx, |view, cx| view.copy_pinned_selection(cx))
                .ok();
        }
    }));
}

struct Current {
    hovered: Hovered,
    /// Its top-left corner, in window coordinates.
    at: Point<Pixels>,
    /// Whether the delay is up (at once, at the caret).
    shown: bool,
    /// The provider's answer, once it has come.
    content: Option<HoverContent>,
    /// `content` as the card lays it out, once drawn: parsed and
    /// highlighted once, not every frame.
    card: Option<Rc<CardText>>,
    /// Where the provider says what it answered about is defined, once it
    /// has said.
    definition: Option<HoverDefinition>,
    /// How many call it, once the provider has said (see
    /// [`HoverProvider::caller_count`]).
    callers: Option<usize>,
    /// Whether the count has been asked for and hasn't come: the footer
    /// holds the room it will take, so the card doesn't move or grow when
    /// it arrives.
    callers_pending: bool,
    /// Whether the count was asked for at all, pending or not.
    callers_asked: bool,
    /// The offset the provider was asked about, until the pointer moves on
    /// within what this covers: what a reply's range must hold.
    requested: usize,
    /// The delay's timer, then the provider's request: dropped with the
    /// popover, which cancels them.
    _task: Task<()>,
    /// Once the pointer has left it and what it shows: the timer that
    /// hides it ([`HOVER_HIDE_DELAY`]).
    hiding: Option<Task<()>>,
    /// Its content's scroll, from the top.
    scroll: ScrollHandle,
    /// Pinned by a second `editor.show_hover`: kept until `escape`, another
    /// `editor.show_hover`, or the caret leaves the line it was pinned on.
    pin: Option<Pinned>,
}

/// A pinned card ([`Current::pin`]).
struct Pinned {
    /// The start of the caret's line when it was pinned: it follows edits.
    line: Anchor,
    /// The part of the card selected, to copy: its text.
    selected: Option<(usize, String)>,
}

/// What the pointer, or the caret, rests on.
#[derive(Clone, Debug, PartialEq)]
struct Hovered {
    /// Where: the offset of the grapheme under the pointer, or of the end
    /// of the row it's just past ([`Under`]).
    under: Under,
    /// The range of each diagnostic there, and what the popover says of
    /// it, untranslated: a locale switch doesn't make it stale.
    items: Vec<(Range<usize>, DiagnosticItem)>,
    /// The word there, to ask the provider about, if there's one.
    word: Option<Range<usize>>,
    /// The buffer version and scroll it was found at: an edit or a scroll
    /// hides it.
    version: Version,
    scroll: Point<Pixels>,
    /// Shown at the caret, by `editor.show_hover`: where the caret was.
    caret: Option<usize>,
    /// The host's text, not the provider's or the diagnostics': shown for
    /// whatever diagnostics come and go.
    host: bool,
    /// The index of the gutter annotation the pointer rests on, whose
    /// popover this is ([`EditorView::hover_annotation`]).
    annotation: Option<usize>,
    /// The index of the inlay hint the pointer rests on, whose tooltip
    /// this is ([`EditorView::hover_inlay_hint`]).
    hint: Option<u64>,
}

impl Hovered {
    /// Whether it's the host's text the pointer found: an after-line
    /// decoration's or a gutter annotation's.
    fn found_by_pointer_for_host(&self) -> bool {
        self.host && self.caret.is_none()
    }
}

/// Where the pointer is, in the text.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Under {
    /// Over the grapheme starting at this offset: the diagnostics holding
    /// it are there, and the empty ones at it.
    Char(usize),
    /// Past the end of a row, at this offset, within the space-wide
    /// squiggle of an empty diagnostic there: only those are.
    RowEnd(usize),
}

impl Under {
    fn offset(self) -> usize {
        match self {
            Self::Char(offset) | Self::RowEnd(offset) => offset,
        }
    }

    /// The range and what's said of each of `entries` here, by start.
    fn items(self, entries: &[DiagnosticEntry<'_>]) -> Vec<(Range<usize>, DiagnosticItem)> {
        let offset = self.offset();
        (entries.iter())
            .filter(|entry| {
                let empty_here = entry.range.is_empty() && entry.range.start == offset;
                match self {
                    Self::Char(_) => empty_here || entry.range.contains(&offset),
                    Self::RowEnd(_) => empty_here,
                }
            })
            .map(|entry| (entry.range.clone(), DiagnosticItem::new(entry)))
            .collect()
    }
}

/// What the popover says of a diagnostic, as data: what [`Hovered`]
/// compares to tell whether the diagnostics where it was found changed, so
/// that switching the locale doesn't. [`DiagnosticItem::text`] says it in
/// the active locale.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DiagnosticItem {
    message: String,
    /// Its related locations, as they've moved.
    related: Vec<RelatedInfo>,
    code: Option<String>,
    code_href: Option<String>,
}

impl DiagnosticItem {
    fn new(entry: &DiagnosticEntry<'_>) -> Self {
        Self {
            message: entry.message.clone(),
            related: entry.related.clone(),
            code: entry.code.clone(),
            code_href: entry.code_href.clone(),
        }
    }

    /// The diagnostic as the popover says it, in the active locale: its
    /// message, then a line per related location, `↳ main.rs:4: first
    /// borrow here` (`line 4` in its own file), then its code and the page
    /// describing it, if it has both.
    fn text(&self) -> String {
        let mut text = self.message.clone();
        for related in &self.related {
            let name = (related.range.is_none())
                .then(|| related.path.as_deref()?.file_name())
                .flatten();
            let at = match name {
                Some(name) => format!("{}:{}", name.to_string_lossy(), related.row + 1),
                None => tr!("gpui-hover-related-line", line = related.row + 1).to_string(),
            };
            let line = tr!("gpui-hover-related", at = at, message = &related.message);
            text.push('\n');
            text.push_str(&line);
        }
        if let (Some(code), Some(href)) = (&self.code, &self.code_href) {
            text.push('\n');
            text.push_str(&tr!("gpui-hover-code", code = code, href = href));
        }
        text
    }
}

/// The diagnostics' text, a line apart, in the active locale.
fn diagnostics_text(items: &[(Range<usize>, DiagnosticItem)]) -> String {
    let texts: Vec<String> = items.iter().map(|(_, item)| item.text()).collect();
    texts.join("\n")
}

/// A part of the popover's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    /// Prose, as written: diagnostic messages, and markdown outside fences.
    Text(String),
    /// A fenced code block's lines.
    Code(String),
}

impl Block {
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::Text(text) | Self::Code(text) => text,
        }
    }
}

impl Current {
    /// Whether the pointer at `hovered` is still on what this shows, or
    /// waits to: the same diagnostics, and the range the provider answered
    /// for, or else the same word. (Moving to another word within the same
    /// diagnostics keeps them shown, and asks about the new word:
    /// [`EditorView::hover_diagnostics`].)
    fn covers(&self, hovered: &Hovered) -> bool {
        let shows = &self.hovered;
        if shows.items != hovered.items
            || shows.version != hovered.version
            || shows.scroll != hovered.scroll
        {
            return false;
        }
        let offset = hovered.under.offset();
        match self
            .content
            .as_ref()
            .and_then(|content| content.range.as_ref())
        {
            Some(range) => range.contains(&offset) || range.start == offset,
            None => shows.word == hovered.word,
        }
    }

    /// What it says: the diagnostics' messages, each with its related
    /// locations and code link, then the provider's text.
    fn blocks(&self) -> Vec<Block> {
        let mut blocks = Vec::new();
        if !self.hovered.items.is_empty() {
            blocks.push(Block::Text(diagnostics_text(&self.hovered.items)));
        }
        if let Some(content) = &self.content {
            blocks.extend(markdown_blocks(&content.text));
        }
        blocks
    }
}

/// Draws Markdown for the hover popover and the completion menu's
/// documentation, which fe-gpui can't do itself: the host sets one that does
/// ([`EditorView::set_markdown_renderer`]). Without one, they're drawn as
/// text with fenced code in the editor's font.
pub trait MarkdownRenderer {
    /// An element showing `markdown`, at most `max_width` wide.
    fn render(
        &self,
        markdown: &str,
        theme: &EditorTheme,
        max_width: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
}

impl EditorView {
    /// Sets what draws hover and completion documentation Markdown, or
    /// none: then it's drawn as text.
    pub fn set_markdown_renderer(&mut self, renderer: Option<Rc<dyn MarkdownRenderer>>) {
        self.markdown_renderer = renderer;
    }

    pub fn markdown_renderer(&self) -> Option<&Rc<dyn MarkdownRenderer>> {
        self.markdown_renderer.as_ref()
    }

    /// Sets where hover text comes from, or none: then the popover holds
    /// only diagnostics.
    /// A popover waiting on the last one's answer goes.
    pub fn set_hover_provider(&mut self, provider: Option<Rc<dyn HoverProvider>>) {
        self.hover_provider = provider;
        self.popover.current = None;
    }

    pub fn hover_provider(&self) -> Option<&Rc<dyn HoverProvider>> {
        self.hover_provider.as_ref()
    }

    /// Where a window position is in the last frame's text, if that frame
    /// is of the current text.
    fn under(&mut self, position: Point<Pixels>) -> Option<Under> {
        let text = self.model.text();
        let layout = self.layout.as_mut()?;
        if layout.version != self.model.snapshot().version() {
            return None;
        }
        match layout.char_at(text, position) {
            Some(offset) => Some(Under::Char(offset)),
            None => layout.row_end_at(position).map(Under::RowEnd),
        }
    }

    /// The word holding the grapheme at `offset`, unless that's whitespace
    /// or there's none.
    fn word_at(&self, offset: usize) -> Option<Range<usize>> {
        let text = self.model.text();
        let end = text.clip_offset(offset + 1, Bias::Right);
        if offset >= text.len() || text.slice_to_string(offset..end).trim().is_empty() {
            return None;
        }
        Some(self.model.word_range_at(offset))
    }

    /// What's at `under` to show, with `word` to ask the provider about:
    /// `None` if there's nothing, no diagnostics and no word (or no
    /// provider to ask).
    fn hovered(
        &self,
        under: Under,
        word: Option<Range<usize>>,
        caret: Option<usize>,
    ) -> Option<Hovered> {
        let hovered = self.hovered_unfiltered(under, word, caret);
        (!hovered.items.is_empty() || hovered.word.is_some()).then_some(hovered)
    }

    /// [`Self::hovered`] for what's there even if it's nothing to show:
    /// to tell whether the popover still covers it.
    fn hovered_unfiltered(
        &self,
        under: Under,
        word: Option<Range<usize>>,
        caret: Option<usize>,
    ) -> Hovered {
        let word = word.filter(|_| self.hover_provider.is_some());
        Hovered {
            under,
            items: under.items(&self.model.diagnostics()),
            word,
            version: self.model.snapshot().version(),
            scroll: self.scroll,
            caret,
            host: false,
            annotation: None,
            hint: None,
        }
    }

    /// The pointer moved to `position` over the text, or off it (`None`):
    /// waits out the delay over a diagnostic or a word, or hides the
    /// popover once the pointer has left what it shows
    /// ([`Self::leave_popover`]).
    pub(crate) fn hover_diagnostics(
        &mut self,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hover_after_line(position, window, cx)
            || self.hover_inlay_hint(position, window, cx)
        {
            return;
        }
        let under = position.and_then(|position| Some((self.under(position)?, position)));
        let raw = under.map(|(under, _)| {
            let word = match under {
                Under::Char(offset) => self.word_at(offset),
                Under::RowEnd(_) => None,
            };
            self.hovered_unfiltered(under, word, None)
        });
        // Still on what the popover shows, or waits to (whitespace within
        // its range included, and a popover from the caret too): it goes
        // on, from where the pointer is now. Not the host's for an
        // after-line decoration or an annotation, which the pointer is off.
        if let (Some(raw), Some(current)) = (&raw, self.popover.current.as_mut())
            && !current.hovered.found_by_pointer_for_host()
            && current.covers(raw)
        {
            current.hovered.under = raw.under;
            current.requested = raw.under.offset();
            current.hiding = None;
            return;
        }
        let found = under.zip(raw).and_then(|((_, position), raw)| {
            (!raw.items.is_empty() || raw.word.is_some()).then_some((raw, position))
        });
        let at_caret =
            (self.popover.current.as_ref()).is_some_and(|current| current.hovered.caret.is_some());
        let Some((hovered, position)) = found else {
            // One shown at the caret stays while the pointer finds nothing.
            if !at_caret {
                self.leave_popover(window, cx);
            }
            return;
        };
        // Onto another word within the same diagnostics: they stay shown
        // where they are, as card 7's popover did, while the new word's
        // wait and request run; the last word's text goes.
        let keep = (self.popover.current.as_ref()).and_then(|current| {
            let shows = &current.hovered;
            (current.shown
                && shows.caret.is_none()
                && !shows.items.is_empty()
                && shows.items == hovered.items
                && shows.version == hovered.version
                && shows.scroll == hovered.scroll)
                .then_some(current.at)
        });
        if let Some(at) = keep {
            self.wait_for_popover(hovered, at, Some(HOVER_DELAY), true, None, cx);
            cx.notify();
            return;
        }
        // The one shown stays a while for the pointer to reach it: what's
        // here waits its turn, as a fresh move once that's up.
        if self.leave_popover(window, cx) {
            return;
        }
        // Below the pointer's row, from the pointer.
        let offset = hovered.under.offset();
        let at = self
            .layout
            .as_mut()
            .and_then(|layout| layout.caret_bounds(offset))
            .map_or(position, |caret| gpui::point(position.x, caret.bottom()));
        self.wait_for_popover(hovered, at, Some(HOVER_DELAY), false, None, cx);
    }

    /// The pointer moved to `position`, or off the text: over the label of
    /// the host's after-line popover, waits out the delay to show it, or
    /// keeps it shown; off it, hides it. Whether the pointer is over the
    /// label.
    fn hover_after_line(
        &mut self,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let shows_after_line = (self.popover.current.as_ref()).is_some_and(|current| {
            current.hovered.found_by_pointer_for_host()
                && current.hovered.annotation.is_none()
                && current.hovered.hint.is_none()
        });
        let Some((position, bottom, content)) = position.and_then(|position| {
            let (label, content) = self.popover.after_line.as_ref()?;
            let layout = self.layout.as_ref()?;
            let placed = (layout.decorations.placed.iter()).find(|placed| {
                matches!(&placed.kind, DecorationKind::AfterLine(shown) if shown == label)
                    && placed.bounds.contains(&position)
            })?;
            Some((position, placed.bounds.bottom(), content.clone()))
        }) else {
            if shows_after_line {
                self.leave_popover(window, cx);
            }
            return false;
        };
        if shows_after_line {
            self.keep_popover();
            return true;
        }
        if self.leave_popover(window, cx) {
            return true;
        }
        let text = self.model.text();
        let Some(layout) = self.layout.as_mut() else {
            return false;
        };
        let offset = layout.offset_at(text, position);
        let hovered = Hovered {
            under: Under::RowEnd(offset),
            items: Vec::new(),
            word: None,
            version: self.model.snapshot().version(),
            scroll: self.scroll,
            caret: None,
            host: true,
            annotation: None,
            hint: None,
        };
        let at = gpui::point(position.x, bottom);
        self.wait_for_popover(hovered, at, Some(HOVER_DELAY), false, Some(content), cx);
        true
    }

    /// The pointer moved to `position`, or off the text: on an inlay hint,
    /// waits out the delay, resolves the hint through the host (once) and
    /// shows its tooltip below it, or keeps that shown; off it, hides it.
    /// Whether the pointer is over a hint.
    fn hover_inlay_hint(
        &mut self,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let shows = (self.popover.current.as_ref()).and_then(|current| current.hovered.hint);
        let found = position.and_then(|position| Some((position, self.inlay_hint_at(position)?)));
        let Some((position, id)) = found else {
            if shows.is_some() {
                self.leave_popover(window, cx);
            }
            return false;
        };
        if shows == Some(id) {
            self.keep_popover();
            return true;
        }
        if self.leave_popover(window, cx) {
            return true;
        }
        let Some(ix) = self.inlay_hint_ix(id) else {
            return false;
        };
        let offset = self.inlay_hints.hints[ix].offset;
        let bottom = (self.inlay_hints_bounds.iter())
            .find(|(found, _)| *found == ix)
            .map_or(position.y, |(_, bounds)| bounds.bottom());
        let hovered = Hovered {
            under: Under::RowEnd(offset),
            items: Vec::new(),
            word: None,
            version: self.model.snapshot().version(),
            scroll: self.scroll,
            caret: None,
            host: true,
            annotation: None,
            hint: Some(id),
        };
        let at = gpui::point(position.x, bottom);
        self.wait_for_popover(hovered, at, Some(HOVER_DELAY), false, None, cx);
        let task = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(HOVER_DELAY).await;
            view.update(cx, |view, cx| {
                let stands = (view.popover.current.as_ref())
                    .is_some_and(|current| view.popover_stands(&current.hovered));
                match view.popover.current.as_mut().filter(|_| stands) {
                    Some(current) => {
                        current.shown = true;
                        cx.notify();
                        // Resolved once per hint, whoever asks first.
                        view.request_inlay_hint(id, false, cx);
                        view.show_hint_tooltip(id, cx);
                    }
                    None => view.popover.current = None,
                }
            })
            .ok();
        });
        if let Some(current) = self.popover.current.as_mut() {
            current._task = task;
        }
        true
    }

    /// Shows hint `id`'s tooltip, if its popover is the one up and the
    /// hint is resolved.
    pub(crate) fn show_hint_tooltip(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(tooltip) = self.inlay_hint_tooltip(id) else {
            return;
        };
        let Some(current) =
            (self.popover.current.as_mut()).filter(|current| current.hovered.hint == Some(id))
        else {
            return;
        };
        current.content = Some(HoverContent {
            text: tooltip,
            range: None,
        });
        current.card = None;
        cx.notify();
    }

    /// Has the pointer resting on an after-line decoration whose text is
    /// `label` show `content`, as the provider's hover shows, or nothing
    /// (`None`), hiding one shown.
    pub fn set_after_line_popover(
        &mut self,
        popover: Option<(Arc<str>, HoverContent)>,
        cx: &mut Context<Self>,
    ) {
        if self.popover.after_line == popover {
            return;
        }
        self.popover.after_line = popover;
        if (self.popover.current.as_ref()).is_some_and(|current| {
            current.hovered.found_by_pointer_for_host()
                && current.hovered.annotation.is_none()
                && current.hovered.hint.is_none()
        }) {
            self.hide_popover(cx);
        }
    }

    /// Whether the popover is a gutter annotation's, shown or waiting.
    pub(crate) fn popover_shows_annotation(&self) -> bool {
        (self.popover.current.as_ref()).is_some_and(|current| current.hovered.annotation.is_some())
    }

    /// The pointer moved to `position` over the annotation column, or off
    /// it (`None`): over an entry's row, waits out the delay to show the
    /// host's popover for it below the row, or keeps the one shown while
    /// the pointer stays on that entry; elsewhere, hides one shown. Whether
    /// the pointer is over an entry.
    pub(crate) fn hover_annotation(
        &mut self,
        position: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let version = self.model.snapshot().version();
        let found = position.and_then(|position| {
            let layout = self
                .layout
                .as_ref()
                .filter(|layout| layout.version == version)?;
            let (row, slot) = layout.annotation_row_at(position)?;
            let ix = self.resolved_annotations()?.entry_at(row)?;
            Some((position, row, slot, ix))
        });
        let shows = (self.popover.current.as_ref()).and_then(|current| current.hovered.annotation);
        let Some((position, row, slot, ix)) = found else {
            if shows.is_some() {
                self.leave_popover(window, cx);
            }
            return false;
        };
        if shows == Some(ix) {
            self.keep_popover();
            return true;
        }
        if self.leave_popover(window, cx) {
            return true;
        }
        let Some(hover) = (self.annotations.as_ref()).map(|shown| shown.annotations.hover.clone())
        else {
            return false;
        };
        // Asked once per entry rested on; with nothing to show, the
        // popover waits unseen, so moves within the entry don't ask again.
        let content = hover(ix, row);
        let hovered = Hovered {
            under: Under::RowEnd(self.model.text().row_start(row)),
            items: Vec::new(),
            word: None,
            version,
            scroll: self.scroll,
            caret: None,
            host: true,
            annotation: Some(ix),
            hint: None,
        };
        let at = gpui::point(position.x, slot.bottom());
        self.wait_for_popover(hovered, at, Some(HOVER_DELAY), false, content, cx);
        true
    }

    /// Shows `content`, the host's, in the popover below the caret, at once,
    /// in place of any shown: as `editor.show_hover` shows the provider's,
    /// and hidden as that is. Nothing for a caret out of the frame.
    pub fn show_popover(&mut self, content: HoverContent, cx: &mut Context<Self>) {
        self.close_popover(cx);
        let caret = self.model.primary_selection().head();
        let hovered = Hovered {
            under: Under::Char(caret),
            items: Vec::new(),
            word: None,
            version: self.model.snapshot().version(),
            scroll: self.scroll,
            caret: Some(caret),
            host: true,
            annotation: None,
            hint: None,
        };
        let Some(at) = (self.layout.as_mut())
            .filter(|layout| layout.version == hovered.version)
            .and_then(|layout| layout.caret_bounds(caret))
            .map(|caret| gpui::point(caret.left(), caret.bottom()))
        else {
            return;
        };
        self.wait_for_popover(hovered, at, None, false, Some(content), cx);
    }

    /// `editor.show_hover`: the popover for what's at the caret, at once.
    pub(crate) fn show_hover(&mut self, cx: &mut Context<Self>) {
        // A second one pins the card it opened, or the pointer's; a third
        // closes it.
        if self.popover_pinned() {
            self.close_popover(cx);
            return;
        }
        if self.popover_blocks().is_some() {
            self.pin_popover(cx);
            return;
        }
        self.close_popover(cx);
        let caret = self.model.primary_selection().head();
        // The word the caret is in, or else the one it's just past the end
        // of, preferring a name to punctuation: `beta|;` asks about `beta`.
        let text = self.model.text();
        let before = (caret > 0)
            .then(|| self.word_at(text.clip_offset(caret - 1, Bias::Left)))
            .flatten()
            .filter(|word| word.end == caret);
        let is_name = |word: &Range<usize>| {
            (text.slice_to_string(word.clone()).chars().next())
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        };
        let word = match (self.word_at(caret), before) {
            (Some(at), Some(before)) if !is_name(&at) && is_name(&before) => Some(before),
            (at, before) => at.or(before),
        };
        let Some(hovered) = self.hovered(Under::Char(caret), word, Some(caret)) else {
            return;
        };
        // Below the caret; nothing for a caret out of the frame, which
        // revealing it would scroll, hiding the popover anyway.
        let Some(at) = (self.layout.as_mut())
            .filter(|layout| layout.version == hovered.version)
            .and_then(|layout| layout.caret_bounds(caret))
            .map(|caret| gpui::point(caret.left(), caret.bottom()))
        else {
            return;
        };
        self.wait_for_popover(hovered, at, None, false, None, cx);
    }

    /// Shows `hovered` at `at` once `delay` is up (or at once, if
    /// `shown`), with the host's `content`, or else asking the provider
    /// about its word, if any.
    fn wait_for_popover(
        &mut self,
        hovered: Hovered,
        at: Point<Pixels>,
        delay: Option<Duration>,
        shown: bool,
        content: Option<HoverContent>,
        cx: &mut Context<Self>,
    ) {
        if self.popover_pinned() {
            return;
        }
        let buffer = self.model.buffer().id();
        // The offset asked about: the pointer's or caret's, in the word.
        let ask = hovered.word.clone().map(|word| {
            let offset = hovered.caret.unwrap_or(hovered.under.offset());
            if word.contains(&offset) {
                offset
            } else {
                word.start
            }
        });
        let hovered_offset = hovered.under.offset();
        let version = hovered.version;
        let provider = self.hover_provider.clone();
        let task = cx.spawn(async move |view, cx| {
            if let Some(delay) = delay {
                cx.background_executor().timer(delay).await;
                let shown = view.update(cx, |view, cx| {
                    // Found on text an edit or a scroll has moved since:
                    // nothing to show, or to ask about.
                    let stands = (view.popover.current.as_ref())
                        .is_some_and(|current| view.popover_stands(&current.hovered));
                    if !stands {
                        view.popover.current = None;
                        return false;
                    }
                    let Some(current) = view.popover.current.as_mut() else {
                        return false;
                    };
                    current.shown = true;
                    cx.notify();
                    true
                });
                if !matches!(shown, Ok(true)) {
                    return;
                }
            }
            let (Some(provider), Some(offset)) = (provider, ask) else {
                return;
            };
            let content = provider.hover(buffer, version, offset).await;
            let answered = view.update(cx, |view, cx| {
                let current_version = view.model.snapshot().version();
                let Some(current) = view.popover.current.as_mut() else {
                    return false;
                };
                // For text the view has moved on from, or nothing: what
                // there was to show stays, if it still stands. One with
                // nothing to show stays too, unseen, so moves within its
                // word don't ask again.
                // One whose range leaves out where the pointer has moved to
                // since is for an older position: the popover goes.
                if let Some(content) = content.filter(|_| current_version == version) {
                    let offset = current.requested;
                    let covers = (content.range.as_ref())
                        .is_none_or(|range| range.contains(&offset) || range.start == offset);
                    if covers {
                        current.content = Some(content);
                        current.callers_pending = true;
                        current.callers_asked = true;
                        cx.notify();
                        return true;
                    } else if current.hovered.items.is_empty() {
                        view.hide_popover(cx);
                    } else {
                        // The diagnostics still stand at the pointer;
                        // only the provider's text was for elsewhere.
                        current.content = None;
                        cx.notify();
                    }
                }
                false
            });
            if !matches!(answered, Ok(true)) {
                return;
            }
            // Then where it's defined and how many call it, for the footer,
            // each filling in as it arrives.
            let mut callers_cx = cx.clone();
            let defined = async {
                let definition = provider.definition(buffer, version, offset).await?;
                view.update(cx, |view, cx| {
                    let current_version = view.model.snapshot().version();
                    if let Some(current) = (view.popover.current.as_mut())
                        .filter(|current| current.content.is_some() && current_version == version)
                    {
                        current.definition = Some(definition);
                        cx.notify();
                    }
                })
                .ok()
            };
            let called = async {
                let count = provider.caller_count(buffer, version, offset).await;
                view.update(&mut callers_cx, |view, cx| {
                    let current_version = view.model.snapshot().version();
                    if let Some(current) = (view.popover.current.as_mut())
                        .filter(|current| current.content.is_some() && current_version == version)
                    {
                        current.callers = count.map(|count| count.min(CALLER_COUNT_CAP));
                        current.callers_pending = false;
                        cx.notify();
                    }
                })
                .ok()
            };
            futures::future::join(defined, called).await;
        });
        self.popover.current = Some(Current {
            hovered,
            at,
            shown: shown || delay.is_none(),
            content,
            card: None,
            definition: None,
            callers: None,
            callers_pending: false,
            callers_asked: false,
            requested: ask.unwrap_or(hovered_offset),
            _task: task,
            hiding: None,
            scroll: ScrollHandle::new(),
            pin: None,
        });
        if delay.is_none() {
            cx.notify();
        }
    }

    /// Whether what the popover was found on still stands: no edit or
    /// scroll since, exactly the same diagnostics where the pointer last
    /// was over them, at the caret, the caret where it was, and on a gutter
    /// annotation, the column still showing.
    fn popover_stands(&self, shown: &Hovered) -> bool {
        if let Some(pin) = (self.popover.current.as_ref()).and_then(|current| current.pin.as_ref())
        {
            return self.pin_stands(pin);
        }
        shown.version == self.model.snapshot().version()
            && shown.scroll == self.scroll
            && (shown.caret).is_none_or(|caret| caret == self.model.primary_selection().head())
            && (shown.host || shown.under.items(&self.model.diagnostics()) == shown.items)
            && (shown.annotation).is_none_or(|_| self.gutter && self.annotations.is_some())
            && (shown.hint).is_none_or(|id| self.inlay_hint_ix(id).is_some())
    }

    /// What the popover says, if it's shown, still stands and has
    /// anything to say yet.
    pub(crate) fn popover_blocks(&self) -> Option<Vec<Block>> {
        let current = self
            .popover
            .current
            .as_ref()
            .filter(|current| current.shown)?;
        if !self.popover_stands(&current.hovered) {
            return None;
        }
        Some(current.blocks()).filter(|blocks| !blocks.is_empty())
    }

    /// The popover's text, if it's shown and still stands: its parts a
    /// line apart, as written.
    pub fn popover_text(&self) -> Option<String> {
        let blocks = self.popover_blocks()?;
        Some(
            blocks
                .iter()
                .map(Block::text)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    /// Hides the popover, and stops waiting for one.
    pub(crate) fn hide_popover(&mut self, cx: &mut Context<Self>) {
        if self.popover_pinned() {
            return;
        }
        self.close_popover(cx);
    }

    /// Whether the popover is pinned.
    fn popover_pinned(&self) -> bool {
        (self.popover.current.as_ref()).is_some_and(|current| current.pin.is_some())
    }

    /// Pins the shown popover where the caret is.
    fn pin_popover(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.model.snapshot();
        let row = self
            .model
            .text()
            .offset_to_point(self.model.primary_selection().head())
            .row;
        let start = self.model.text().row_start(row);
        let line = snapshot.anchor_before(start);
        if let Some(current) = self.popover.current.as_mut() {
            current.hiding = None;
            current.pin = Some(Pinned {
                line,
                selected: None,
            });
            cx.notify();
        }
    }

    /// Whether the caret is on the line a pinned card was pinned on.
    fn pin_stands(&self, pin: &Pinned) -> bool {
        let text = self.model.text();
        let line = self.model.snapshot().resolve(&pin.line);
        let row = |offset: usize| text.offset_to_point(offset.min(text.len())).row;
        row(line) == row(self.model.primary_selection().head())
    }

    /// Copies what's selected in a pinned card.
    fn copy_pinned_selection(&mut self, cx: &mut Context<Self>) {
        let selected = (self.popover.current.as_ref())
            .and_then(|current| current.pin.as_ref()?.selected.as_ref())
            .map(|(_, text)| text.clone());
        if let Some(text) = selected {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Selects part `ix` of a pinned card, whose text is `text`.
    fn select_in_pinned(&mut self, ix: usize, text: String, cx: &mut Context<Self>) {
        let pin = (self.popover.current.as_mut()).and_then(|current| current.pin.as_mut());
        if let Some(pin) = pin {
            pin.selected = Some((ix, text.clone()));
            cx.write_to_primary(ClipboardItem::new_string(text));
            cx.notify();
        }
    }

    /// Hides the popover, pinned or not, and stops waiting for one.
    pub(crate) fn close_popover(&mut self, cx: &mut Context<Self>) {
        let current = self.popover.current.take();
        if current.is_some_and(|current| current.shown) {
            cx.notify();
        }
    }

    /// The pointer has left what the popover shows, for somewhere it
    /// doesn't find it: one it found and shows hides [`HOVER_HIDE_DELAY`]
    /// from the first such move, unless the pointer reaches it or comes
    /// back first ([`Self::keep_popover`]); then the still pointer is taken
    /// as a fresh move. One waiting, or shown at the caret, goes at once.
    /// Whether one is still shown.
    fn leave_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.popover_pinned() {
            return true;
        }
        let Some(current) = (self.popover.current.as_mut()).filter(|current| {
            current.shown && current.hovered.caret.is_none() && !current.blocks().is_empty()
        }) else {
            self.hide_popover(cx);
            return false;
        };
        if current.hiding.is_none() {
            current.hiding = Some(cx.spawn_in(window, async move |view, cx| {
                cx.background_executor().timer(HOVER_HIDE_DELAY).await;
                view.update_in(cx, |view, window, cx| {
                    view.hide_popover(cx);
                    crate::element::pointer_still(view, window, cx);
                })
                .ok();
            }));
        }
        true
    }

    /// The pointer is over the popover, or back on what it shows: it stays.
    pub(crate) fn keep_popover(&mut self) {
        if let Some(current) = &mut self.popover.current {
            current.hiding = None;
        }
    }

    /// Whether `position` is over the popover as the last frame drew it.
    pub(crate) fn pointer_on_popover(&self, position: Point<Pixels>) -> bool {
        (self.popover.current.as_ref()).is_some_and(|current| current.shown)
            && (self.popover.bounds.get()).is_some_and(|bounds| bounds.contains(&position))
    }

    /// The popover, if it's shown and still stands; a stale one is dropped.
    /// Its content scrolls past [`MAX_POPOVER_ROWS`] lines, or past what
    /// fits in `window`.
    pub(crate) fn render_popover(
        &mut self,
        this: gpui::WeakEntity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        // Where the last frame drew it, for a card that mustn't move.
        let drawn_at = self.popover.bounds.get().map(|bounds| bounds.origin);
        self.popover.bounds.set(None);
        #[cfg(test)]
        {
            self.painted_popover = None;
            self.painted_popover_blocks = Vec::new();
        }
        // A stale one goes, and its request with it.
        if (self.popover.current.as_ref())
            .is_some_and(|current| current.shown && !self.popover_stands(&current.hovered))
        {
            self.popover.current = None;
        }
        let blocks = self.popover_blocks()?;
        let footer = self.card_footer();
        let current = self.popover.current.as_mut()?;
        let card = match (&current.content, &current.card) {
            (None, _) => None,
            (Some(content), Some(card)) if card.source == content.text => Some(card.clone()),
            (Some(content), _) => {
                let card = Rc::new(CardText::new(&content.text));
                current.card = Some(card.clone());
                Some(card)
            }
        };
        let current = self.popover.current.as_ref()?;
        // Once it's asked for the callers it stays where it was drawn: the
        // count arriving, or not, doesn't change where the window fits it.
        let at = match drawn_at {
            Some(origin) if current.callers_asked => origin,
            _ => current.at,
        };
        let scroll = current.scroll.clone();
        let line_height =
            (self.layout.as_ref()).map_or(px(20.), |layout| px(layout.frame.line_height));
        let theme = &self.theme;
        let sizes = CardSizes::new(theme);
        // Its padding and border, above and below, and the footer's room.
        let footer_room = match &footer {
            Some(_) => sizes.small * FOOTER_LINE_HEIGHT + FOOTER_GAP * 2. + px(1.),
            None => px(0.),
        };
        let chrome = window.rem_size() * 0.25 * 2. + px(2.) + footer_room;
        let room = window.viewport_size().height - WINDOW_MARGIN * 2. - chrome;
        let max_height = (line_height * MAX_POPOVER_ROWS).min(room).max(px(0.));
        let ui_font = crate::ui_font_family(cx);
        // What it says, part by part, for the view's own tests.
        #[cfg(test)]
        {
            let font_of = |block: &Block| -> SharedString {
                match block {
                    Block::Text(_) => ui_font.clone(),
                    Block::Code(_) => theme.font_family.clone(),
                }
            };
            let texts: Vec<&str> = blocks.iter().map(Block::text).collect();
            self.painted_popover = Some(texts.join("\n"));
            self.painted_popover_blocks = (blocks.iter())
                .map(|block| (block.clone(), font_of(block)))
                .collect();
        }
        #[cfg(not(test))]
        drop(blocks);
        let theme = &self.theme;
        let fonts = CardFonts::new(theme, ui_font.clone());
        let faint = faint_text(theme);
        let mut children: Vec<(String, AnyElement)> = Vec::new();
        // The diagnostics first, as text: only the provider's is Markdown.
        if !current.hovered.items.is_empty() {
            let text = diagnostics_text(&current.hovered.items);
            let runs = vec![run(text.len(), fonts.ui.clone(), theme.foreground)];
            let part = card_text(
                HoverCardPart::Diagnostics,
                text.clone(),
                sizes.body,
                runs,
                cx,
            );
            children.push((text, part.into_any_element()));
        }
        if let Some(card) = &card {
            if let Some(path) = &card.path {
                let runs = vec![run(path.len(), fonts.code.clone(), faint)];
                let text = card_text(
                    HoverCardPart::ModulePath,
                    path.clone(),
                    sizes.small_code,
                    runs,
                    cx,
                );
                children.push((path.clone(), text.into_any_element()));
            }
            if let Some(signature) = &card.signature {
                let runs = signature.runs(theme, &fonts.code);
                let code = signature.code.clone();
                let text = card_text(HoverCardPart::Signature, code.clone(), sizes.code, runs, cx);
                children.push((code, text.into_any_element()));
            }
            let docs = card.docs.as_str();
            if !docs.trim().is_empty() {
                match self.markdown_renderer.clone() {
                    Some(renderer) => {
                        // Its padding and border, left and right.
                        let max_width = POPOVER_WIDTH - window.rem_size() * 0.5 * 2. - px(2.);
                        let drawn = renderer.render(docs, theme, max_width, window, cx);
                        children.push((
                            docs.to_owned(),
                            div()
                                .debug_selector(|| "hover-card-docs".into())
                                .child(drawn)
                                .into_any_element(),
                        ));
                    }
                    None => {
                        for block in markdown_blocks(docs) {
                            let font = match &block {
                                Block::Text(_) => fonts.ui.clone(),
                                Block::Code(_) => fonts.code.clone(),
                            };
                            let text = block.text().to_owned();
                            let runs = vec![run(text.len(), font, theme.foreground)];
                            let part =
                                card_text(HoverCardPart::Docs, text.clone(), sizes.body, runs, cx);
                            children.push((text, part.into_any_element()));
                        }
                    }
                }
            }
        }
        // A pinned card's parts select on a press, to copy.
        let pin = current.pin.as_ref();
        let children: Vec<AnyElement> = (children.into_iter().enumerate())
            .map(|(ix, (text, part))| match pin {
                None => part,
                Some(pin) => {
                    let this = this.clone();
                    let selected = pin.selected.as_ref().is_some_and(|(at, _)| *at == ix);
                    div()
                        .id(("hover-card-part", ix))
                        .when(selected, |part| part.bg(theme.selection))
                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                            let text = text.clone();
                            this.update(cx, |view, cx| view.select_in_pinned(ix, text, cx))
                                .ok();
                        })
                        .child(part)
                        .into_any_element()
                }
            })
            .collect();
        // The prose in the UI family, a renderer's too, inheriting it; code
        // sets its own.
        let body = div()
            .debug_selector(|| "hover-popover-body".into())
            .font_family(ui_font.clone())
            .flex()
            .flex_col()
            .gap_1()
            .children(children);
        let content = div()
            .id("hover-popover-content")
            .debug_selector(|| "hover-popover-content".into())
            .max_h(max_height)
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .child(body);
        // Under the content, whatever its scroll: where it's defined, left,
        // and the key that goes there, right.
        let footer = footer.map(|footer| {
            let mut row = div()
                .debug_selector(|| "hover-card-footer".into())
                .flex()
                .flex_row()
                .justify_between()
                .gap_4()
                .mt(FOOTER_GAP)
                .pt(FOOTER_GAP)
                .border_t_1()
                .border_color(theme.popover_border)
                .line_height(sizes.small * FOOTER_LINE_HEIGHT);
            let small = fonts.ui_small.clone();
            let piece = |text: String, cx: &mut App| {
                let runs = vec![run(text.len(), small.clone(), faint)];
                card_text(HoverCardPart::Footer, text, sizes.small, runs, cx)
            };
            // Where it's defined, then how many call it, which may come late.
            let mut left = div().flex().flex_row().gap_2();
            if let Some(location) = footer.location {
                left = left.child(piece(location, cx));
            }
            if let Some(callers) = footer.callers {
                left = left.child(piece(callers, cx));
            } else if let Some(widest) = footer.callers_room {
                // Not drawn, but laid out.
                let runs = vec![run(widest.len(), small.clone(), faint)];
                left = left.child(
                    div()
                        .invisible()
                        .text_size(sizes.small)
                        .child(StyledText::new(widest).with_runs(runs)),
                );
            }
            row = row.child(left);
            if let Some(hint) = footer.hint {
                row = row.child(piece(hint, cx));
            }
            row
        });
        // It takes the pointer: the text under it sees no moves, clicks or
        // wheel. A floating surface: raised, a hairline round it, the
        // design's menu radius and float shadow.
        let popover = div()
            .id("hover-popover")
            .debug_selector(|| "hover-popover".into())
            .occlude()
            .max_w(POPOVER_WIDTH)
            .px_2()
            .py_1()
            .bg(theme.popover_background)
            .border_1()
            .border_color(theme.popover_border)
            .rounded(CARD_RADIUS)
            .shadow(vec![float_shadow(theme)])
            .text_color(theme.foreground)
            .text_size(theme.font_size)
            .child(content)
            .children(footer);
        let bounds = self.popover.bounds.clone();
        let drawn = div()
            .on_children_prepainted(move |drawn, _, _| bounds.set(drawn.first().copied()))
            .child(popover);
        Some(
            anchored()
                .position(at)
                .snap_to_window()
                .child(drawn)
                .into_any_element(),
        )
    }

    /// The card's footer, for the provider's answer: where it says what the
    /// hover is about is defined, unless that's where the hover is, and
    /// `gd`, vim's key to go there, while vim is on. `None` with neither.
    fn card_footer(&self) -> Option<CardFooter> {
        let current = (self.popover.current.as_ref())
            .filter(|current| !current.hovered.host && current.content.is_some())?;
        let text = self.model.text();
        let here = current.requested.min(text.len());
        let row = text.offset_to_point(here).row;
        let own = self.model.path();
        let location = (current.definition.as_ref())
            .filter(|definition| {
                let same_file = (definition.path.as_deref())
                    .is_none_or(|path| own.is_some_and(|own| own == path));
                !(same_file && definition.row == row)
            })
            .map(|definition| {
                let path = definition.path.as_deref().or(own);
                match path.and_then(|path| path.file_name()) {
                    Some(name) => format!("{}:{}", name.to_string_lossy(), definition.row + 1),
                    None => tr!("gpui-hover-related-line", line = definition.row + 1).to_string(),
                }
            });
        let hint = match (current.pin.is_some(), self.vim.is_some()) {
            (true, _) => Some(tr!("gpui-hover-pinned").to_string()),
            (false, true) => {
                Some(tr!("gpui-hover-go-to-definition", keys = VIM_DEFINITION_KEYS).to_string())
            }
            (false, false) => None,
        };
        let callers = current.callers.map(|count| {
            let shown = if count >= CALLER_COUNT_CAP {
                format!("{CALLER_COUNT_CAP}+")
            } else {
                count.to_string()
            };
            let plural = if count >= CALLER_COUNT_CAP { 2 } else { count };
            tr!("gpui-hover-callers", count = plural, shown = shown).to_string()
        });
        // While it's coming, room for the widest it can be.
        let callers_room = (callers.is_none() && current.callers_pending)
            .then(|| tr!("gpui-hover-callers", count = 2, shown = "99+").to_string());
        (location.is_some() || callers.is_some() || callers_room.is_some() || hint.is_some())
            .then_some(CardFooter {
                location,
                callers,
                callers_room,
                hint,
            })
    }
}

/// The card's widest.
const POPOVER_WIDTH: Pixels = px(560.);

/// The card's corner radius: the design's for menus and cards.
pub(crate) const CARD_RADIUS: Pixels = px(6.);

/// The design's float shadow: 0 12 32, at 55% black on a dark surface and
/// 14% on a light one (kiln-ui's `SHADOW_FLOAT`, which fe-gpui doesn't
/// depend on).
const SHADOW_Y: Pixels = px(12.);
const SHADOW_BLUR: Pixels = px(32.);
const SHADOW_DARK: f32 = 0.55;
const SHADOW_LIGHT: f32 = 0.14;

/// The footer's line height, as a multiple of its text size: the design's
/// UI small, 11 / 14.
const FOOTER_LINE_HEIGHT: f32 = 14. / 11.;

/// Above the footer's hairline, and between it and the footer's text.
const FOOTER_GAP: Pixels = px(4.);

/// Vim's keys to go to a definition, as the footer names them.
const VIM_DEFINITION_KEYS: &str = "gd";

/// The float shadow under a card on `theme`'s popover surface.
pub(crate) fn float_shadow(theme: &EditorTheme) -> BoxShadow {
    let alpha = match theme.popover_background.l > 0.5 {
        true => SHADOW_LIGHT,
        false => SHADOW_DARK,
    };
    BoxShadow {
        color: hsla(0., 0., 0., alpha),
        offset: point(px(0.), SHADOW_Y),
        blur_radius: SHADOW_BLUR,
        spread_radius: px(0.),
        inset: false,
    }
}

/// The card's faint text, its module path and footer: the theme's line
/// numbers' colour, which is the design's faint text.
fn faint_text(theme: &EditorTheme) -> Hsla {
    theme.gutter_foreground
}

/// The card's text sizes, from the editor's: at its default 13, the
/// design's code (13), code small (12), UI body (13) and UI small (11).
struct CardSizes {
    code: Pixels,
    small_code: Pixels,
    body: Pixels,
    small: Pixels,
}

impl CardSizes {
    fn new(theme: &EditorTheme) -> Self {
        let size = theme.font_size;
        Self {
            code: size,
            small_code: size - px(1.),
            body: size,
            small: size - px(2.),
        }
    }
}

/// The card's fonts: the editor's for code, the UI's for prose, and the
/// UI's at UI small's weight for the footer.
struct CardFonts {
    code: Font,
    ui: Font,
    ui_small: Font,
}

impl CardFonts {
    fn new(theme: &EditorTheme, ui: SharedString) -> Self {
        let mut code = font(theme.font_family.clone());
        code.features = theme.font_features.clone();
        let ui_small = Font {
            weight: FontWeight::MEDIUM,
            ..font(ui.clone())
        };
        Self {
            code,
            ui: font(ui),
            ui_small,
        }
    }
}

/// The footer's two sides.
struct CardFooter {
    location: Option<String>,
    /// "3 callers".
    callers: Option<String>,
    /// The widest the count can read, to hold its room while it's coming.
    callers_room: Option<String>,
    hint: Option<String>,
}

fn run(len: usize, font: Font, color: Hsla) -> TextRun {
    TextRun {
        len,
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

/// `text` in `runs` at `size`, as part `part` of the card, recorded if
/// [`PaintedHoverCard`] is recording.
fn card_text(
    part: HoverCardPart,
    text: String,
    size: Pixels,
    runs: Vec<TextRun>,
    cx: &mut App,
) -> Div {
    PaintedHoverCard::record(cx, part, &text, size, &runs);
    div()
        .debug_selector(move || part.selector().into())
        .text_size(size)
        .child(StyledText::new(text).with_runs(runs))
}

/// A part of the hover card ([`PaintedHoverCard`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverCardPart {
    /// The messages of the diagnostics there, above the rest.
    Diagnostics,
    /// The module path, above the signature.
    ModulePath,
    /// The signature, in its syntax colours.
    Signature,
    /// The prose docs, when no [`MarkdownRenderer`] draws them.
    Docs,
    /// The footer: where it's defined, and the key to go there.
    Footer,
}

impl HoverCardPart {
    /// Its `debug_selector`.
    fn selector(self) -> &'static str {
        match self {
            Self::Diagnostics => "hover-card-diagnostics",
            Self::ModulePath => "hover-card-path",
            Self::Signature => "hover-card-signature",
            Self::Docs => "hover-card-docs",
            Self::Footer => "hover-card-footer",
        }
    }
}

/// A text the hover card drew.
#[derive(Clone, Debug, PartialEq)]
pub struct HoverCardText {
    pub part: HoverCardPart,
    pub text: String,
    pub font_size: Pixels,
    /// Its font and colour, by byte range of `text`, covering it.
    pub runs: Vec<TextRun>,
}

/// The texts of the hover card as each frame builds it, part by part, with
/// their fonts, sizes and colours: what a test can't read back from the
/// glyphs. Nothing is recorded, and the card does nothing extra, unless
/// the Global is installed with [`PaintedHoverCard::set`]. A frame records
/// every card text it builds, so [`PaintedHoverCard::take`] after the draw
/// you mean.
#[derive(Default)]
pub struct PaintedHoverCard(Vec<HoverCardText>);

impl Global for PaintedHoverCard {}

impl PaintedHoverCard {
    /// Starts recording, dropping anything recorded before.
    pub fn set(cx: &mut App) {
        cx.set_global(Self::default());
    }

    /// What has been recorded since the last take, in build order; empty
    /// if not recording.
    pub fn take(cx: &mut App) -> Vec<HoverCardText> {
        match cx.try_global::<Self>() {
            Some(_) => std::mem::take(&mut cx.global_mut::<Self>().0),
            None => Vec::new(),
        }
    }

    /// Stops recording.
    pub fn clear(cx: &mut App) {
        if cx.has_global::<Self>() {
            cx.remove_global::<Self>();
        }
    }

    fn record(cx: &mut App, part: HoverCardPart, text: &str, font_size: Pixels, runs: &[TextRun]) {
        if cx.has_global::<Self>() {
            cx.global_mut::<Self>().0.push(HoverCardText {
                part,
                text: text.to_owned(),
                font_size,
                runs: runs.to_vec(),
            });
        }
    }
}

/// How long a signature's code may take to parse: it's parsed once per
/// answer, on the UI thread, and is a line or two.
const SIGNATURE_PARSE_BUDGET: Duration = Duration::from_millis(50);

/// How many times highlighting is retried for injected languages compiled
/// on the way, as fe-markdown's fences are.
const HIGHLIGHT_ATTEMPTS: usize = 3;

/// A hover's text as the card lays it out (`docs/design/design-system.md`,
/// Hover card): the leading fences are its module path and signature, as
/// rust-analyzer sends them, and the rest is its docs.
#[derive(Debug)]
pub(crate) struct CardText {
    /// What it was laid out from.
    source: String,
    /// The module path above the signature, `core::option::Option`: a
    /// leading fence of one word followed by another fence, or a fence's
    /// first line when it's one word and a blank line parts it from the
    /// rest.
    pub(crate) path: Option<String>,
    /// The signature: the leading fence's code, highlighted as its info
    /// string's language.
    pub(crate) signature: Option<Signature>,
    /// The rest, Markdown, as written: all of it without a leading fence.
    pub(crate) docs: String,
}

#[derive(Debug)]
pub(crate) struct Signature {
    pub(crate) code: String,
    highlights: Vec<HighlightSpan>,
}

/// A leading fence of a hover's text.
struct Fence {
    info: String,
    code: String,
    /// Where the text after it starts.
    end: usize,
}

impl CardText {
    pub(crate) fn new(markdown: &str) -> Self {
        let fences = leading_fences(markdown);
        // One word, as a path is: no whitespace.
        let one_word = |code: &str| !code.is_empty() && !code.contains(char::is_whitespace);
        let (path, signature, rest) = match fences.as_slice() {
            [first, second, ..] if one_word(first.code.trim_matches('\n')) => (
                Some(first.code.trim_matches('\n').to_owned()),
                Some((second.info.as_str(), second.code.clone())),
                second.end,
            ),
            [first, ..] => {
                let code = first.code.trim_matches('\n');
                match code.split_once("\n\n") {
                    Some((path, signature)) if one_word(path) && !signature.trim().is_empty() => (
                        Some(path.to_owned()),
                        Some((first.info.as_str(), signature.trim_matches('\n').to_owned())),
                        first.end,
                    ),
                    _ => (
                        None,
                        Some((first.info.as_str(), code.to_owned())),
                        first.end,
                    ),
                }
            }
            [] => (None, None, 0),
        };
        Self {
            source: markdown.to_owned(),
            path,
            signature: (signature.filter(|(_, code)| !code.trim().is_empty())).map(
                |(info, code)| Signature {
                    highlights: highlight(info, &code),
                    code,
                },
            ),
            docs: markdown[rest..].to_owned(),
        }
    }
}

impl Signature {
    /// Runs covering its code: each highlight's span in the theme's colour
    /// for it, the rest in the foreground, all in `code`.
    fn runs(&self, theme: &EditorTheme, code: &Font) -> Vec<TextRun> {
        let text = &self.code;
        let mut runs = Vec::new();
        let mut at = 0;
        let mut push = |len: usize, color: Hsla| {
            if len > 0 {
                runs.push(run(len, code.clone(), color));
            }
        };
        for span in &self.highlights {
            let start = span.range.start.max(at).min(text.len());
            let end = span.range.end.min(text.len());
            if start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                continue;
            }
            push(start - at, theme.foreground);
            let color = theme
                .syntax_color(span.highlight)
                .unwrap_or(theme.foreground);
            push(end - start, color);
            at = end;
        }
        push(text.len() - at, theme.foreground);
        runs
    }
}

/// The highlights of `code` in the language a fence's `info` names; none
/// for an unknown or absent one.
fn highlight(info: &str, code: &str) -> Vec<HighlightSpan> {
    let Some(language) = fence_language(info) else {
        return Vec::new();
    };
    let buffer = Buffer::new(code);
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    syntax.parse_within(SIGNATURE_PARSE_BUDGET);
    let mut spans = Vec::new();
    for _ in 0..HIGHLIGHT_ATTEMPTS {
        spans = syntax.highlights(0..code.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            break;
        }
        for name in missing {
            language_by_name(name);
        }
    }
    spans
}

/// The fenced code blocks `markdown` starts with, blank lines apart, at
/// most two: what the card's header can be made of. A fence never closed
/// runs to the end.
fn leading_fences(markdown: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    // The open fence's char, length and info, and its lines so far.
    let mut open: Option<(char, usize, String, Vec<&str>)> = None;
    let mut at = 0;
    for line in markdown.split_inclusive('\n') {
        at += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        match &mut open {
            Some((ch, len, info, lines)) => match fence(text) {
                Some((close, close_len, rest))
                    if close == *ch && close_len >= *len && rest.trim().is_empty() =>
                {
                    fences.push(Fence {
                        info: std::mem::take(info),
                        code: lines.join("\n"),
                        end: at,
                    });
                    open = None;
                    if fences.len() == 2 {
                        break;
                    }
                }
                _ => lines.push(text),
            },
            None if text.trim().is_empty() => {}
            None => match fence(text) {
                Some((ch, len, info)) if ch == '~' || !info.contains('`') => {
                    open = Some((ch, len, info.trim().to_owned(), Vec::new()));
                }
                _ => break,
            },
        }
    }
    if let Some((_, _, info, lines)) = open {
        fences.push(Fence {
            info,
            code: lines.join("\n"),
            end: markdown.len(),
        });
    }
    fences
}

/// `markdown` as text and fenced code blocks (CommonMark's fences: three or
/// more backticks or tildes, indented at most three spaces; one never
/// closed runs to the end). Blank lines around each are dropped.
pub(crate) fn markdown_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut text: Vec<&str> = Vec::new();
    // The open fence's char and length, and its lines so far.
    let mut code: Option<(char, usize, Vec<&str>)> = None;
    let flush_text = |text: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        let joined = text.join("\n");
        let trimmed = joined.trim_matches('\n');
        if !trimmed.trim().is_empty() {
            blocks.push(Block::Text(trimmed.to_owned()));
        }
        text.clear();
    };
    for line in markdown.lines() {
        match &mut code {
            Some((ch, len, lines)) => match fence(line) {
                Some((close, close_len, info))
                    if close == *ch && close_len >= *len && info.trim().is_empty() =>
                {
                    blocks.push(Block::Code(lines.join("\n")));
                    code = None;
                }
                _ => lines.push(line),
            },
            None => match fence(line) {
                Some((ch, len, info)) if ch == '~' || !info.contains('`') => {
                    flush_text(&mut text, &mut blocks);
                    code = Some((ch, len, Vec::new()));
                }
                _ => text.push(line),
            },
        }
    }
    match code {
        Some((_, _, lines)) => blocks.push(Block::Code(lines.join("\n"))),
        None => flush_text(&mut text, &mut blocks),
    }
    blocks
}

/// A fence line's char, length and what follows it.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let ch = rest.chars().next().filter(|&ch| ch == '`' || ch == '~')?;
    let len = rest.len() - rest.trim_start_matches(ch).len();
    (len >= 3).then(|| (ch, len, &rest[len..]))
}

/// Where the view's popover is drawn: over everything else in the window,
/// and built only once the frame's layout has settled the scroll, which the
/// editor's element does in its prepaint, after the view rendered. A
/// popover a scroll made stale is never drawn.
pub(crate) fn popover_layer(cx: &mut Context<EditorView>) -> impl IntoElement {
    let view = cx.entity().downgrade();
    let layer = canvas(
        move |_, window, cx| {
            let this = view.clone();
            let mut popover = view
                .update(cx, |view, cx| view.render_popover(this, window, cx))
                .ok()??;
            popover.prepaint_as_root(
                Point::default(),
                window.viewport_size().map(AvailableSpace::Definite),
                window,
                cx,
            );
            Some(popover)
        },
        |_, popover, window, cx| {
            if let Some(mut popover) = popover {
                popover.paint(window, cx);
            }
        },
    );
    deferred(layer.absolute())
}

#[cfg(test)]
mod host_popover_tests {
    //! The host's popovers: over an after-line decoration's text, and at
    //! the caret.

    use std::{cell::RefCell, ops::Range, rc::Rc, sync::Arc};

    use fe_buffer::{Buffer, BufferSnapshot, ResolvedSelection};
    use fe_editor::{
        EditorModel,
        decorations::{Decoration, DecorationKind, DecorationProvider, Tint},
    };
    use gpui::{
        Entity, Focusable as _, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point,
        px,
    };

    use super::HOVER_DELAY;
    use crate::{EditorTheme, EditorView, HoverContent, HoverFuture, HoverProvider, init};

    const TEXT: &str = "one\ntwo\nthree\n";

    /// `blame` after row 1.
    struct Note;

    impl DecorationProvider for Note {
        fn decorations(
            &self,
            snapshot: &BufferSnapshot,
            _: Range<usize>,
            out: &mut Vec<Decoration>,
        ) {
            let start = snapshot.text().row_start(1);
            let kind = DecorationKind::AfterLine("blame".into());
            out.push(Decoration::new(start..start + 3, kind, Tint::Muted));
        }
    }

    fn content(text: &str) -> HoverContent {
        HoverContent {
            text: text.into(),
            range: None,
        }
    }

    fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(init);
        let (view, cx) = cx.add_window_view(|_, cx| {
            let mut model = EditorModel::new(Buffer::new(TEXT), None);
            model.set_selections(vec![ResolvedSelection {
                range: 0..0,
                reversed: false,
            }]);
            model.add_decoration_provider(Rc::new(RefCell::new(Note)));
            EditorView::new(model, EditorTheme::default_dark(cx), cx)
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&view.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (view, cx)
    }

    /// The middle of the last frame's `blame` text.
    fn over_note(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point<Pixels> {
        view.read_with(cx, |view, _| {
            let layout = view.layout.as_ref().expect("drawn");
            let placed = (layout.decorations.placed.iter())
                .find(|placed| matches!(placed.kind, DecorationKind::AfterLine(_)))
                .expect("the note is drawn");
            placed.bounds.center()
        })
    }

    fn rest(position: Point<Pixels>, cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(position, None, Modifiers::none());
        cx.run_until_parked();
        cx.executor().advance_clock(HOVER_DELAY);
        cx.run_until_parked();
    }

    fn shown(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| view.popover_text())
    }

    #[gpui::test]
    fn over_after_line_text(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        let note = over_note(&view, cx);

        // Nothing set for it: nothing.
        rest(note, cx);
        assert_eq!(shown(&view, cx), None);

        // Set for another text: nothing.
        let other = Some((Arc::from("other"), content("no")));
        view.update(cx, |view, cx| view.set_after_line_popover(other, cx));
        rest(note, cx);
        assert_eq!(shown(&view, cx), None);

        let blame = Some((Arc::from("blame"), content("**Ann** ann@example.com")));
        view.update(cx, |view, cx| view.set_after_line_popover(blame, cx));
        rest(note, cx);
        assert_eq!(shown(&view, cx).as_deref(), Some("**Ann** ann@example.com"));

        // Moving within it keeps it; off it, it goes.
        rest(note + point(px(1.), px(0.)), cx);
        assert_eq!(shown(&view, cx).as_deref(), Some("**Ann** ann@example.com"));
        rest(note + point(px(0.), px(200.)), cx);
        assert_eq!(shown(&view, cx), None);

        // Unset while shown, it goes.
        rest(note, cx);
        assert!(shown(&view, cx).is_some());
        view.update(cx, |view, cx| view.set_after_line_popover(None, cx));
        assert_eq!(shown(&view, cx), None);
    }

    /// A provider that counts its questions and answers nothing, at once or
    /// never.
    struct Silent {
        asked: Rc<std::cell::Cell<usize>>,
        never: bool,
    }

    impl HoverProvider for Silent {
        fn hover(&self, _: fe_buffer::BufferId, _: fe_buffer::Version, _: usize) -> HoverFuture {
            self.asked.set(self.asked.get() + 1);
            match self.never {
                true => Box::pin(std::future::pending()),
                false => Box::pin(async { None }),
            }
        }
    }

    /// Leaving a word whose popover isn't visible, while the provider is
    /// still working or after it answered nothing, drops it at once: the
    /// next word's request goes out after the plain delay.
    fn leaving_an_invisible_popover(never: bool, cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        let asked = Rc::new(std::cell::Cell::new(0));
        view.update(cx, |view, _| {
            let asked = asked.clone();
            view.set_hover_provider(Some(Rc::new(Silent { asked, never })));
        });
        let at = |offset: usize, cx: &mut VisualTestContext| {
            view.update(cx, |view, _| {
                let caret = view.layout.as_mut().unwrap().caret_bounds(offset).unwrap();
                caret.center()
            })
        };
        let one = at(1, cx);
        rest(one, cx);
        assert_eq!(asked.get(), 1);
        assert_eq!(shown(&view, cx), None);

        let two = at(5, cx);
        rest(two, cx);
        assert_eq!(asked.get(), 2);
    }

    #[gpui::test]
    fn leaving_during_a_pending_request(cx: &mut TestAppContext) {
        leaving_an_invisible_popover(true, cx);
    }

    #[gpui::test]
    fn leaving_after_a_none_answer(cx: &mut TestAppContext) {
        leaving_an_invisible_popover(false, cx);
    }

    #[gpui::test]
    fn at_the_caret(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        view.update(cx, |view, cx| view.show_popover(content("Hi\n\nthere"), cx));
        assert_eq!(shown(&view, cx).as_deref(), Some("Hi\n\nthere"));
        cx.run_until_parked();
        assert_eq!(shown(&view, cx).as_deref(), Some("Hi\n\nthere"));

        // The caret moving hides it.
        view.update(cx, |view, cx| view.select_and_reveal(4..4, cx));
        assert_eq!(shown(&view, cx), None);
    }
}
