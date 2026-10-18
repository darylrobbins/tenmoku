//! Merge conflicts in the editor: each one's rows banded by side, and
//! `git.next_conflict` and `git.previous_conflict` to go from one to the
//! next, wrapping round. The markers are parsed by
//! [`fe_git::conflict_markers::parse`].

use std::{cell::RefCell, ops::Range};

use fe_buffer::Origin;
use fe_buffer::{BufferSnapshot, Version};
use fe_editor::decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps};
use fe_git::conflict_markers;
use gpui::{Context, Window};

use crate::{
    Workbench,
    workbench::toast::{self, Toast},
};

/// Larger files than this get no bands (and are not scanned for them).
pub(crate) const MAX_SCAN_BYTES: usize = 8 << 20;

/// Below diagnostics' decorations and the git change bar's, so they still
/// show.
const PRIORITY: i32 = -200;

const WRAPPED_TO_FIRST: &str = "Wrapped to the first conflict";
const WRAPPED_TO_LAST: &str = "Wrapped to the last conflict";
const NO_CONFLICTS: &str = "No merge conflicts found in this file";
const NOT_IN_CONFLICT: &str = "Editor cursor is not within a merge conflict";
const READ_ONLY: &str = "Cannot make changes, the editor is read-only";

/// Which side of a conflict an accept command keeps.
#[derive(Clone, Copy)]
pub(crate) enum Side {
    Current,
    Incoming,
    Both,
}

/// Which conflicts an accept command resolves: the one holding a row (the
/// primary caret's, if none is given), or every one.
#[derive(Clone, Copy)]
pub(crate) enum Which {
    At(Option<u32>),
    All,
}

/// Whether `text` has a start marker at a row's start.
fn has_marker(text: &str) -> bool {
    text.starts_with("<<<<<<<") || text.contains("\n<<<<<<<")
}

/// `snapshot`'s text, if it has a start marker, however large.
fn marked(snapshot: &BufferSnapshot) -> Option<String> {
    let text = snapshot.text();
    let text = text.slice_to_string(0..text.len());
    has_marker(&text).then_some(text)
}

/// `snapshot`'s text, if it's small enough to band and has a start marker.
fn scannable(snapshot: &BufferSnapshot) -> Option<String> {
    (snapshot.text().len() <= MAX_SCAN_BYTES)
        .then(|| marked(snapshot))
        .flatten()
}

/// Bands each conflict in a snapshot's rows by side: the start marker and
/// the current side added, a base muted, the incoming side and the end
/// marker modified.
#[derive(Default)]
pub(crate) struct ConflictBands {
    cache: RefCell<Option<(Version, Vec<Decoration>)>>,
}

impl ConflictBands {
    fn bands(snapshot: &BufferSnapshot) -> Vec<Decoration> {
        let Some(text) = scannable(snapshot) else {
            return Vec::new();
        };
        let band = |range: Range<usize>, tint| {
            Decoration::new(range, DecorationKind::RowBackground, tint).with_priority(PRIORITY)
        };
        let mut out = Vec::new();
        for conflict in conflict_markers::parse(&text) {
            out.push(band(
                conflict.start.line.start..conflict.current.end,
                Tint::Added,
            ));
            for (marker, rows) in &conflict.bases {
                out.push(band(marker.line.start..rows.end, Tint::Muted));
            }
            out.push(band(
                conflict.incoming.start..conflict.end.line.end,
                Tint::Modified,
            ));
        }
        out
    }
}

impl DecorationProvider for ConflictBands {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let mut cache = self.cache.borrow_mut();
        let version = snapshot.version();
        if cache.as_ref().is_none_or(|(cached, _)| *cached != version) {
            *cache = Some((version, Self::bands(snapshot)));
        }
        let bands = cache.as_ref().map_or(&[][..], |(_, bands)| bands);
        out.extend(
            bands
                .iter()
                .filter(|band| overlaps(&band.range, &range))
                .cloned(),
        );
    }
}

impl Workbench {
    /// `git.next_conflict` and `git.previous_conflict`: the caret to the
    /// start of the next or previous conflict's start marker row in the
    /// active editor, wrapping at the ends, a toast saying so (as
    /// `answer_git_op` does for a command from the palette or a key).
    pub(crate) fn go_to_conflict(
        &mut self,
        forward: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let (starts, row) = {
            let model = editor.read(cx).model();
            let text = model.text();
            let starts: Vec<usize> = match marked(model.snapshot()) {
                Some(string) => (conflict_markers::parse(&string).iter())
                    .map(|conflict| conflict.start.line.start)
                    .collect(),
                None => Vec::new(),
            };
            let head = model.primary_selection().head();
            let row = text.offset_to_point(head).row;
            let rows: Vec<u32> = (starts.iter())
                .map(|&at| text.offset_to_point(at).row)
                .collect();
            (rows.into_iter().zip(starts).collect::<Vec<_>>(), row)
        };
        if starts.is_empty() {
            return toast::show(cx, Toast::info(NO_CONFLICTS.to_owned()));
        }
        let found = match forward {
            true => starts.iter().find(|(start, _)| *start > row),
            false => starts.iter().rev().find(|(start, _)| *start < row),
        };
        let (at, wrapped) = match (found, forward) {
            (Some((_, at)), _) => (*at, None),
            (None, true) => (starts[0].1, Some(WRAPPED_TO_FIRST)),
            (None, false) => (starts[starts.len() - 1].1, Some(WRAPPED_TO_LAST)),
        };
        editor.update(cx, |editor, cx| editor.select_and_reveal(at..at, cx));
        if let Some(wrapped) = wrapped {
            toast::show(cx, Toast::info(wrapped.to_owned()));
        }
    }

    /// `git.accept_*`: replaces the chosen conflicts' marker blocks in the
    /// active editor with `side`'s lines (both: current, then incoming),
    /// the base dropped, in one user transaction, and puts the caret at the
    /// start of the first replacement.
    pub(crate) fn accept(
        &mut self,
        side: Side,
        which: Which,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        if editor.read(cx).is_read_only() {
            return toast::show(cx, Toast::info(READ_ONLY.to_owned()));
        }
        let edits = {
            let model = editor.read(cx).model();
            let text = model.text();
            let string = marked(model.snapshot()).unwrap_or_default();
            let mut conflicts = conflict_markers::parse(&string);
            match which {
                Which::All => {}
                Which::At(row) => {
                    let row = row.unwrap_or_else(|| {
                        text.offset_to_point(model.primary_selection().head()).row
                    });
                    conflicts.retain(|conflict| {
                        let range = conflict.range();
                        let (first, last) = (
                            text.offset_to_point(range.start).row,
                            text.offset_to_point(range.end.saturating_sub(1).max(range.start))
                                .row,
                        );
                        (first..=last).contains(&row)
                    });
                }
            }
            let edits: Vec<(Range<usize>, String)> = (conflicts.iter())
                .map(|conflict| {
                    let current = &string[conflict.current.clone()];
                    let incoming = &string[conflict.incoming.clone()];
                    let kept = match side {
                        Side::Current => current.to_owned(),
                        Side::Incoming => incoming.to_owned(),
                        Side::Both => format!("{current}{incoming}"),
                    };
                    (conflict.range(), kept)
                })
                .collect();
            edits
        };
        let Some((first, _)) = edits.first() else {
            let message = match which {
                Which::All => NO_CONFLICTS,
                Which::At(_) => NOT_IN_CONFLICT,
            };
            return toast::show(cx, Toast::info(message.to_owned()));
        };
        let at = first.start;
        editor.update(cx, |editor, cx| {
            editor.model_mut().buffer_mut().edit(edits, Origin::User);
            editor.select_and_reveal(at..at, cx);
        });
    }
}
