//! The editor's hover popover: what the text under a still pointer says,
//! shown after [`HOVER_DELAY`] as plain text beside it. For now that's the
//! message of each diagnostic there (docs/decisions/lsp-client.md §7);
//! a language server's hover is to share the surface.
//!
//! Moving off the hovered range, leaving the window, scrolling (the
//! wheel's, or the layout's to reveal the caret) or an edit hides it, and
//! so does any change to the diagnostics where it was found.

use std::{ops::Range, time::Duration};

use fe_buffer::Version;
use fe_editor::diagnostics::DiagnosticEntry;
use gpui::{
    AnyElement, AvailableSpace, Context, IntoElement, ParentElement as _, Pixels, Point,
    SharedString, Styled as _, Task, anchored, canvas, deferred, div, px,
};

use crate::EditorView;

/// How long the pointer rests on a range before its popover shows.
pub(crate) const HOVER_DELAY: Duration = Duration::from_millis(300);

/// A popover waiting out the delay, or shown.
#[derive(Default)]
pub(crate) struct HoverPopover {
    /// The range waited on, and the delay's timer.
    pending: Option<(Hovered, Task<()>)>,
    shown: Option<Shown>,
}

/// What the pointer rests on.
#[derive(Clone, Debug, PartialEq)]
struct Hovered {
    /// Where: the offset of the grapheme under the pointer, or of the end
    /// of the row it's just past ([`Under`]).
    under: Under,
    /// The range and message of each diagnostic there.
    items: Vec<(Range<usize>, String)>,
    /// The buffer version and scroll it was found at: an edit or a scroll
    /// hides it.
    version: Version,
    scroll: Point<Pixels>,
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

impl Hovered {
    /// What the popover says: each message, a line each.
    fn text(&self) -> SharedString {
        let lines: Vec<&str> = self
            .items
            .iter()
            .map(|(_, message)| message.as_str())
            .collect();
        lines.join("\n").into()
    }
}

struct Shown {
    hovered: Hovered,
    /// Its top-left corner, in window coordinates.
    at: Point<Pixels>,
}

impl EditorView {
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

    /// The pointer moved to `position` over the text, or off it (`None`):
    /// waits out the delay over a diagnostic, or hides the popover once the
    /// pointer has left what it shows.
    pub(crate) fn hover_diagnostics(
        &mut self,
        position: Option<Point<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let found = position.and_then(|position| {
            let under = self.under(position)?;
            let items = under.items(&self.model.diagnostics());
            let hovered = Hovered {
                under,
                items,
                version: self.model.snapshot().version(),
                scroll: self.scroll,
            };
            (!hovered.items.is_empty()).then_some((hovered, position))
        });
        let popover = &mut self.popover;
        let Some((hovered, position)) = found else {
            popover.pending = None;
            if popover.shown.take().is_some() {
                cx.notify();
            }
            return;
        };
        // The same diagnostics, even from another grapheme of their range:
        // the popover, or its wait, goes on, from where the pointer is now.
        let same = |other: &Hovered| {
            other.items == hovered.items
                && other.version == hovered.version
                && other.scroll == hovered.scroll
        };
        if let Some(shown) = popover.shown.as_mut().filter(|shown| same(&shown.hovered)) {
            shown.hovered.under = hovered.under;
            return;
        }
        if let Some((pending, _)) = popover
            .pending
            .as_mut()
            .filter(|(pending, _)| same(pending))
        {
            pending.under = hovered.under;
            return;
        }
        if popover.shown.take().is_some() {
            cx.notify();
        }
        // Below the pointer's row, from the pointer.
        let offset = hovered.under.offset();
        let at = self
            .layout
            .as_mut()
            .and_then(|layout| layout.caret_bounds(offset))
            .map_or(position, |caret| gpui::point(position.x, caret.bottom()));
        let timer = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(HOVER_DELAY).await;
            view.update(cx, |view, cx| {
                // As the pointer has moved since, within the same ones.
                if let Some((hovered, _)) = view.popover.pending.take() {
                    view.popover.shown = Some(Shown { hovered, at });
                    cx.notify();
                }
            })
            .ok();
        });
        self.popover.pending = Some((hovered, timer));
    }

    /// The popover's text, if it's shown and still stands: no edit or
    /// scroll since, and exactly the same diagnostics where the pointer
    /// last was over them.
    pub(crate) fn popover_text(&self) -> Option<SharedString> {
        let shown = &self.popover.shown.as_ref()?.hovered;
        if shown.version != self.model.snapshot().version() || shown.scroll != self.scroll {
            return None;
        }
        (shown.under.items(&self.model.diagnostics()) == shown.items).then(|| shown.text())
    }

    /// Hides the popover, and stops waiting for one.
    pub(crate) fn hide_popover(&mut self, cx: &mut Context<Self>) {
        self.popover.pending = None;
        if self.popover.shown.take().is_some() {
            cx.notify();
        }
    }

    /// The popover, if it's shown and still stands; a stale one is dropped.
    pub(crate) fn render_popover(&mut self) -> Option<AnyElement> {
        #[cfg(test)]
        {
            self.painted_popover = None;
        }
        let Some(text) = self.popover_text() else {
            self.popover.shown = None;
            return None;
        };
        let at = self.popover.shown.as_ref()?.at;
        #[cfg(test)]
        {
            self.painted_popover = Some(text.to_string());
        }
        let theme = &self.theme;
        let popover = div()
            .max_w(px(560.))
            .px_2()
            .py_1()
            .bg(theme.popover_background)
            .border_1()
            .border_color(theme.popover_border)
            .rounded_sm()
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .text_size(theme.font_size)
            .child(text);
        Some(
            anchored()
                .position(at)
                .snap_to_window()
                .child(popover)
                .into_any_element(),
        )
    }
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
