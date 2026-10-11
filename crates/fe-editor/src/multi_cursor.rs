//! More than one selection: adding carets above and below, selecting the
//! next or every occurrence of the primary selection, splitting selections
//! into lines and going back to one. Plus what the mouse needs: toggling a
//! caret and a box selection.

use std::ops::Range;

use fe_buffer::{Bias, ResolvedSelection, Text};
use fe_command::{Effect, Registry};

use crate::{
    CharClass, EditorModel, Movement, caret,
    commands::{Editor, add},
};

/// The most selections [`EditorModel::select_all_occurrences`] makes.
pub const MAX_OCCURRENCES: usize = 10_000;

/// A command's name and title, and the model method it runs.
type Command = (&'static str, &'static str, fn(&mut EditorModel));

/// Registers the multi-cursor commands. None edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    let commands: [Command; 6] = [
        (
            "add_cursor_above",
            "Add Cursor Above",
            EditorModel::add_caret_above,
        ),
        (
            "add_cursor_below",
            "Add Cursor Below",
            EditorModel::add_caret_below,
        ),
        (
            "select_next_occurrence",
            "Select Next Occurrence",
            EditorModel::select_next_occurrence,
        ),
        (
            "select_all_occurrences",
            "Select All Occurrences",
            EditorModel::select_all_occurrences,
        ),
        (
            "split_selection_into_lines",
            "Split Selection into Lines",
            EditorModel::split_selection_into_lines,
        ),
        (
            "single_selection",
            "Single Selection",
            EditorModel::single_selection,
        ),
    ];
    for (name, title, run) in commands {
        // `Changed` stands only if the selections did change.
        add(registry, name, title, false, move |model, _| {
            run(model);
            Ok(Effect::Changed)
        });
    }
}

/// What to do after a match: stop, look past it, or look from the next
/// byte (the match didn't count, so one overlapping it still may).
enum Step {
    Stop,
    Take,
    Pass,
}

impl EditorModel {
    /// Adds a caret on the display row above the topmost selection's head,
    /// at its goal column. Nothing at the first row.
    pub fn add_caret_above(&mut self) {
        self.add_caret_vertically(Movement::Up);
    }

    /// Adds a caret on the display row below the bottommost selection's
    /// head, at its goal column. Nothing at the last row.
    pub fn add_caret_below(&mut self) {
        self.add_caret_vertically(Movement::Down);
    }

    fn add_caret_vertically(&mut self, movement: Movement) {
        self.sync_display();
        let selections = self.selections();
        let mut goals = if self.goals.len() == selections.len() {
            self.goals.clone()
        } else {
            vec![None; selections.len()]
        };
        let above = movement == Movement::Up;
        let index = if above { 0 } else { selections.len() - 1 };
        let head = selections[index].head();
        let row = self.display.row_for_offset(head);
        if (above && row == 0) || (!above && row + 1 >= self.display.row_count()) {
            return;
        }
        let mut goal = goals[index];
        let offset = self.moved(head, movement, &mut goal);
        goals[index] = goal;
        let mut added = selections;
        if above {
            added.insert(0, caret(offset));
            goals.insert(0, goal);
        } else {
            added.push(caret(offset));
            goals.push(goal);
        }
        self.set_selections(added);
        // The new caret lands before (after) every other, so goals line up
        // unless it merged with one.
        if self.selections.len() == goals.len() {
            self.goals = goals;
        }
    }

    /// With a caret, selects the word around each caret (one on whitespace
    /// stays a caret). Otherwise adds the next match of the primary
    /// selection's text after the bottommost selection, wrapping once, that
    /// overlaps no selection; whole words only if the primary is a word.
    pub fn select_next_occurrence(&mut self) {
        self.sync_display();
        if self.seed_carets() {
            return;
        }
        let Some((needle, whole_word)) = self.needle() else {
            return;
        };
        let selections = self.selections();
        let text = self.text();
        let n = needle.len();
        let from = selections.last().expect("one selection").range.end;
        let first_in = |range: Range<usize>| {
            let mut found = None;
            each_match(text, needle.as_bytes(), range, |offset| {
                if overlaps(&selections, offset..offset + n)
                    || (whole_word && !is_whole_word(text, offset..offset + n))
                {
                    return Step::Pass;
                }
                found = Some(offset);
                Step::Stop
            });
            found
        };
        // Wrapping, matches starting from `from` were all looked at already.
        let found = first_in(from..text.len()).or_else(|| {
            // On a char boundary: the needle may end in a multibyte char.
            first_in(0..text.clip_offset(from + n - 1, Bias::Right))
        });
        let Some(offset) = found else {
            return;
        };
        let mut added = selections;
        added.push(ResolvedSelection {
            range: offset..offset + n,
            reversed: false,
        });
        self.set_selections(added);
    }

    /// Seeds carets as [`EditorModel::select_next_occurrence`] does, then
    /// selects every match of the primary selection's text in the buffer,
    /// up to [`MAX_OCCURRENCES`].
    pub fn select_all_occurrences(&mut self) {
        self.sync_display();
        self.seed_carets();
        let Some((needle, whole_word)) = self.needle() else {
            return;
        };
        let text = self.text();
        let n = needle.len();
        let mut matches = Vec::new();
        each_match(text, needle.as_bytes(), 0..text.len(), |offset| {
            if whole_word && !is_whole_word(text, offset..offset + n) {
                return Step::Pass;
            }
            if matches.len() == MAX_OCCURRENCES {
                log::warn!("select all occurrences: stopped at {MAX_OCCURRENCES} selections");
                return Step::Stop;
            }
            matches.push(ResolvedSelection {
                range: offset..offset + n,
                reversed: false,
            });
            Step::Take
        });
        if !matches.is_empty() {
            self.set_selections(matches);
        }
    }

    /// Turns each caret into a selection of the word around it, unless it's
    /// on whitespace. Returns whether there was a caret.
    fn seed_carets(&mut self) -> bool {
        let selections = self.selections();
        if !selections
            .iter()
            .any(|selection| selection.range.is_empty())
        {
            return false;
        }
        let seeded: Vec<_> = selections
            .into_iter()
            .map(|selection| {
                if !selection.range.is_empty() {
                    return selection;
                }
                // Whitespace at the caret, not in the word: just past a
                // word, `word_range_at` gives the word.
                let blank = self
                    .text()
                    .char_at(selection.head())
                    .is_some_and(|ch| CharClass::of(ch) == CharClass::Whitespace);
                let word = self.word_range_at(selection.head());
                if word.is_empty() || blank {
                    selection
                } else {
                    ResolvedSelection {
                        range: word,
                        reversed: false,
                    }
                }
            })
            .collect();
        self.set_selections(seeded);
        true
    }

    /// The primary selection's text, and whether it's a whole word; `None`
    /// for a caret.
    fn needle(&self) -> Option<(String, bool)> {
        let primary = self.primary_selection();
        if primary.range.is_empty() {
            return None;
        }
        let whole_word = self.word_range_at(primary.range.start) == primary.range;
        Some((self.text().slice_to_string(primary.range), whole_word))
    }

    /// Splits each selection over more than one row into one per row,
    /// without line breaks. A last row reached only at its start is dropped.
    pub fn split_selection_into_lines(&mut self) {
        self.sync_display();
        let text = self.text();
        let mut split = Vec::new();
        for selection in self.selections() {
            let start = text.offset_to_point(selection.range.start).row;
            let end = text.offset_to_point(selection.range.end).row;
            if start == end {
                split.push(selection);
                continue;
            }
            for row in start..=end {
                let row_start = text.row_start(row);
                let row_end = row_start + text.row_len(row) as usize;
                let range = if row == start {
                    selection.range.start..row_end
                } else if row == end {
                    if selection.range.end == row_start {
                        continue;
                    }
                    row_start..selection.range.end
                } else {
                    row_start..row_end
                };
                split.push(ResolvedSelection {
                    range,
                    reversed: selection.reversed,
                });
            }
        }
        self.set_selections(split);
    }

    /// Keeps only the primary selection.
    pub fn single_selection(&mut self) {
        self.sync_display();
        if self.selections.len() > 1 {
            let primary = self.primary_selection();
            self.set_selections([primary]);
        }
    }

    /// Removes the selection containing or touching `offset`, if there's
    /// more than one; otherwise adds a caret there (unless one is there).
    pub fn toggle_caret(&mut self, offset: usize) {
        self.sync_display();
        let offset = self.text().clip_offset(offset, Bias::Left);
        let mut selections = self.selections();
        let touching = selections
            .iter()
            .position(|selection| selection.range.start <= offset && offset <= selection.range.end);
        match touching {
            Some(ix) if selections.len() > 1 => {
                selections.remove(ix);
            }
            Some(_) => return,
            None => selections.push(caret(offset)),
        }
        self.set_selections(selections);
    }

    /// Selects a box: on each buffer row from `anchor`'s to `head`'s (each
    /// a row and a display column), from the anchor's column to the head's,
    /// clamped to the row's end. Rows ending before the box's left column
    /// get none; if every row does, a caret at the head.
    pub fn select_box(&mut self, anchor: (u32, usize), head: (u32, usize)) {
        self.sync_display();
        let text = self.text();
        let last_row = text.row_count() - 1;
        let (anchor_row, head_row) = (anchor.0.min(last_row), head.0.min(last_row));
        let (left, right) = (anchor.1.min(head.1), anchor.1.max(head.1));
        let reversed = head.1 < anchor.1;
        let mut selections = Vec::new();
        for row in anchor_row.min(head_row)..=anchor_row.max(head_row) {
            let expanded = self.expanded_row(row);
            if expanded.text().chars().count() < left {
                continue;
            }
            let row_start = text.row_start(row);
            let at = |column: usize, bias: Bias| {
                row_start + expanded.to_buffer(expanded.display_col_for_char_column(column), bias)
            };
            let start = at(left, Bias::Left);
            let end = if right > left {
                at(right, Bias::Right)
            } else {
                start
            };
            selections.push(ResolvedSelection {
                range: start..end,
                reversed,
            });
        }
        if selections.is_empty() {
            let expanded = self.expanded_row(head_row);
            let column = expanded.display_col_for_char_column(head.1);
            let offset = text.row_start(head_row) + expanded.to_buffer(column, Bias::Left);
            selections.push(caret(offset));
        }
        self.set_selections(selections);
    }
}

/// Whether `range` overlaps any of `selections`, sorted and disjoint.
fn overlaps(selections: &[ResolvedSelection], range: Range<usize>) -> bool {
    let ix = selections.partition_point(|selection| selection.range.end <= range.start);
    selections
        .get(ix)
        .is_some_and(|selection| selection.range.start < range.end)
}

/// Whether the match at `range`, a run of one char class, isn't part of a
/// longer run of that class in its row (as [`EditorModel::word_range_at`]
/// sees runs, so a line break ends one).
fn is_whole_word(text: &Text, range: Range<usize>) -> bool {
    let class_at = |offset: usize| {
        text.char_at(offset)
            .filter(|&ch| ch != '\n')
            .map(CharClass::of)
    };
    let class = class_at(range.start);
    let before = (range.start > 0)
        .then(|| class_at(text.clip_offset(range.start - 1, Bias::Left)))
        .flatten();
    before != class && class_at(range.end) != class
}

/// Calls `found` with the start of each match of `needle` that starts and
/// ends in `range`, in order, walking the text's chunks.
fn each_match(
    text: &Text,
    needle: &[u8],
    range: Range<usize>,
    mut found: impl FnMut(usize) -> Step,
) {
    let n = needle.len();
    if n == 0 || range.len() < n {
        return;
    }
    // The bytes not yet looked at, with what's carried over from the last
    // chunk: fewer than `n`, so a match can straddle chunks.
    let mut window: Vec<u8> = Vec::new();
    let mut window_start = range.start;
    for chunk in text.chunks_in_range(range) {
        window.extend_from_slice(chunk.as_bytes());
        let mut ix = 0;
        while ix + n <= window.len() {
            let Some(at) = window[ix..=window.len() - n]
                .iter()
                .position(|&byte| byte == needle[0])
            else {
                ix = window.len() - n + 1;
                break;
            };
            ix += at;
            if window[ix..ix + n] != *needle {
                ix += 1;
                continue;
            }
            match found(window_start + ix) {
                Step::Stop => return,
                Step::Take => ix += n,
                Step::Pass => ix += 1,
            }
        }
        window.drain(..ix);
        window_start += ix;
    }
}

#[cfg(test)]
#[path = "multi_cursor_tests.rs"]
mod tests;
