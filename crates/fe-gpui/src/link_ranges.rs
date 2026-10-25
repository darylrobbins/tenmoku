//! The host's links in an editor's text, such as a language server's
//! document links: [`EditorView::set_link_ranges`].
//!
//! - **Edits.** Each link is anchored where it was set and moves with the
//!   text until the next set; text typed at either edge stays outside it.
//!   Links set before the buffer's oldest mappable version are dropped.
//! - **Hover.** With a modifier held whose click is bound, the pointer over
//!   a link underlines the whole link rather than the word under it
//!   ([`EditorView::hover_at`]), found again on the frame after a set even
//!   with nothing hovered, as over a space in a link.
//! - **Click.** A bound modifier click notes the link under the pointer
//!   before it places the caret, which may land at either edge of the
//!   glyph clicked; the command it runs takes it
//!   ([`EditorView::take_clicked_link`]).

use std::ops::Range;

use fe_buffer::{Anchor, Version};
use gpui::{Pixels, Point};

use crate::EditorView;

/// The view's links, anchored at the version they were set at.
#[derive(Default)]
pub(crate) struct LinkRanges {
    version: Option<Version>,
    ranges: Vec<Range<Anchor>>,
    /// The link the last bound click was on, if any, at the buffer's
    /// version then.
    clicked: Option<(Version, Range<usize>)>,
    /// Set since the hover was last found: the next frame finds it again,
    /// though there was none, as over a link's space.
    changed: bool,
}

impl EditorView {
    /// Sets the links in the text, in bytes of the buffer's version now,
    /// replacing any set before: see [`crate::link_ranges`]. Empty ranges
    /// are dropped.
    pub fn set_link_ranges(&mut self, ranges: Vec<Range<usize>>) {
        let snapshot = self.model.snapshot();
        self.links = LinkRanges {
            changed: true,
            clicked: None,
            version: Some(snapshot.version()),
            ranges: (ranges.into_iter())
                .filter(|range| !range.is_empty())
                .map(|range| snapshot.anchor_after(range.start)..snapshot.anchor_before(range.end))
                .collect(),
        };
    }

    /// The links in the text, in bytes of the buffer's version now, each
    /// moved by the edits since it was set; those an edit emptied are gone.
    pub fn link_ranges(&self) -> Vec<Range<usize>> {
        let snapshot = self.model.snapshot();
        let Some(version) = self.links.version else {
            return Vec::new();
        };
        if version < snapshot.oldest_version() || version > snapshot.version() {
            return Vec::new();
        }
        (self.links.ranges.iter())
            .map(|range| snapshot.resolve_range(range))
            .filter(|range| !range.is_empty())
            .collect()
    }

    /// The link holding the char at byte `offset`, if any.
    pub fn link_at(&self, offset: usize) -> Option<Range<usize>> {
        (self.link_ranges().into_iter()).find(|range| range.contains(&offset))
    }

    /// Whether the links were set since this was last asked.
    pub(crate) fn take_links_changed(&mut self) -> bool {
        std::mem::take(&mut self.links.changed)
    }

    /// Notes the link under a bound click at `position`, or none.
    pub(crate) fn note_clicked_link(&mut self, position: Point<Pixels>) {
        let version = self.model.snapshot().version();
        self.links.clicked = self.link_under(position).map(|link| (version, link));
    }

    /// The link the last bound click was on, which it takes: none if the
    /// click wasn't on one, or the text has changed since.
    pub fn take_clicked_link(&mut self) -> Option<Range<usize>> {
        let (version, link) = self.links.clicked.take()?;
        (version == self.model.snapshot().version()).then_some(link)
    }

    /// The link holding the glyph under a window position in the last
    /// frame, if any; none if the text changed since that frame.
    pub(crate) fn link_under(&self, position: Point<Pixels>) -> Option<Range<usize>> {
        if self.links.ranges.is_empty() {
            return None;
        }
        let layout = self.layout.as_ref()?;
        if layout.version != self.model.snapshot().version() {
            return None;
        }
        let offset = layout.char_at(self.model.text(), position)?;
        self.link_at(offset)
    }
}

#[cfg(test)]
#[path = "link_ranges_tests.rs"]
mod tests;
