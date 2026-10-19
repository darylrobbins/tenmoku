//! Inlay hints a host gives an editor ([`EditorView::set_inlay_hints`]):
//! short labels drawn inside the text at buffer offsets, such as a
//! variable's inferred type, in a muted chip as VS Code draws them. They're
//! anchored in the buffer and follow edits until the host's next answer
//! replaces them; the caret is never inside one.

use std::{any::Any, rc::Rc};

use fe_buffer::{BufferSnapshot, Version};
use fe_display::layout::{self, Frame, MeasuredLine as _, Vec2};
#[cfg(test)]
use gpui::Pixels;
use gpui::{App, Bounds, Hsla, Window, fill, px};

use crate::{EditorView, element::FrameLayout, element::Line, element::Shaper};

/// What an [`InlayHint`] labels.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InlayHintKind {
    Type,
    Parameter,
}

/// A label drawn at a buffer offset, not part of the buffer.
#[derive(Clone)]
pub struct InlayHint {
    pub offset: usize,
    pub label: String,
    pub kind: Option<InlayHintKind>,
    /// Draw a space before the label.
    pub padding_left: bool,
    /// Draw a space after the label.
    pub padding_right: bool,
    /// The host's, for a tooltip or an edit.
    pub data: Option<Rc<dyn Any>>,
}

impl InlayHint {
    /// The text drawn: the label with a space for each side of padding.
    pub(crate) fn text(&self) -> String {
        let (left, right) = (self.padding_left, self.padding_right);
        let mut text = String::with_capacity(self.label.len() + 2);
        text.push_str(if left { " " } else { "" });
        text.push_str(&self.label.replace(['\n', '\r'], " "));
        text.push_str(if right { " " } else { "" });
        text
    }
}

/// A view's hints, with their offsets in the buffer at `synced`.
#[derive(Default)]
pub(crate) struct Hints {
    synced: Option<Version>,
    hints: Vec<InlayHint>,
}

/// Where each of `offsets`, in the buffer at `from`, is in `snapshot`: after
/// text inserted at it, and `None` if an edit replaced or deleted text
/// strictly around it.
fn follow(snapshot: &BufferSnapshot, from: Version, offsets: &[usize]) -> Vec<Option<usize>> {
    // Compacted away: the offsets can't be carried, so the hints go.
    if from < snapshot.oldest_version() {
        return vec![None; offsets.len()];
    }
    let mut offsets: Vec<Option<usize>> = offsets.iter().map(|&offset| Some(offset)).collect();
    for batch in snapshot.edits_since(from) {
        for slot in &mut offsets {
            let Some(offset) = *slot else { continue };
            let mut shift = 0isize;
            for edit in batch.iter() {
                let old = &edit.old;
                if old.start > offset || (old.start == offset && old.end > offset) {
                    break;
                }
                if old.end > offset {
                    *slot = None;
                    break;
                }
                shift = edit.new.end as isize - old.end as isize;
            }
            if slot.is_some() {
                *slot = Some(offset.saturating_add_signed(shift));
            }
        }
    }
    offsets
}

impl Hints {
    /// Moves the hints on to `snapshot`'s version, dropping those an edit
    /// took the text of.
    fn advance(&mut self, snapshot: &BufferSnapshot) {
        let Some(from) = self.synced else { return };
        if from == snapshot.version() {
            return;
        }
        let offsets: Vec<usize> = self.hints.iter().map(|hint| hint.offset).collect();
        let moved = follow(snapshot, from, &offsets);
        let mut moved = moved.into_iter();
        self.hints.retain_mut(|hint| match moved.next().flatten() {
            Some(offset) => {
                hint.offset = offset;
                true
            }
            None => false,
        });
        self.synced = Some(snapshot.version());
    }
}

impl EditorView {
    /// Shows `hints`, whose offsets are in the buffer at `version`, in place
    /// of any shown before, from the next frame. They follow edits; one with
    /// an edit's deleted or replaced text strictly around its offset is
    /// dropped. Hints for a version the buffer never had, or has forgotten,
    /// are ignored.
    pub fn set_inlay_hints(&mut self, version: Version, mut hints: Vec<InlayHint>) {
        self.model.sync_display();
        let snapshot = self.model.snapshot();
        if version > snapshot.version() || version < snapshot.oldest_version() {
            log::warn!("inlay hints for a version the buffer doesn't have aren't shown");
            return;
        }
        // Clipped, in order, so a frame lays them out in order.
        if version == snapshot.version() {
            for hint in &mut hints {
                hint.offset = snapshot.resolve(&snapshot.anchor_before(hint.offset));
            }
        }
        hints.sort_by_key(|hint| hint.offset);
        self.inlay_hints = Hints {
            synced: Some(version),
            hints,
        };
        let snapshot = self.model.snapshot().clone();
        self.inlay_hints.advance(&snapshot);
    }

    /// Shows no hints.
    pub fn clear_inlay_hints(&mut self) {
        self.inlay_hints = Hints::default();
    }

    /// The hints shown, each with its offset in the buffer now and its
    /// label.
    pub fn inlay_hints(&self) -> Vec<(usize, String)> {
        let hints = &self.inlay_hints;
        let snapshot = self.model.snapshot();
        let Some(from) = hints.synced else {
            return Vec::new();
        };
        let offsets: Vec<usize> = hints.hints.iter().map(|hint| hint.offset).collect();
        (follow(snapshot, from, &offsets).into_iter())
            .zip(&hints.hints)
            .filter_map(|(offset, hint)| Some((offset?, hint.label.clone())))
            .collect()
    }

    /// Moves the hints to the buffer as it is, before a frame is laid out.
    pub(crate) fn sync_inlay_hints(&mut self) {
        if self.inlay_hints.synced.is_none() {
            return;
        }
        let snapshot = self.model.snapshot().clone();
        self.inlay_hints.advance(&snapshot);
    }
}

/// The hints a frame lays out, in the transparent style: the chip's text is
/// painted separately, smaller ([`layout`]).
pub(crate) fn inlays(view: &EditorView) -> Vec<layout::Inlay<Hsla>> {
    (view.inlay_hints.hints.iter())
        .map(|hint| layout::Inlay {
            offset: hint.offset,
            text: hint.text(),
            style: Hsla::transparent_black(),
        })
        .collect()
}

/// A hint a frame laid out, shaped small to paint.
pub(crate) struct Shown {
    #[cfg(test)]
    text: String,
    line: Line,
    rect: layout::Rect,
}

/// Shapes the hints `frame` laid out, from the view's hints, with `small`.
pub(crate) fn layout(frame: &Frame<Line>, view: &EditorView, small: &mut Shaper) -> Vec<Shown> {
    let muted = view.theme.gutter_foreground;
    (frame.hints.iter())
        .filter_map(|&(ix, rect)| {
            let text = view.inlay_hints.hints.get(ix)?.text();
            let runs = [layout::StyleRun {
                len: text.len(),
                style: muted,
            }];
            let line = layout::TextMeasure::shape(small, &text, &runs);
            Some(Shown {
                #[cfg(test)]
                text,
                line,
                rect,
            })
        })
        .collect()
}

/// Paints the chips and their text, noting in tests what was painted.
pub(crate) fn paint(
    shown: &[Shown],
    layout: &FrameLayout,
    theme_muted: Hsla,
    window: &mut Window,
    cx: &mut App,
    #[cfg(test)] painted: &mut Vec<(String, Bounds<Pixels>)>,
) {
    let line_height = px(layout.frame.line_height);
    for hint in shown {
        let bounds = layout.bounds(hint.rect);
        // A little in from the row's top and bottom, as a chip.
        let inset = px(1.);
        let chip = Bounds::new(
            bounds.origin + gpui::point(px(0.), inset),
            gpui::size(bounds.size.width, bounds.size.height - inset * 2.),
        );
        let mut background = fill(chip, theme_muted.opacity(0.1));
        background.corner_radii = gpui::Corners::all(px(3.));
        window.paint_quad(background);
        let across = (hint.rect.width - hint.line.width()).max(0.) / 2.;
        let origin = layout.to_window(Vec2 {
            x: hint.rect.x + across,
            y: hint.rect.y,
        });
        hint.line.paint(origin, line_height, window, cx);
        #[cfg(test)]
        painted.push((hint.text.clone(), bounds));
    }
}
