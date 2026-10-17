//! Toggling comments over the rows selections touch, from the delimiters of
//! the buffer's language, as one transaction that keeps the selections on
//! the same text.

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

/// Registers `editor.toggle_comment`, which edits the buffer.
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

#[cfg(test)]
#[path = "comment_tests.rs"]
mod tests;
