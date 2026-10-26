//! Toggling comments over the rows selections touch, from the delimiters of
//! the buffer's language, as one transaction that keeps the selections on
//! the same text.

use std::ops::Range;

use fe_buffer::{Bias, ResolvedSelection};

use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, add},
};

/// A language's comment delimiters, which the host gives from the buffer's
/// language (fe-editor doesn't know languages).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CommentTokens {
    /// The token that starts a line comment, if the language has one.
    pub line: Option<&'static str>,
    /// The open and close tokens of a block comment, if the language has one.
    pub block: Option<(&'static str, &'static str)>,
}

/// Registers `editor.toggle_comment` and `editor.toggle_block_comment`,
/// which edit the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(
        registry,
        "toggle_comment",
        "Toggle Line Comment",
        true,
        |model, host| {
            if let Some(tokens) = host.comment_tokens() {
                model.toggle_comment(tokens);
            }
            Ok(Effect::Changed)
        },
    );
    add(
        registry,
        "toggle_block_comment",
        "Toggle Block Comment",
        true,
        |model, host| {
            if let Some((open, close)) = host.comment_tokens().and_then(|tokens| tokens.block) {
                model.toggle_block_comment(open, close);
            }
            Ok(Effect::Changed)
        },
    );
}

impl EditorModel {
    /// Comments out the rows selections touch, or uncomments them if all
    /// are commented already, skipping blank rows. When every touched row
    /// is blank, a line token is put at the end of each, as VS Code does. A
    /// line token is preferred to a block pair, which wraps each row on its
    /// own and leaves blank rows alone. Does nothing with neither.
    pub fn toggle_comment(&mut self, tokens: CommentTokens) {
        self.sync_display();
        let text = self.text();
        // Each non-blank row: where its text starts, where it ends without
        // trailing whitespace, and where the row ends, as offsets.
        let mut rows = Vec::new();
        // Where each blank row's content ends.
        let mut blanks = Vec::new();
        for row in self
            .blocks(false)
            .iter()
            .flat_map(|block| block.rows.clone())
        {
            let content = text.row_to_string(row);
            let content = content.trim_end_matches(['\n', '\r']);
            let trimmed = content.trim_start();
            let code = trimmed.trim_end();
            let start = text.row_start(row) + (content.len() - trimmed.len());
            if code.is_empty() {
                blanks.push(text.row_start(row) + content.len());
            } else {
                rows.push((start, start + code.len(), start + trimmed.len()));
            }
        }
        if rows.is_empty() {
            if let Some(token) = tokens.line {
                let edits = blanks
                    .into_iter()
                    .map(|at| (at..at, format!("{token} ")))
                    .collect();
                self.edit_rows_in_place(edits);
            }
            return;
        }
        let slice = |range: (usize, usize)| text.slice_to_string(range.0..range.1);
        let mut edits = Vec::new();
        if let Some(token) = tokens.line {
            let commented = rows
                .iter()
                .all(|&(start, end, _)| slice((start, end)).starts_with(token));
            if commented {
                for &(start, _, full) in &rows {
                    let after = slice((start + token.len(), full));
                    let len = token.len() + usize::from(after.starts_with(' '));
                    edits.push((start..start + len, String::new()));
                }
            } else {
                // The smallest indentation, in chars, so the markers line up.
                let indent = |&(start, ..): &(usize, usize, usize)| {
                    let row = text.offset_to_point(start).row;
                    text.slice_to_string(text.row_start(row)..start)
                        .chars()
                        .count()
                };
                let width = rows.iter().map(indent).min().unwrap_or(0);
                for &row in &rows {
                    let line = text.offset_to_point(row.0).row;
                    let line_start = text.slice_to_string(text.row_start(line)..row.0);
                    let at: usize = line_start.chars().take(width).map(char::len_utf8).sum();
                    let at = text.row_start(line) + at;
                    edits.push((at..at, format!("{token} ")));
                }
            }
        } else if let Some((open, close)) = tokens.block {
            let wrapped = rows.iter().all(|&(start, end, _)| {
                let code = slice((start, end));
                code.len() >= open.len() + close.len()
                    && code.starts_with(open)
                    && code.ends_with(close)
            });
            for &(start, end, full) in &rows {
                if wrapped {
                    let code = slice((start, end));
                    let inner = &code[open.len()..code.len() - close.len()];
                    let before = usize::from(inner.starts_with(' '));
                    let after = usize::from(inner.len() > before && inner.ends_with(' '));
                    edits.push((start..start + open.len() + before, String::new()));
                    edits.push((end - close.len() - after..end, String::new()));
                } else {
                    edits.push((start..start, format!("{open} ")));
                    edits.push((full..full, format!(" {close}")));
                }
            }
        }
        self.edit_rows_in_place(edits);
    }
}

/// One edit of a block-comment toggle: `range` becomes `text`. An insertion
/// that `opens` moves offsets at its position; one that closes doesn't.
struct BlockEdit {
    range: Range<usize>,
    text: String,
    opens: bool,
}

/// Where `offset` is after `edits`, which are in order and don't overlap.
fn map_block_offset(
    edits: &[BlockEdit],
    offset: usize,
    start_of_range: bool,
    empty: bool,
) -> usize {
    let mut mapped = offset as isize;
    for edit in edits {
        let (start, end) = (edit.range.start, edit.range.end);
        if start == end {
            let at = if edit.opens {
                start_of_range || empty
            } else {
                start_of_range && !empty
            };
            if start < offset || (start == offset && at) {
                mapped += edit.text.len() as isize;
            }
        } else if end <= offset {
            mapped -= (end - start) as isize;
        } else if start < offset {
            mapped -= (offset - start) as isize;
        }
    }
    mapped as usize
}

impl EditorModel {
    /// Wraps each selection in the block pair, or takes the pair off if the
    /// selection holds it or sits inside it. An empty selection acts on its
    /// row's text, without the indent. Selections whose rows were already
    /// edited by an earlier one are left to it.
    pub fn toggle_block_comment(&mut self, open: &str, close: &str) {
        self.sync_display();
        let text = self.text().clone();
        let selections = self.selections();
        let mut edits: Vec<BlockEdit> = Vec::new();
        for selection in &selections {
            let Range { start, end } = selection.range;
            let row = text.offset_to_point(start).row;
            let row_start = text.row_start(row);
            let content = text.row_to_string(row);
            let content = content.trim_end_matches(['\n', '\r']);
            let mut found = if start == end {
                pair_around(content, start - row_start, open, close)
                    .map(|(o, c)| (row_start + o, row_start + c))
            } else {
                let inner = text.slice_to_string(start..end);
                let lead = inner.len() - inner.trim_start().len();
                let code = inner.trim();
                if code.len() >= open.len() + close.len()
                    && code.starts_with(open)
                    && code.ends_with(close)
                {
                    let from = start + lead;
                    Some((from, from + code.len() - close.len()))
                } else {
                    let end_row = text.offset_to_point(end).row;
                    let before = text.slice_to_string(row_start..start);
                    let after_end = text.row_start(end_row)
                        + text
                            .row_to_string(end_row)
                            .trim_end_matches(['\n', '\r'])
                            .len();
                    let after = text.slice_to_string(end..after_end);
                    let before = before.trim_end();
                    let after_trimmed = after.trim_start();
                    (before.ends_with(open) && after_trimmed.starts_with(close)).then(|| {
                        (
                            start
                                - (text.slice_to_string(row_start..start).len() - before.len())
                                - open.len(),
                            end + (after.len() - after_trimmed.len()),
                        )
                    })
                }
            };
            let mut these = Vec::new();
            if let Some((o, c)) = found.take() {
                let after_open = o + open.len();
                let space = text
                    .slice_to_string(after_open..text.clip_offset(after_open + 1, Bias::Right))
                    == " ";
                let open_end = after_open + usize::from(space && after_open < c);
                let space = c > open_end
                    && text.slice_to_string(text.clip_offset(c - 1, Bias::Left)..c) == " ";
                let close_start = c - usize::from(space);
                these.push(BlockEdit {
                    range: o..open_end,
                    text: String::new(),
                    opens: true,
                });
                these.push(BlockEdit {
                    range: close_start..c + close.len(),
                    text: String::new(),
                    opens: false,
                });
            } else {
                let (from, to) = if start == end {
                    let trimmed = content.trim_start();
                    let code = trimmed.trim_end();
                    let from = row_start + content.len() - trimmed.len();
                    (from, from + code.len())
                } else {
                    let inner = text.slice_to_string(start..end);
                    let lead = inner.len() - inner.trim_start().len();
                    (start + lead, start + lead + inner.trim().len())
                };
                if from == to {
                    continue;
                }
                these.push(BlockEdit {
                    range: from..from,
                    text: format!("{open} "),
                    opens: true,
                });
                these.push(BlockEdit {
                    range: to..to,
                    text: format!(" {close}"),
                    opens: false,
                });
            }
            if edits
                .last()
                .is_some_and(|last| these[0].range.start < last.range.end)
            {
                continue;
            }
            edits.extend(these);
        }
        let resolved = selections
            .iter()
            .map(|selection| {
                let empty = selection.range.is_empty();
                ResolvedSelection {
                    range: map_block_offset(&edits, selection.range.start, true, empty)
                        ..map_block_offset(&edits, selection.range.end, false, empty),
                    reversed: selection.reversed,
                }
            })
            .collect();
        let edits = edits.into_iter().map(|e| (e.range, e.text)).collect();
        self.apply_row_edits(edits, resolved, Vec::new());
    }
}

/// The offsets in `row` of an `open` and the `close` after it that hold
/// `at`, the innermost such pair that ends last.
fn pair_around(row: &str, at: usize, open: &str, close: &str) -> Option<(usize, usize)> {
    row.rmatch_indices(open)
        .map(|(o, _)| o)
        .filter(|&o| o <= at)
        .find_map(|o| {
            let c = o + open.len() + row.get(o + open.len()..)?.find(close)?;
            (c + close.len() >= at).then_some((o, c))
        })
}

#[cfg(test)]
#[path = "comment_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "block_comment_tests.rs"]
mod block_comment_tests;
