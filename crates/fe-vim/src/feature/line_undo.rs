//! `U` (`:help U`), a feature module; see [`crate::feature`]: undoes every
//! latest change on the last changed line, and `U` again puts them back.
//!
//! The "U line" is the row of the latest change and its text from before
//! the first of the run of changes on it. A change on another row starts a
//! new run there, as nvim's `u_saveline` does; one on the same row, `u`,
//! `<C-r>` and `U` itself included, belongs to the run. `U` replaces the
//! row's text with the kept text as a change of its own (so `u` undoes
//! it), keeping the text it replaced for the next `U`. The caret goes to
//! the column it was at when the run began, if it was on that row (else
//! column 0); the column kept for the next `U` is the caret's now, if it's
//! on the row. A count is ignored; `U` doesn't change what `.` repeats.
//!
//! The row is a number, as in nvim: rows added or removed above it don't
//! move it. What clears it is what nvim's `open_line` and linewise delete
//! clear it with: a change that breaks a row (`o`, `O`, `<CR>`) or deletes
//! whole rows (`dd`, `dj`), read from a batch's edits: one that inserts a
//! line break and only blanks, or removes a line break with text. Joining
//! rows (`J`) and putting rows (`p`) leave it. With no U line, or its row
//! past the last, `U` fails with nothing changed.
//!
//! The batches are read from a subscription to the buffer that [`follow`]
//! starts and drains before each command (see `marks::follow`), so a run is
//! seen however its changes were made. A clone of the state shares the
//! subscription and takes events from the original (known limit: it may
//! miss the start of a run).

use std::{fmt, ops::Range, sync::Arc};

use fe_buffer::{BufferEvent, Edit, Subscription, TransactionId};
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::{EditorModel, commands::origin};
use fe_text::{Bias, Text};

use crate::{
    Pending, Step, VimCommands, VimKey, VimState, VimTarget,
    feature::Feature,
    history::{Carets, pair},
    operator::{self, land},
    reducer::invocation,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    register,
    ..Feature::NONE
};

/// The U line, and what keeps it up to date.
#[derive(Clone, Debug, Default)]
pub(crate) struct LineUndo {
    events: Option<Events>,
    /// The text after the last batch read.
    text: Option<Text>,
    line: Option<Line>,
    /// The transaction `U` made, whose batch isn't a change to follow.
    own: Option<TransactionId>,
}

/// The buffer's events, shared by a clone of the state.
#[derive(Clone)]
struct Events(Arc<Subscription>);

impl fmt::Debug for Events {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Events")
    }
}

#[derive(Clone, Debug)]
struct Line {
    row: u32,
    /// The row's text `U` puts back.
    text: String,
    /// The byte column `U` puts the caret at.
    column: usize,
}

/// `U` in normal mode.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    if key != VimKey::Char('U') || pending.is_visual() || pending.has_operator() {
        return None;
    }
    Some(invocation("vim.line_undo", ()))
}

fn register(registry: &mut Registry<VimCommands>) {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    registry
        .register(
            "vim.line_undo",
            "Vim Line Undo",
            |target: &mut VimTarget<'_>, actor: &Actor| run(target, actor),
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Undoes every latest change on the last changed line, as vim's U does; U again puts them back. Fails with nothing changed when there's no such line.",
        )
        .context(normal)
        .mutates_buffer(true);
}

/// Starts following `model`'s buffer if this hasn't, and reads the batches
/// since the last call.
pub(crate) fn follow(state: &mut VimState, model: &mut EditorModel) {
    let line_undo = &mut state.line_undo;
    if line_undo.events.is_none() {
        line_undo.events = Some(Events(Arc::new(model.buffer_mut().subscribe())));
        line_undo.text = Some(model.snapshot().text().clone());
    }
    line_undo.read(&state.carets);
}

impl LineUndo {
    fn read(&mut self, carets: &Carets) {
        let Some(events) = &self.events else {
            return;
        };
        for event in events.0.drain() {
            let BufferEvent::Edited {
                transaction,
                edits,
                text,
                ..
            } = event
            else {
                continue;
            };
            let Some(before) = self.text.replace(text.clone()) else {
                continue;
            };
            if self.own == Some(transaction) {
                continue;
            }
            self.change(transaction, &edits, &before, &text, carets);
        }
    }

    /// One batch of `edits`, taking `before` to `after`.
    fn change(
        &mut self,
        transaction: TransactionId,
        edits: &[Edit],
        before: &Text,
        after: &Text,
        carets: &Carets,
    ) {
        let slice = |text: &Text, range: &Range<usize>| text.slice_to_string(range.clone());
        let broken = |text: &str| text.contains('\n');
        if edits
            .iter()
            .any(|edit| broken(&slice(before, &edit.old)) || broken(&slice(after, &edit.new)))
        {
            let clears = edits.iter().any(|edit| {
                let (old, new) = (slice(before, &edit.old), slice(after, &edit.new));
                let blank = |text: &str| text.chars().all(char::is_whitespace);
                // A row opened or split, or rows deleted (not joined).
                (broken(&new) && blank(&old) && blank(&new))
                    || (broken(&old) && !broken(&new) && !blank(&old))
            });
            if clears {
                self.line = None;
            }
            return;
        }
        // Only the first of a batch's rows counts, as nvim's `:s` over a
        // range leaves its first.
        for edit in edits.iter().take(1) {
            let at = before.offset_to_point(edit.old.start);
            if self.line.as_ref().is_some_and(|line| line.row == at.row) {
                continue;
            }
            // The caret before the change, if it was on the row; recorded
            // for the commands that record carets, else where it began.
            let column = match carets.get(transaction).and_then(<[_]>::first) {
                Some(&(row, column)) => {
                    if row == at.row {
                        column
                    } else {
                        0
                    }
                }
                None => at.column as usize,
            };
            self.line = Some(Line {
                row: at.row,
                text: before.row_to_string(at.row),
                column,
            });
        }
    }
}

/// `vim.line_undo`.
fn run(
    target: &mut VimTarget<'_>,
    actor: &Actor,
) -> Result<Dispatched<TransactionId>, CommandError> {
    crate::commands::follow(target);
    let VimTarget { model, state, .. } = target;
    let text = model.text().clone();
    let Some(line) = state
        .line_undo
        .line
        .clone()
        .filter(|line| line.row < text.row_count())
    else {
        state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    };
    let caret = pair(&text, model.primary_selection().head());
    let start = text.row_start(line.row);
    let end = start + text.row_len(line.row) as usize;
    let replaced = text.row_to_string(line.row);
    let edit = vec![(start..end, line.text.clone())];
    let (transaction, snapshot) = operator::apply(model, edit, origin(actor));
    if let Some(id) = transaction {
        state.carets.record(id, vec![caret]);
    }
    state.line_undo.own = transaction;
    // The caret goes where the run began; the next `U` goes where it was
    // now, if it was on the row.
    let after = snapshot.text();
    let offset = start + line.column.min(after.row_len(line.row) as usize);
    land(model, &snapshot, [after.clip_offset(offset, Bias::Left)]);
    state.line_undo.line = Some(Line {
        row: line.row,
        text: replaced,
        column: match caret.0 == line.row {
            true => caret.1,
            false => line.column,
        },
    });
    state.goals = None;
    Ok(match transaction {
        Some(id) => Dispatched::new(Effect::Changed).with_transactions(vec![id]),
        None => Dispatched::new(Effect::Changed),
    })
}
