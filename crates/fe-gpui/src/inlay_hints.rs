//! Inlay hints a host gives an editor ([`EditorView::set_inlay_hints`]):
//! short labels drawn inside the text at buffer offsets, such as a
//! variable's inferred type, in a muted chip as VS Code draws them. They're
//! anchored in the buffer and follow edits until the host's next answer
//! replaces them; the caret is never inside one.

use std::{any::Any, future::Future, ops::Range, pin::Pin, rc::Rc};

use fe_buffer::{BufferSnapshot, Version};
use fe_display::layout::{self, Frame, MeasuredLine as _, Vec2};
use gpui::Pixels;
use gpui::{App, Bounds, Context, Hsla, Window, fill, px};

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
    /// Shown in the hover popover when the pointer rests on the hint:
    /// markdown or plain text.
    pub tooltip: Option<String>,
    /// What a double-click on the hint edits, in bytes of the version the
    /// hints were given at.
    pub edits: Vec<(Range<usize>, String)>,
}

/// A hint with its tooltip and edits filled in, once it's resolved.
pub type InlayHintFuture = Pin<Box<dyn Future<Output = InlayHint>>>;

/// Where a view's hints are resolved, set by its host
/// ([`EditorView::set_inlay_hint_host`]): a language server fills in a
/// hint's tooltip and edits only when asked.
pub trait InlayHintHost {
    /// `hint` with its `tooltip` and `edits`, as far as they can be had; as
    /// it is if they can't. Dropping the future cancels the work.
    fn resolve(&self, hint: InlayHint) -> InlayHintFuture;
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

/// Where a hint stands with the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Unresolved,
    /// Asked, and unanswered; whether to apply the edits when it is.
    Pending {
        accept: bool,
    },
    Resolved,
}

/// A view's hints, with their offsets in the buffer at `synced`.
#[derive(Default)]
pub(crate) struct Hints {
    pub(crate) synced: Option<Version>,
    /// The buffer as of the version the hints were given at, which their
    /// edits are in bytes of, if the view still had it then.
    base: Option<BufferSnapshot>,
    pub(crate) hints: Vec<InlayHint>,
    /// Each hint's identity, which edits, resolving and hovering keep it
    /// by; never reused in a view.
    ids: Vec<u64>,
    states: Vec<State>,
    /// The resolutions under way, by hint id: dropped with the hints.
    tasks: std::collections::HashMap<u64, gpui::Task<()>>,
}

/// The next hint identity.
fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Where each of `offsets`, in the buffer at `from`, is in `snapshot`: after
/// text inserted at it, and `None` if an edit replaced or deleted text
/// strictly around it.
pub(crate) fn follow(
    snapshot: &BufferSnapshot,
    from: Version,
    offsets: &[usize],
) -> Vec<Option<usize>> {
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
        let mut kept = Vec::with_capacity(self.hints.len());
        self.hints.retain_mut(|hint| match moved.next().flatten() {
            Some(offset) => {
                hint.offset = offset;
                kept.push(true);
                true
            }
            None => {
                kept.push(false);
                false
            }
        });
        let mut keep = kept.iter();
        self.ids.retain(|_| *keep.next().unwrap_or(&true));
        let mut keep = kept.iter();
        self.states.retain(|_| *keep.next().unwrap_or(&true));
        let ids = &self.ids;
        self.tasks.retain(|id, _| ids.contains(id));
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
        self.inlay_hints_bounds.clear();
        self.inlay_hints = Hints {
            synced: Some(version),
            // Only text the view has is a base to check edits against.
            base: (version == self.model.snapshot().version())
                .then(|| self.model.snapshot().clone()),
            ids: hints.iter().map(|_| next_id()).collect(),
            states: vec![State::Unresolved; hints.len()],
            tasks: Default::default(),
            hints,
        };
        let snapshot = self.model.snapshot().clone();
        self.inlay_hints.advance(&snapshot);
    }

    /// Shows no hints.
    pub fn clear_inlay_hints(&mut self) {
        self.inlay_hints = Hints::default();
        self.inlay_hints_bounds.clear();
    }

    /// Sets where hints are resolved for their tooltips and edits, or
    /// none: then they're used as given.
    pub fn set_inlay_hint_host(&mut self, host: Option<Rc<dyn InlayHintHost>>) {
        self.inlay_hint_host = host;
    }

    /// Where the last frame painted each hint: its offset in the buffer
    /// then and its window bounds.
    pub fn inlay_hint_bounds(&self) -> Vec<(usize, Bounds<Pixels>)> {
        (self.inlay_hints_bounds.iter())
            .filter_map(|(ix, bounds)| Some((self.inlay_hints.hints.get(*ix)?.offset, *bounds)))
            .collect()
    }

    /// The identity of the hint the last frame painted under `position`, if
    /// that frame is of the current text and hints.
    pub(crate) fn inlay_hint_at(&self, position: gpui::Point<Pixels>) -> Option<u64> {
        let current = (self.layout.as_ref())
            .is_some_and(|layout| layout.version == self.model.snapshot().version());
        if !current {
            return None;
        }
        let (ix, _) =
            (self.inlay_hints_bounds.iter()).find(|(_, bounds)| bounds.contains(&position))?;
        self.inlay_hints.ids.get(*ix).copied()
    }

    /// Where hint `id` is now, if it still is.
    pub(crate) fn inlay_hint_ix(&self, id: u64) -> Option<usize> {
        self.inlay_hints.ids.iter().position(|&found| found == id)
    }

    /// Where hint `id`'s tooltip is, once resolved (or with no host to
    /// resolve it).
    pub(crate) fn inlay_hint_tooltip(&self, id: u64) -> Option<String> {
        let ix = self.inlay_hint_ix(id)?;
        (self.inlay_hints.states[ix] == State::Resolved)
            .then(|| self.inlay_hints.hints[ix].tooltip.clone())
            .flatten()
    }

    /// Has the host resolve hint `id` unless that's done or under way,
    /// applying its edits once it has if `accept`. Hover and double-click
    /// share the one request.
    pub(crate) fn request_inlay_hint(&mut self, id: u64, accept: bool, cx: &mut Context<Self>) {
        let Some(ix) = self.inlay_hint_ix(id) else {
            return;
        };
        match self.inlay_hints.states[ix] {
            State::Pending { accept: asked } => {
                self.inlay_hints.states[ix] = State::Pending {
                    accept: asked || accept,
                };
                return;
            }
            State::Resolved => {
                if accept && self.accept_inlay_hint(id, cx) {
                    self.changed(cx);
                }
                return;
            }
            State::Unresolved => {}
        }
        let Some(host) = self.inlay_hint_host.clone() else {
            self.inlay_hints.states[ix] = State::Resolved;
            self.show_hint_tooltip(id, cx);
            if accept && self.accept_inlay_hint(id, cx) {
                self.changed(cx);
            }
            return;
        };
        self.inlay_hints.states[ix] = State::Pending { accept };
        let asked = host.resolve(self.inlay_hints.hints[ix].clone());
        let task = cx.spawn(async move |view, cx| {
            let mut resolved = asked.await;
            view.update(cx, |view, cx| {
                let Some(ix) = view.inlay_hint_ix(id) else {
                    return;
                };
                let hints = &mut view.inlay_hints;
                // Where it is now; its edits are in bytes of the base.
                resolved.offset = hints.hints[ix].offset;
                hints.hints[ix] = resolved;
                let accept = hints.states[ix] == State::Pending { accept: true };
                hints.states[ix] = State::Resolved;
                view.show_hint_tooltip(id, cx);
                if accept && view.accept_inlay_hint(id, cx) {
                    view.changed(cx);
                }
            })
            .ok();
        });
        self.inlay_hints.tasks.insert(id, task);
    }

    /// A double-click at `position`: on a hint with its edits in hand,
    /// applies them. On one the host hasn't resolved, asks it to, and has
    /// the edits applied when it has, while the click does what it does
    /// there meanwhile. Whether the hint took the click.
    pub(crate) fn double_click_inlay_hint(
        &mut self,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(id) = self.inlay_hint_at(position) else {
            return false;
        };
        let Some(ix) = self.inlay_hint_ix(id) else {
            return false;
        };
        if self.inlay_hint_host.is_none() || self.inlay_hints.states[ix] == State::Resolved {
            return self.accept_inlay_hint(id, cx);
        }
        self.request_inlay_hint(id, true, cx);
        false
    }

    /// Applies the edits of hint `id`, if it has any, as one user
    /// transaction, mapped from the version the hints were given at, and
    /// the hint goes. Whether it did. Nothing is applied if the text an edit
    /// replaces has changed since.
    pub(crate) fn accept_inlay_hint(&mut self, id: u64, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self.inlay_hint_ix(id) else {
            return false;
        };
        let hints = &self.inlay_hints;
        let (Some(base), edits) = (hints.base.clone(), hints.hints[ix].edits.clone()) else {
            return false;
        };
        if edits.is_empty() {
            return false;
        }
        // Planned once the model is ready for edits (after vim's session).
        let applied =
            self.edit_and_reveal(move |model| rebase(&base, model.snapshot(), &edits), cx);
        if applied && let Some(ix) = self.inlay_hint_ix(id) {
            let hints = &mut self.inlay_hints;
            hints.hints.remove(ix);
            hints.ids.remove(ix);
            hints.states.remove(ix);
            hints.tasks.remove(&id);
            self.inlay_hints_bounds.clear();
        }
        applied
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

/// `edits`, in bytes of `base`, in bytes of `current`, in order: none if
/// the text any of them replaces has changed since, or they overlap.
fn rebase(
    base: &BufferSnapshot,
    current: &BufferSnapshot,
    edits: &[(Range<usize>, String)],
) -> Vec<(Range<usize>, String)> {
    let from = base.version();
    if from < current.oldest_version() || from > current.version() {
        return Vec::new();
    }
    let len = base.text().len();
    let mut mapped = Vec::with_capacity(edits.len());
    for (range, text) in edits {
        let range = range.start.min(len)..range.end.min(len);
        let start = current.map_offset(range.start, fe_buffer::Bias::Right, from);
        let end = match range.is_empty() {
            true => start,
            false => current.map_offset(range.end, fe_buffer::Bias::Left, from),
        };
        let unchanged = range.is_empty()
            || (start <= end
                && base.text().slice_to_string(range.clone())
                    == current.text().slice_to_string(start..end));
        if !unchanged {
            return Vec::new();
        }
        mapped.push((start..end, text.clone()));
    }
    mapped.sort_by_key(|(range, _)| range.start);
    let overlap = mapped
        .windows(2)
        .any(|pair| pair[0].0.end > pair[1].0.start);
    if overlap { Vec::new() } else { mapped }
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
    ix: usize,
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
                ix,
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
    bounds_out: &mut Vec<(usize, Bounds<Pixels>)>,
    #[cfg(test)] painted: &mut Vec<(String, Bounds<Pixels>)>,
) {
    let line_height = px(layout.frame.line_height);
    for hint in shown {
        let bounds = layout.bounds(hint.rect);
        bounds_out.push((hint.ix, bounds));
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
