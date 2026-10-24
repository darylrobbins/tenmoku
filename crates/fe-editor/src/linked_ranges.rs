//! Linked ranges, as a host sets them from a language server's
//! `textDocument/linkedEditingRange` (VS Code's `editor.linkedEditing`: the
//! opening tag of an HTML element renamed as its closing tag is typed).
//!
//! [`EditorModel::set_linked_ranges`] starts a session over ranges of the
//! text. While the single selection is inside one of them (touching its
//! ends counts), an insert or delete wholly inside that range (typing,
//! backspace, delete-word, paste, cut) is copied into every other range at
//! the same offset within the range, so ranges of unequal text still work.
//! The copies are in the edit's batch, so in its undo step and its run of
//! typing, as a snippet's mirrors are. Every range follows the edit: it
//! grows with text typed at its ends and shifts for one before it. Vim's
//! insert mode edits through the same path and mirrors too.
//!
//! The session ends, dropping the ranges and mirroring nothing, when:
//! - an edit falls outside every range or crosses a range's boundary;
//! - the selection leaves the ranges, or moves from one to a place outside
//!   (moving into another linked range keeps it);
//! - a second selection is added;
//! - the edited range's new text has a character other than alphanumerics,
//!   `_`, `-`, `.` and `:` (the server's `wordPattern` is ignored in this
//!   version);
//! - the text changes any way but an edit made here (an undo or redo, or
//!   another model's edit);
//! - [`EditorModel::set_linked_ranges`] or
//!   [`EditorModel::clear_linked_ranges`] is called.
//!
//! A snippet session takes precedence: while one is active
//! `set_linked_ranges` does nothing, and one starting ends this session.
//! Ranges that overlap are refused, as are fewer than two, out-of-bounds
//! ones, and a model with several selections. Of ranges that touch, an edit
//! at the boundary belongs to the first that holds it and the selection.

use std::ops::Range;

use fe_buffer::{Bias, Buffer, ResolvedSelection, TransactionId, Version};

use crate::EditorModel;

/// The ranges linked, as of `version`.
pub(crate) struct LinkedSession {
    version: Version,
    ranges: Vec<Range<usize>>,
    /// The ranges once the edits last [`EditorModel::mirror_linked_edits`]
    /// planned are made, from the text at `version`.
    planned: Option<Vec<Range<usize>>>,
}

fn is_word(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':')
}

/// The index of the range holding `range`, touching ends counting.
fn holding(ranges: &[Range<usize>], range: &Range<usize>) -> Option<usize> {
    ranges
        .iter()
        .position(|r| r.start <= range.start && range.end <= r.end)
}

impl EditorModel {
    /// Links `ranges`, offsets in [`EditorModel::text`]: see the
    /// [module](self). Ends any session going on; with fewer than two,
    /// overlapping, or out-of-bounds ranges, several
    /// selections, or a snippet session, starts none.
    pub fn set_linked_ranges(&mut self, mut ranges: Vec<Range<usize>>) {
        self.sync_display();
        self.linked = None;
        ranges.sort_by_key(|range| (range.start, range.end));
        let text = self.text();
        let fine = ranges.len() >= 2
            && self.snippet.is_none()
            && self.selections().len() == 1
            && ranges.iter().all(|r| {
                r.start <= r.end
                    && r.end <= text.len()
                    && text.clip_offset(r.start, Bias::Left) == r.start
                    && text.clip_offset(r.end, Bias::Left) == r.end
            })
            && ranges.windows(2).all(|pair| pair[0].end <= pair[1].start);
        if !fine {
            return;
        }
        self.linked = Some(LinkedSession {
            version: self.snapshot.version(),
            ranges,
            planned: None,
        });
        self.follow_linked();
    }

    /// Ends the linked ranges session, if there's one.
    pub fn clear_linked_ranges(&mut self) {
        self.linked = None;
    }

    /// The ranges linked, in the text as of the last catch-up, by start.
    pub fn linked_ranges(&self) -> Option<&[Range<usize>]> {
        self.linked.as_ref().map(|session| &session.ranges[..])
    }

    /// Ends the session if the text changed other than by an edit made
    /// here, or the selection isn't inside the ranges. Runs whenever the
    /// text is caught up with or the selections change.
    pub(crate) fn follow_linked(&mut self) {
        let Some(session) = &self.linked else {
            return;
        };
        let selections = self.selections();
        let held = self.snippet.is_none()
            && session.version == self.snapshot.version()
            && selections.len() == 1
            && holding(&session.ranges, &selections[0].range).is_some();
        if !held {
            self.linked = None;
        }
    }

    /// `edits`, sorted and apart, about to be made from the text as of the
    /// last catch-up, with `selections`, offsets in the text they make, and
    /// the same edits made in the other ranges, if all lie in one. In one
    /// batch with the edits, they're in its undo step. Ends the session
    /// instead if they don't, or leave a character that isn't a word's.
    pub(crate) fn mirror_linked_edits(
        &mut self,
        edits: Vec<(Range<usize>, String)>,
        selections: Vec<ResolvedSelection>,
    ) -> (Vec<(Range<usize>, String)>, Vec<ResolvedSelection>) {
        let Some(session) = &mut self.linked else {
            return (edits, selections);
        };
        session.planned = None;
        if edits.is_empty() || session.version != self.snapshot.version() {
            return (edits, selections);
        }
        let ranges = session.ranges.clone();
        // Ranges that touch may both hold the edits: the one the selection
        // is in, else the first, owns them.
        let holds = |ix: usize, within: &Range<usize>| {
            ranges[ix].start <= within.start && within.end <= ranges[ix].end
        };
        let selected = self.selections.resolve(&self.snapshot);
        let owners: Vec<usize> = (0..ranges.len())
            .filter(|&ix| edits.iter().all(|(old, _)| holds(ix, old)))
            .collect();
        let owner = (owners.iter().copied())
            .find(|&ix| selected.iter().all(|sel| holds(ix, &sel.range)))
            .or(owners.first().copied());
        let Some(owner) = owner else {
            self.linked = None;
            return (edits, selections);
        };
        let home = &ranges[owner];
        // The range's new text.
        let text = self.snapshot.text();
        let mut now = String::new();
        let mut at = home.start;
        for (old, new) in &edits {
            now.push_str(&text.slice_to_string(at..old.start));
            now.push_str(new);
            at = old.end;
        }
        now.push_str(&text.slice_to_string(at..home.end));
        if !now.chars().all(is_word) {
            self.linked = None;
            return (edits, selections);
        }
        // Each range's change in length, and the copies.
        let mut deltas = vec![0isize; ranges.len()];
        deltas[owner] = edits
            .iter()
            .map(|(old, new)| new.len() as isize - old.len() as isize)
            .sum();
        let mut copies: Vec<(usize, Range<usize>, String)> = Vec::new();
        for (ix, range) in ranges.iter().enumerate().filter(|(ix, _)| *ix != owner) {
            for (old, new) in &edits {
                let clamp = |offset: usize| {
                    let inner = (offset - home.start).min(range.len());
                    text.clip_offset(range.start + inner, Bias::Left)
                };
                let (start, end) = (clamp(old.start), clamp(old.end));
                deltas[ix] += new.len() as isize - (end - start) as isize;
                copies.push((ix, start..end, new.clone()));
            }
        }
        // The ranges after the edits.
        let mut before = 0isize;
        let planned = (ranges.iter().zip(&deltas))
            .map(|(range, delta)| {
                let start = range.start.checked_add_signed(before).expect("in the text");
                before += delta;
                start..range.end.checked_add_signed(before).expect("in the text")
            })
            .collect();
        // The selections move past the copies before them, where each lands
        // among the edits asked for.
        let asked = edits
            .iter()
            .map(|(old, new)| new.len() as isize - old.len() as isize);
        // An edit asked for is before a copy at its offset if its range is.
        let asked_before = |offset: usize, copy_of: usize| -> isize {
            (edits.iter().zip(asked.clone()))
                .filter(|((old, _), _)| {
                    old.start < offset || (old.start == offset && owner < copy_of)
                })
                .map(|(_, delta)| delta)
                .sum()
        };
        let shifts: Vec<(usize, isize)> = copies
            .iter()
            .map(|(ix, old, new)| {
                let at = old
                    .start
                    .checked_add_signed(asked_before(old.start, *ix))
                    .expect("sorted");
                (at, new.len() as isize - old.len() as isize)
            })
            .collect();
        let shift = |offset: usize| {
            let moved: isize = (shifts.iter())
                .filter(|(at, _)| *at < offset)
                .map(|(_, delta)| delta)
                .sum();
            offset.checked_add_signed(moved).expect("copies before it")
        };
        let selections = (selections.into_iter())
            .map(|selection| ResolvedSelection {
                range: shift(selection.range.start)..shift(selection.range.end),
                reversed: selection.reversed,
            })
            .collect();
        // By offset, then range, so text at a boundary two ranges share goes
        // in range by range.
        let mut all: Vec<(usize, Range<usize>, String)> = (edits.into_iter())
            .map(|(old, new)| (owner, old, new))
            .collect();
        all.extend(copies);
        all.sort_by_key(|(ix, old, _)| (old.start, old.end, *ix));
        let all = all.into_iter().map(|(_, old, new)| (old, new)).collect();
        if let Some(session) = &mut self.linked {
            session.planned = Some(planned);
        }
        (all, selections)
    }

    /// Brings the session up to the buffer after an edit, to the ranges
    /// [`EditorModel::mirror_linked_edits`] planned. Ends it if the text
    /// changed otherwise. Run with the buffer as the edit left it, before
    /// catching up.
    pub(crate) fn sync_linked_ranges(&mut self, buffer: &Buffer, _id: Option<TransactionId>) {
        let Some(session) = &mut self.linked else {
            return;
        };
        let version = buffer.version();
        if version == session.version {
            session.planned = None;
            return;
        }
        match session.planned.take() {
            Some(ranges) => {
                session.ranges = ranges;
                session.version = version;
            }
            None => self.linked = None,
        }
    }
}

#[cfg(test)]
#[path = "linked_ranges_tests.rs"]
mod tests;
