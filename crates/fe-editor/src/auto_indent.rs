//! Indentation by language as lines break and closers are typed: a new row
//! takes the indentation the [`EditorHost`] suggests from the buffer's
//! syntax, and a `}` typed at a row's start lines the row up with its
//! opener's.
//!
//! The host answers from the text as the edit leaves it, so each lands in
//! two batches of one undo step: the edit as without syntax, then the
//! indentation the host gives for it.

use std::{collections::HashMap, ops::Range};

use fe_buffer::{Buffer, ResolvedSelection, TransactionId};

use crate::{EditorModel, IndentStyle, caret, chars_backward, chars_forward, commands::EditorHost};

/// The indentation an [`EditorHost`] suggests for a row: fe-syntax's
/// suggestion, which fe-editor doesn't depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndentSuggestion {
    /// `basis_row`'s indentation plus `delta` levels (fewer when negative).
    Relative { basis_row: u32, delta: i32 },
    /// No indentation.
    Zero,
}

impl EditorModel {
    /// Replaces each selection with a line break, indenting the new row as
    /// `host` suggests for the buffer's language, in the buffer's indent
    /// unit ([`EditorModel::indent_unit`]); where it has no suggestion, the
    /// row's indentation is copied, as [`EditorModel::newline`] does.
    ///
    /// At a caret between an opener and its closer, from the host's pairs
    /// (`{|}`), the closer goes to a row of its own at the opener's
    /// indentation, and the caret to a row between, indented as suggested,
    /// or a level in from the opener without a suggestion.
    ///
    /// One undo step, which puts back the caret where it was; redoing puts
    /// one at the end of each new row's indentation.
    pub fn newline_in(&mut self, host: &dyn EditorHost) {
        self.sync_display();
        let unit = self.indent_unit();
        let mut edits = Vec::new();
        let mut carets = Vec::new();
        let mut delta = 0isize;
        let mut splits = Vec::new();
        for selection in self.selections() {
            let range = selection.range;
            let row = self.row_span(range.start);
            let indent_len = self.indent_len(&row).min(range.start - row.start);
            let indent = self
                .text()
                .slice_to_string(row.start..row.start + indent_len);
            let mut text = format!("\n{indent}");
            let mut caret_len = text.len();
            let split = range.is_empty() && self.between_pair(range.start, host.auto_pairs());
            if split {
                text.push_str(&unit);
                caret_len = text.len();
                text.push('\n');
                text.push_str(&indent);
            }
            splits.push(split);
            let start = range
                .start
                .checked_add_signed(delta)
                .expect("sorted selections");
            carets.push(caret(start + caret_len));
            delta += text.len() as isize - range.len() as isize;
            edits.push((range, text));
        }
        let (_, transaction) = self.apply_through(carets, |model, buffer| {
            model.edit_typed(buffer, edits, None)
        });
        if let Some(transaction) = transaction {
            self.indent_new_rows(host, transaction, &splits);
        }
    }

    /// Indents each caret's row, which [`EditorModel::newline_in`] has just
    /// made, as `host` suggests, in `transaction`. A row's suggestion is
    /// relative to its basis row as this indents it, if it's another new
    /// row. Where `splits` says a caret split a pair, the closer's row, the
    /// next, goes to the opener's row's indentation, as this leaves it.
    ///
    /// The closers go in a batch of their own, then the caret rows, in one
    /// where every caret ends an edit, so redoing puts it back: a
    /// re-indented row's edit replaces its indentation, a caret at its end,
    /// and any other's puts back the text from the line break before to its
    /// caret. Nothing is edited if no row's indentation changes and no pair
    /// was split, whose edit ends past its caret.
    fn indent_new_rows(
        &mut self,
        host: &dyn EditorHost,
        transaction: TransactionId,
        splits: &[bool],
    ) {
        let mut planned = HashMap::new();
        let mut reindents = Vec::new();
        let mut closers = Vec::new();
        for (selection, &split) in self.selections().iter().zip(splits) {
            let row = self.row_span(selection.range.start);
            let current = self
                .text()
                .slice_to_string(row.start..row.start + self.indent_len(&row));
            let number = self.text().offset_to_point(row.start).row;
            // A suggested indentation takes the caret to its end, past any
            // blanks the line break carried down after it.
            let at_end = selection.range.start == row.start + current.len();
            let indent = host
                .suggest_indent(self.snapshot(), number)
                .map(|suggestion| self.suggested_indent(suggestion, &planned))
                .filter(|indent| *indent != current || !at_end);
            reindents.push(indent.clone());
            let indent = indent.unwrap_or(current);
            planned.insert(number, indent);
            if split {
                let opener = self.indent_of(number - 1, &planned);
                let closer = self.row_span(self.text().row_start(number + 1));
                let current = self.indent_len(&closer);
                if self
                    .text()
                    .slice_to_string(closer.start..closer.start + current)
                    != opener
                {
                    closers.push((closer.start..closer.start + current, opener.clone()));
                }
                // Later rows may count from the closer's.
                planned.insert(number + 1, opener);
            }
        }
        if !splits.contains(&true) && reindents.iter().all(Option::is_none) {
            return;
        }
        let in_transaction = |model: &mut Self, buffer: &mut Buffer, edits: Vec<_>| {
            // The transaction was made just now, so it's still there.
            buffer
                .edit_in(transaction, edits.iter().cloned())
                .unwrap_or_else(|_| buffer.edit(edits, model.origin.clone()))
        };
        if !closers.is_empty() {
            let selections = self
                .selections()
                .into_iter()
                .map(|selection| ResolvedSelection {
                    range: moved(&closers, selection.range.start)
                        ..moved(&closers, selection.range.end),
                    reversed: selection.reversed,
                })
                .collect();
            self.apply_through(selections, |model, buffer| {
                in_transaction(model, buffer, closers)
            });
        }
        let edits: Vec<(Range<usize>, String)> = (self.selections().iter().zip(reindents))
            .map(|(selection, reindent)| {
                let offset = selection.range.start;
                let row = self.row_span(offset);
                match reindent {
                    Some(indent) => (
                        row.start - 1..row.start + self.indent_len(&row),
                        format!("\n{indent}"),
                    ),
                    None => (
                        row.start - 1..offset,
                        self.text().slice_to_string(row.start - 1..offset),
                    ),
                }
            })
            .collect();
        let carets = ends(&edits);
        self.apply_through(carets, |model, buffer| in_transaction(model, buffer, edits));
    }

    /// Types `ch` as [`EditorModel::insert_typed`] does, closing from
    /// `host`'s pairs. Where that leaves `ch` the first non-blank of its
    /// row and `host` says the row lines up with an earlier one
    /// ([`EditorHost::closer_outdents`]), as after a `}`, the row is
    /// re-indented to that row's indentation, as it's re-indented here if
    /// it's another such row, in the transaction the char went into: one
    /// undo takes out both. The run of typing ends there.
    ///
    /// Every caret after a typed `ch` ends an edit of the re-indent, which
    /// puts back the char if nothing else, so redoing puts it back.
    pub fn insert_typed_in(&mut self, ch: char, host: &dyn EditorHost) {
        // The char's transaction, noted as a command's are.
        let outer = self.transactions.replace(Vec::new());
        self.insert_typed(ch, host.auto_pairs());
        let produced = std::mem::replace(&mut self.transactions, outer).unwrap_or_default();
        if let Some(transactions) = &mut self.transactions {
            for &id in &produced {
                if transactions.last() != Some(&id) {
                    transactions.push(id);
                }
            }
        }
        let Some(&transaction) = produced.last() else {
            return;
        };
        if ch.is_whitespace() {
            return;
        }
        let len = ch.len_utf8();
        let mut planned = HashMap::new();
        let mut edits: Vec<(Range<usize>, String)> = Vec::new();
        let mut changed = false;
        for selection in self.selections() {
            let head = selection.head();
            let row = self.row_span(head);
            let typed = selection.range.is_empty()
                && head >= row.start + len
                && chars_backward(self.text(), row.start..head).next() == Some((head - len, ch));
            if !typed {
                continue;
            }
            let first = row.start + self.indent_len(&row);
            let number = self.text().offset_to_point(row.start).row;
            let basis = (first + len == head)
                .then(|| host.closer_outdents(self.snapshot(), number))
                .flatten()
                .filter(|&basis| basis < number);
            let indent = basis.map(|basis| self.indent_of(basis, &planned));
            match indent {
                Some(indent) if self.text().slice_to_string(row.start..first) != indent => {
                    changed = true;
                    edits.push((row.start..head, format!("{indent}{ch}")));
                    planned.insert(number, indent);
                }
                _ => edits.push((head - len..head, ch.to_string())),
            }
        }
        if !changed {
            return;
        }
        let selections = self
            .selections()
            .into_iter()
            .map(|selection| ResolvedSelection {
                range: moved(&edits, selection.range.start)..moved(&edits, selection.range.end),
                reversed: selection.reversed,
            })
            .collect();
        self.end_typing();
        self.apply_through(selections, |model, buffer| {
            buffer
                .edit_in(transaction, edits.iter().cloned())
                .unwrap_or_else(|_| buffer.edit(edits, model.origin.clone()))
        });
    }

    /// Whether `offset` is between an opener and its closer from `pairs`,
    /// leaving out quotes, whose opener is their closer.
    fn between_pair(&self, offset: usize, pairs: &[(char, char)]) -> bool {
        let row = self.row_span(offset);
        let text = self.text();
        let before = chars_backward(text, row.start..offset).next();
        let after = chars_forward(text, offset..row.end).next();
        let (Some((_, before)), Some((_, after))) = (before, after) else {
            return false;
        };
        pairs
            .iter()
            .any(|&(open, close)| open != close && open == before && close == after)
    }

    /// The indentation `suggestion` gives, in the buffer's indent unit: a
    /// level deeper is the basis row's indentation and the unit; a level
    /// out is that many fewer columns, in tabs as far as they go if the
    /// buffer indents with tabs. The basis row's indentation is as
    /// `planned`, if there.
    fn suggested_indent(
        &self,
        suggestion: IndentSuggestion,
        planned: &HashMap<u32, String>,
    ) -> String {
        let IndentSuggestion::Relative { basis_row, delta } = suggestion else {
            return String::new();
        };
        let basis = self.indent_of(basis_row, planned);
        if delta >= 0 {
            return basis + &self.indent_unit().repeat(delta.unsigned_abs() as usize);
        }
        let tab_size = self.tab_size;
        let width = match self.indent_style {
            Some(IndentStyle::Spaces(n)) => usize::from(n),
            Some(IndentStyle::Tabs) | None => tab_size,
        };
        let columns = basis.chars().fold(0, |column, ch| match ch {
            '\t' => column + tab_size - column % tab_size,
            _ => column + 1,
        });
        let columns = columns.saturating_sub(width * delta.unsigned_abs() as usize);
        match self.indent_style {
            Some(IndentStyle::Tabs) => {
                "\t".repeat(columns / tab_size) + &" ".repeat(columns % tab_size)
            }
            _ => " ".repeat(columns),
        }
    }

    /// `row`'s indentation, as `planned` if there; none past the text.
    fn indent_of(&self, row: u32, planned: &HashMap<u32, String>) -> String {
        if let Some(indent) = planned.get(&row) {
            return indent.clone();
        }
        let text = self.text();
        if row >= text.row_count() {
            return String::new();
        }
        let span = self.row_span(text.row_start(row));
        text.slice_to_string(span.start..span.start + self.indent_len(&span))
    }
}

/// A caret at the end of each of `edits`, sorted and apart, once they're
/// made.
fn ends(edits: &[(Range<usize>, String)]) -> Vec<ResolvedSelection> {
    let mut delta = 0isize;
    edits
        .iter()
        .map(|(range, text)| {
            let start = range.start.checked_add_signed(delta).expect("sorted edits");
            delta += text.len() as isize - range.len() as isize;
            caret(start + text.len())
        })
        .collect()
}

/// Where `offset` is once `edits`, sorted and apart, are made: an offset
/// inside an edit goes to its end.
fn moved(edits: &[(Range<usize>, String)], offset: usize) -> usize {
    let mut delta = 0isize;
    for (range, text) in edits {
        if offset <= range.start {
            break;
        }
        if offset < range.end {
            return range.start.checked_add_signed(delta).expect("sorted edits") + text.len();
        }
        delta += text.len() as isize - range.len() as isize;
    }
    offset
        .checked_add_signed(delta)
        .expect("offset in the text")
}

#[cfg(test)]
#[path = "auto_indent_tests.rs"]
mod tests;
