//! A column of the host's text left of the line numbers, one entry per run
//! of buffer rows, as JetBrains' "Annotate with Git Blame" shows each row's
//! commit: [`EditorView::set_gutter_annotations`].
//!
//! - **Width.** The widest entry's text in the editor's font, plus an em,
//!   at most [`MAX_WIDTH_EMS`]; measured when the entries or the font
//!   change, not every frame. No entries, or no gutter, leave no column.
//! - **Rows.** Each entry's rows come from its anchors at every buffer
//!   version. A row an edit has touched since the annotations' version
//!   shows nothing until the host gives new ones; the rows around it move
//!   with the text.
//! - **Pointer.** Resting on an entry's row shows the host's
//!   [`GutterAnnotations::hover`] in the popover below it; a plain left
//!   click calls [`GutterAnnotations::on_click`], leaving the carets and
//!   folds be. Elsewhere in the column a click does nothing.
//! - **Agent lane.** Rows a host names as an agent's
//!   ([`EditorView::set_gutter_annotations_with_agent_lane`]) are drawn as
//!   a dotted lane in the change bar instead, in the theme's `agent_lane`
//!   (`docs/design/design-system.md#agent-provenance`); they follow edits
//!   as the entries do.

use std::{ops::Range, rc::Rc};

use fe_buffer::{Anchor, Bias, BufferSnapshot, Edit, Version};
use gpui::{
    App, Bounds, Context, FontId, MouseDownEvent, Pixels, SharedString, TextRun, Window,
    WindowTextSystem, px,
};

use crate::{EditorView, HoverContent};

/// The widest the column gets, in ems; an entry's text past it is clipped.
pub(crate) const MAX_WIDTH_EMS: f32 = 32.;

/// One entry: text for a run of buffer rows.
#[derive(Clone, Debug, PartialEq)]
pub struct GutterAnnotation {
    /// The rows' bytes, anchored in the buffer at `GutterAnnotations::version`.
    pub rows: Range<Anchor>,
    pub text: SharedString,
    /// 0.0 oldest to 1.0 newest; None paints no tint.
    pub heat: Option<f32>,
}

/// What the column shows, and what hovering and clicking it do.
#[derive(Clone)]
pub struct GutterAnnotations {
    pub version: Version,
    pub entries: Vec<GutterAnnotation>,
    /// The popover for entry `ix`, hovered at buffer row `row`.
    pub hover: Rc<dyn Fn(usize, u32) -> Option<HoverContent>>,
    /// Entry `ix` clicked at buffer row `row`.
    #[allow(clippy::type_complexity)]
    pub on_click: Rc<dyn Fn(usize, u32, &mut Window, &mut App)>,
}

/// The view's annotations, with what's derived from them.
pub(crate) struct Annotated {
    pub(crate) annotations: GutterAnnotations,
    /// The agent lane's rows' bytes, anchored at the annotations' version.
    agent: Vec<Range<Anchor>>,
    /// The column's width, for the font and size it was measured in.
    width: Option<(FontId, Pixels, f32)>,
    /// The entries' rows, at the buffer version last resolved at.
    resolved: Option<Resolved>,
}

/// The entries' buffer rows and the rows edited since the annotations'
/// version, at one buffer version.
pub(crate) struct Resolved {
    version: Version,
    /// Each entry's rows; none if the annotations' version can't be
    /// mapped from.
    entries: Vec<Range<u32>>,
    /// The entries' rows and indexes, by first row.
    by_row: Vec<(Range<u32>, usize)>,
    /// The rows edits have touched since, sorted and apart.
    edited: Vec<Range<u32>>,
    /// The agent lane's rows, sorted by start.
    agent: Vec<Range<u32>>,
}

impl Resolved {
    /// The entry row `row` shows, if it isn't edited: the first by start
    /// of those holding it.
    pub(crate) fn entry_at(&self, row: u32) -> Option<usize> {
        if self.is_edited(row) {
            return None;
        }
        let after = self.by_row.partition_point(|(rows, _)| rows.start <= row);
        (self.by_row[..after].iter().rev())
            .find(|(rows, _)| rows.contains(&row))
            .map(|&(_, ix)| ix)
    }

    /// Whether an edit since the annotations' version touched `row`.
    fn is_edited(&self, row: u32) -> bool {
        let edited = self.edited.partition_point(|rows| rows.end <= row);
        (self.edited.get(edited)).is_some_and(|rows| rows.start <= row)
    }

    /// Whether `row` is in the agent lane: an agent's, and not edited since.
    pub(crate) fn agent_at(&self, row: u32) -> bool {
        let after = self.agent.partition_point(|rows| rows.start <= row);
        let agent = self.agent[..after].iter().any(|rows| rows.contains(&row));
        agent && !self.is_edited(row)
    }

    /// The first buffer row of entry `ix`.
    pub(crate) fn first_row(&self, ix: usize) -> Option<u32> {
        Some(self.entries.get(ix)?.start)
    }
}

impl EditorView {
    /// Shows `annotations` in a column left of the line numbers, or takes
    /// the column away (`None`): see [`crate::gutter_annotations`]. A view
    /// whose gutter is hidden ([`EditorView::set_gutter`]) shows none.
    pub fn set_gutter_annotations(
        &mut self,
        annotations: Option<GutterAnnotations>,
        cx: &mut Context<Self>,
    ) {
        self.show_annotations(annotations.map(|annotations| (annotations, Vec::new())), cx);
    }

    /// Shows `annotations` as [`Self::set_gutter_annotations`] does, and
    /// draws the rows of `agent`, anchored at the annotations' version, as
    /// the change bar's dotted agent lane: see [`crate::gutter_annotations`].
    pub fn set_gutter_annotations_with_agent_lane(
        &mut self,
        annotations: GutterAnnotations,
        agent: Vec<Range<Anchor>>,
        cx: &mut Context<Self>,
    ) {
        self.show_annotations(Some((annotations, agent)), cx);
    }

    fn show_annotations(
        &mut self,
        annotations: Option<(GutterAnnotations, Vec<Range<Anchor>>)>,
        cx: &mut Context<Self>,
    ) {
        self.annotations = annotations.map(|(annotations, agent)| Annotated {
            annotations,
            agent,
            width: None,
            resolved: None,
        });
        // What it showed for the old ones goes.
        if self.popover_shows_annotation() {
            self.hide_popover(cx);
        }
        cx.notify();
    }

    pub fn gutter_annotations(&self) -> Option<&GutterAnnotations> {
        self.annotations.as_ref().map(|shown| &shown.annotations)
    }

    /// The window bounds of buffer row `buffer_row`'s slot in the column,
    /// all its display rows, in the last frame: `None` if there's no column
    /// or the row isn't in the frame.
    pub fn annotation_bounds(&self, buffer_row: u32) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let frame = &layout.frame;
        let column = frame.annotation_column;
        if column.width <= 0. {
            return None;
        }
        let mut rows = (frame.rows.iter()).filter(|row| row.buffer_row == buffer_row);
        let first = rows.next()?;
        let last = rows.next_back().unwrap_or(first);
        let top = frame.row_top(first.row);
        let bottom = frame.row_top(last.row) + frame.line_height;
        Some(layout.bounds(fe_display::layout::Rect {
            x: column.x,
            y: top,
            width: column.width,
            height: bottom - top,
        }))
    }

    /// Of a left mouse-down at a window position: if it's in the last
    /// frame's annotation column, and a plain click on an entry's row, calls
    /// the host's [`GutterAnnotations::on_click`] once this listener is
    /// done. Whether it was in the column.
    pub(crate) fn click_annotation(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let column = layout.bounds(layout.frame.annotation_column);
        if column.size.width <= px(0.) || !column.contains(&event.position) {
            return false;
        }
        let current = layout.version == self.model.snapshot().version();
        let row = layout.annotation_row_at(event.position).map(|(row, _)| row);
        let plain = event.click_count == 1 && !event.modifiers.modified();
        let Some(row) = row.filter(|_| current && plain) else {
            return true;
        };
        let Some(ix) = self
            .resolved_annotations()
            .and_then(|resolved| resolved.entry_at(row))
        else {
            return true;
        };
        let Some(on_click) =
            (self.annotations.as_ref()).map(|shown| shown.annotations.on_click.clone())
        else {
            return true;
        };
        // The host may update this view, which is busy until then.
        window.defer(cx, move |window, cx| on_click(ix, row, window, cx));
        true
    }

    /// The column's width for the next frame: 0 with no annotations,
    /// entries or gutter; measured in `font_id` at `font_size` if it wasn't
    /// already.
    pub(crate) fn annotation_width(
        &mut self,
        text_system: &WindowTextSystem,
        font: &gpui::Font,
        font_id: FontId,
        font_size: Pixels,
        em: f32,
    ) -> f32 {
        let Some(shown) = self.annotations.as_mut().filter(|_| self.gutter) else {
            return 0.;
        };
        let entries = &shown.annotations.entries;
        if entries.is_empty() {
            return 0.;
        }
        if let Some((id, size, width)) = shown.width
            && id == font_id
            && size == font_size
        {
            return width;
        }
        let widest = (entries.iter())
            .map(|entry| {
                let run = TextRun {
                    len: entry.text.len(),
                    font: font.clone(),
                    color: gpui::Hsla::default(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                f32::from(
                    text_system
                        .layout_line(&entry.text, font_size, &[run], None)
                        .width,
                )
            })
            .fold(0., f32::max);
        let width = (widest + em).min(em * MAX_WIDTH_EMS);
        shown.width = Some((font_id, font_size, width));
        width
    }

    /// The entries' rows at the buffer's current version, resolved once per
    /// version.
    pub(crate) fn resolved_annotations(&mut self) -> Option<&Resolved> {
        let shown = self.annotations.as_mut()?;
        let snapshot = self.model.snapshot();
        if shown.resolved.as_ref().map(|resolved| resolved.version) != Some(snapshot.version()) {
            shown.resolved = Some(resolve(&shown.annotations, &shown.agent, snapshot));
        }
        shown.resolved.as_ref()
    }

    /// The annotations, and their rows at the buffer's current version if
    /// [`Self::resolved_annotations`] has resolved them at it.
    pub(crate) fn annotation_rows(&self) -> Option<(&GutterAnnotations, &Resolved)> {
        let shown = self.annotations.as_ref()?;
        let resolved = (shown.resolved.as_ref())
            .filter(|resolved| resolved.version == self.model.snapshot().version())?;
        Some((&shown.annotations, resolved))
    }
}

/// `annotations`' rows and the agent lane's, `agent`, in `snapshot`: none at
/// all for a version it can't map from.
fn resolve(
    annotations: &GutterAnnotations,
    agent: &[Range<Anchor>],
    snapshot: &BufferSnapshot,
) -> Resolved {
    let version = annotations.version;
    let entries = &annotations.entries;
    let (floor, now) = (snapshot.oldest_version(), snapshot.version());
    let in_log = |version: Version| floor <= version && version <= now;
    let ranges = || (entries.iter().map(|entry| &entry.rows)).chain(agent);
    let mappable = in_log(version)
        && ranges().all(|rows| in_log(rows.start.version()) && in_log(rows.end.version()));
    if !mappable {
        return Resolved {
            version: now,
            entries: Vec::new(),
            by_row: Vec::new(),
            edited: Vec::new(),
            agent: Vec::new(),
        };
    }
    let text = snapshot.text();
    let anchors: Vec<Anchor> = ranges().flat_map(|rows| [rows.start, rows.end]).collect();
    let offsets = snapshot.resolve_many(&anchors);
    let mut rows: Vec<Range<u32>> = (offsets.chunks(2))
        .map(|pair| rows_of(text, pair[0]..pair[1].max(pair[0])))
        .collect();
    let mut agent = rows.split_off(entries.len());
    agent.sort_by_key(|rows| rows.start);
    let mut by_row: Vec<(Range<u32>, usize)> = (rows.iter().cloned()).zip(0..).collect();
    by_row.sort_by_key(|(rows, _)| rows.start);
    let mut edited: Vec<Range<u32>> = Vec::new();
    for range in edited_ranges(snapshot, version) {
        let rows = rows_of(text, range);
        match edited.last_mut() {
            Some(last) if rows.start <= last.end => last.end = last.end.max(rows.end),
            _ => edited.push(rows),
        }
    }
    Resolved {
        version: now,
        entries: rows,
        by_row,
        edited,
        agent,
    }
}

/// The buffer rows `range` covers, half open: through the row its end is
/// on, unless that end starts the row.
fn rows_of(text: &fe_buffer::Text, range: Range<usize>) -> Range<u32> {
    let first = text.offset_to_point(range.start).row;
    let mut last = text.offset_to_point(range.end).row;
    if last > first && range.end == text.row_start(last) {
        last -= 1;
    }
    first..last + 1
}

/// The bytes of `snapshot`'s text that edits since `version` wrote: each
/// batch's new ranges, carried through the batches after it. Sorted, and
/// apart but for a deletion's empty range at another's end.
pub(crate) fn edited_ranges(snapshot: &BufferSnapshot, version: Version) -> Vec<Range<usize>> {
    let mut edited: Vec<Range<usize>> = Vec::new();
    for batch in snapshot.edits_since(version) {
        for range in &mut edited {
            let start = map_through(batch, range.start, Bias::Left);
            *range = start..map_through(batch, range.end, Bias::Right).max(start);
        }
        edited.extend(batch.iter().map(|edit| edit.new.clone()));
        edited.sort_by_key(|range| range.start);
        // A deletion's empty range at another's end stays apart: it marks
        // the row starting there, which the other's rows end before.
        edited.dedup_by(|next, last| {
            let touches = next.start < last.end
                || next.start == last.end && (next.start < next.end || last.start == last.end);
            if touches {
                last.end = last.end.max(next.end);
            }
            touches
        });
    }
    edited
}

/// Maps `offset` through one batch of edits: shifted by those before it;
/// at text inserted at it, before it (`Left`) or after it; inside text
/// replaced, to the replacement's start (`Left`) or end.
fn map_through(batch: &[Edit], offset: usize, bias: Bias) -> usize {
    // The batch's edits so far moved what's after them by this much.
    let mut shift = 0isize;
    for edit in batch {
        let old = &edit.old;
        if old.start > offset {
            break;
        }
        let before =
            old.end < offset || (old.end == offset && (old.start < offset || bias == Bias::Right));
        if before {
            shift = edit.new.end as isize - old.end as isize;
            continue;
        }
        return match bias {
            Bias::Left => edit.new.start,
            Bias::Right => edit.new.end,
        };
    }
    offset.saturating_add_signed(shift)
}

#[cfg(test)]
mod map_tests {
    use super::*;

    fn edit(old: Range<usize>, new: Range<usize>) -> Edit {
        Edit { old, new }
    }

    #[test]
    fn offsets_follow_a_batch() {
        // "abcdef": "X" inserted at 1, "cd" replaced by "YYY".
        let batch = [edit(1..1, 1..2), edit(2..4, 3..6)];
        assert_eq!(map_through(&batch, 0, Bias::Right), 0);
        assert_eq!(map_through(&batch, 1, Bias::Left), 1);
        assert_eq!(map_through(&batch, 1, Bias::Right), 2);
        assert_eq!(map_through(&batch, 2, Bias::Left), 3);
        assert_eq!(map_through(&batch, 3, Bias::Left), 3);
        assert_eq!(map_through(&batch, 3, Bias::Right), 6);
        assert_eq!(map_through(&batch, 4, Bias::Left), 6);
        assert_eq!(map_through(&batch, 6, Bias::Left), 8);
    }

    /// One entry over all of "zero\none\ntwo\n"'s rows, at its first
    /// version; after `edit`, the rows still showing it.
    fn shown_after(edit: impl FnOnce(&mut fe_buffer::Buffer)) -> Vec<u32> {
        let mut buffer = fe_buffer::Buffer::new("zero\none\ntwo\n");
        let snapshot = buffer.snapshot();
        let annotations = GutterAnnotations {
            version: snapshot.version(),
            entries: vec![GutterAnnotation {
                rows: snapshot.anchor_before(0)..snapshot.anchor_before(snapshot.text().len()),
                text: "entry".into(),
                heat: None,
            }],
            hover: Rc::new(|_, _| None),
            on_click: Rc::new(|_, _, _, _| {}),
        };
        edit(&mut buffer);
        let resolved = resolve(&annotations, &[], buffer.current());
        let rows = buffer.current().text().row_count();
        (0..rows)
            .filter(|&row| resolved.entry_at(row).is_some())
            .collect()
    }

    #[test]
    fn a_deletion_at_an_insertions_end_blanks_its_row() {
        use fe_buffer::Origin;
        // "zero\nnew\n|ne\ntwo\n": rows 1 ("new") and 2 ("ne") are edited.
        let shown = shown_after(|buffer| {
            buffer.edit([(5..5, "new\n")], Origin::User);
            buffer.edit([(9..10, "")], Origin::User);
        });
        assert_eq!(shown, [0, 3]);
        // The same in one batch.
        let shown = shown_after(|buffer| {
            buffer.edit([(5..5, "new\n"), (5..6, "")], Origin::User);
        });
        assert_eq!(shown, [0, 3]);
        // A deletion at an insertion's start: "e\n" from row 1's end, then
        // "\nnew" there, for "zero\non\nnewtwo\n": rows 1 and 2 are edited.
        let shown = shown_after(|buffer| {
            buffer.edit([(7..9, "")], Origin::User);
            buffer.edit([(7..7, "\nnew")], Origin::User);
        });
        assert_eq!(shown, [0]);
    }
}
