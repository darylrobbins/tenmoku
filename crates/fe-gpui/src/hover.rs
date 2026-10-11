//! The editor's hover popover: what the text under a still pointer says,
//! shown after [`HOVER_DELAY`] beside it, or at the caret on
//! `editor.show_hover` (docs/decisions/lsp-client.md §7). It holds the
//! message of each diagnostic there, then what the view's
//! [`HoverProvider`] (a language server's hover, say) says about the word
//! there, once it answers.
//!
//! - **Asking.** The provider is asked once the delay is up, once per word
//!   rested on: moves within the word don't ask again. Leaving the word
//!   drops the request, which cancels it.
//! - **Text.** The provider's is markdown, shown as text: fenced code
//!   blocks in the editor's font, the rest as written, in the UI font.
//!   There's no markdown renderer.
//! - **Hiding.** Moving off what it shows (the word asked about, or the
//!   range the provider answered for), leaving the window, scrolling (the
//!   wheel's, or the layout's to reveal the caret), an edit or `escape`
//!   hides it, and so does any change to the diagnostics where it was
//!   found. One shown at the caret goes when the caret moves, and stays
//!   while the pointer moves over nothing to show. An answer for a buffer
//!   version the view has moved on from is dropped.

use std::{future::Future, ops::Range, pin::Pin, rc::Rc, time::Duration};

use fe_buffer::{Bias, BufferId, Version};
use fe_editor::diagnostics::DiagnosticEntry;
use gpui::{
    AnyElement, AvailableSpace, Context, IntoElement, ParentElement as _, Pixels, Point,
    SharedString, Styled as _, Subscription, Task, anchored, canvas, deferred, div, px,
};

use crate::EditorView;

/// How long the pointer rests on a range before its popover shows.
pub(crate) const HOVER_DELAY: Duration = Duration::from_millis(300);

/// GPUI's name for the platform's UI font, which the popover's prose is
/// set in.
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

/// Where a view's hover text comes from, set by its host
/// ([`EditorView::set_hover_provider`]): fe-gpui knows nothing of language
/// servers.
pub trait HoverProvider {
    /// What to show about byte `offset` of `buffer`, at its `version`.
    fn hover(&self, buffer: BufferId, version: Version, offset: usize) -> HoverFuture;
}

/// The popover: waiting out the delay, asking, or shown.
#[derive(Default)]
pub(crate) struct HoverPopover {
    current: Option<Current>,
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
            this.update(cx, |view, cx| view.hide_popover(cx)).ok();
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
    /// The delay's timer, then the provider's request: dropped with the
    /// popover, which cancels them.
    _task: Task<()>,
}

/// What the pointer, or the caret, rests on.
#[derive(Clone, Debug, PartialEq)]
struct Hovered {
    /// Where: the offset of the grapheme under the pointer, or of the end
    /// of the row it's just past ([`Under`]).
    under: Under,
    /// The range and message of each diagnostic there.
    items: Vec<(Range<usize>, String)>,
    /// The word there, to ask the provider about, if there's one.
    word: Option<Range<usize>>,
    /// The buffer version and scroll it was found at: an edit or a scroll
    /// hides it.
    version: Version,
    scroll: Point<Pixels>,
    /// Shown at the caret, by `editor.show_hover`: where the caret was.
    caret: Option<usize>,
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

    /// The range and message of each of `entries` here, by start.
    fn items(self, entries: &[DiagnosticEntry<'_>]) -> Vec<(Range<usize>, String)> {
        let offset = self.offset();
        (entries.iter())
            .filter(|entry| {
                let empty_here = entry.range.is_empty() && entry.range.start == offset;
                match self {
                    Self::Char(_) => empty_here || entry.range.contains(&offset),
                    Self::RowEnd(_) => empty_here,
                }
            })
            .map(|entry| (entry.range.clone(), entry.message.clone()))
            .collect()
    }
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

    /// What it says: the diagnostics' messages, a line each, then the
    /// provider's text.
    fn blocks(&self) -> Vec<Block> {
        let mut blocks = Vec::new();
        if !self.hovered.items.is_empty() {
            let messages: Vec<&str> = (self.hovered.items.iter())
                .map(|(_, message)| message.as_str())
                .collect();
            blocks.push(Block::Text(messages.join("\n")));
        }
        if let Some(content) = &self.content {
            blocks.extend(markdown_blocks(&content.text));
        }
        blocks
    }
}

impl EditorView {
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
        }
    }

    /// The pointer moved to `position` over the text, or off it (`None`):
    /// waits out the delay over a diagnostic or a word, or hides the
    /// popover once the pointer has left what it shows.
    pub(crate) fn hover_diagnostics(
        &mut self,
        position: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
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
        // on, from where the pointer is now.
        if let (Some(raw), Some(current)) = (&raw, self.popover.current.as_mut())
            && current.covers(raw)
        {
            current.hovered.under = raw.under;
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
                self.hide_popover(cx);
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
            self.wait_for_popover(hovered, at, Some(HOVER_DELAY), true, cx);
            cx.notify();
            return;
        }
        self.hide_popover(cx);
        // Below the pointer's row, from the pointer.
        let offset = hovered.under.offset();
        let at = self
            .layout
            .as_mut()
            .and_then(|layout| layout.caret_bounds(offset))
            .map_or(position, |caret| gpui::point(position.x, caret.bottom()));
        self.wait_for_popover(hovered, at, Some(HOVER_DELAY), false, cx);
    }

    /// `editor.show_hover`: the popover for what's at the caret, at once.
    pub(crate) fn show_hover(&mut self, cx: &mut Context<Self>) {
        self.hide_popover(cx);
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
        self.wait_for_popover(hovered, at, None, false, cx);
    }

    /// Shows `hovered` at `at` once `delay` is up (or at once, if
    /// `shown`), asking the provider about its word, if any.
    fn wait_for_popover(
        &mut self,
        hovered: Hovered,
        at: Point<Pixels>,
        delay: Option<Duration>,
        shown: bool,
        cx: &mut Context<Self>,
    ) {
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
            view.update(cx, |view, cx| {
                let current_version = view.model.snapshot().version();
                let Some(current) = view.popover.current.as_mut() else {
                    return;
                };
                // For text the view has moved on from, or nothing: what
                // there was to show stays, if it still stands. One with
                // nothing to show stays too, unseen, so moves within its
                // word don't ask again.
                // One whose range leaves out where the pointer has moved to
                // since is for an older position: the popover goes.
                if let Some(content) = content.filter(|_| current_version == version) {
                    let offset = current.hovered.under.offset();
                    let covers = (content.range.as_ref())
                        .is_none_or(|range| range.contains(&offset) || range.start == offset);
                    if covers {
                        current.content = Some(content);
                        cx.notify();
                    } else {
                        view.hide_popover(cx);
                    }
                }
            })
            .ok();
        });
        self.popover.current = Some(Current {
            hovered,
            at,
            shown: shown || delay.is_none(),
            content: None,
            _task: task,
        });
        if delay.is_none() {
            cx.notify();
        }
    }

    /// Whether what the popover was found on still stands: no edit or
    /// scroll since, exactly the same diagnostics where the pointer last
    /// was over them, and, at the caret, the caret where it was.
    fn popover_stands(&self, shown: &Hovered) -> bool {
        shown.version == self.model.snapshot().version()
            && shown.scroll == self.scroll
            && (shown.caret).is_none_or(|caret| caret == self.model.primary_selection().head())
            && shown.under.items(&self.model.diagnostics()) == shown.items
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
        let current = self.popover.current.take();
        if current.is_some_and(|current| current.shown) {
            cx.notify();
        }
    }

    /// The popover, if it's shown and still stands; a stale one is dropped.
    pub(crate) fn render_popover(&mut self) -> Option<AnyElement> {
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
        let at = self.popover.current.as_ref()?.at;
        let theme = &self.theme;
        let font = |block: &Block| -> SharedString {
            match block {
                Block::Text(_) => UI_FONT.into(),
                Block::Code(_) => theme.font_family.clone(),
            }
        };
        #[cfg(test)]
        {
            let texts: Vec<&str> = blocks.iter().map(Block::text).collect();
            self.painted_popover = Some(texts.join("\n"));
            self.painted_popover_blocks = (blocks.iter())
                .map(|block| (block.clone(), font(block)))
                .collect();
        }
        let theme = &self.theme;
        let children = blocks.iter().map(|block| {
            div()
                .font_family(font(block))
                .child(SharedString::from(block.text().to_owned()))
        });
        let popover = div()
            .max_w(px(560.))
            .px_2()
            .py_1()
            .flex()
            .flex_col()
            .gap_1()
            .bg(theme.popover_background)
            .border_1()
            .border_color(theme.popover_border)
            .rounded_sm()
            .text_color(theme.foreground)
            .text_size(theme.font_size)
            .children(children);
        Some(
            anchored()
                .position(at)
                .snap_to_window()
                .child(popover)
                .into_any_element(),
        )
    }
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
            let mut popover = view.update(cx, |view, _| view.render_popover()).ok()??;
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
