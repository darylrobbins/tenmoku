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
//! `u` and `<C-r>` leave the U line as it is, as nvim's do (they don't save
//! a line): the buffer's undo and redo batches are told from fresh edits by
//! their [`EditKind`].
//!
//! The row is a number, as in nvim: rows added or removed above it don't
//! move it. What clears it is what nvim's `open_line` and linewise delete
//! clear it with: a change that breaks a row (`o`, `O`, `<CR>`) or deletes
//! whole rows (`dd`, `dj`). Both are read from a batch's edits: one that
//! inserts a line break and only blanks, or one that takes out whole rows
//! (from a row's start through a line break, or a break and the rows to
//! the end). A characterwise delete across rows (`vjd`) leaves it. What the
//! edits can't tell apart, the last repeatable change does: putting rows
//! (`p`, whose blank row looks like `O`'s) and joining them (`J`, `gJ`,
//! whose break removed looks like a blank row's `dd`) leave it. With no U
//! line, or its row past the last, `U` fails with nothing changed.
//!
//! The batches are read from a subscription to the buffer that [`follow`]
//! starts and drains before each command (see `marks::follow`), so a run is
//! seen however its changes were made. A clone of the state shares the
//! whole tracker (subscription, text and U line), as it shares the buffer,
//! so what either does, the other sees.

use std::{
    fmt,
    ops::Range,
    sync::{Arc, Mutex, MutexGuard},
};

use fe_buffer::{BufferEvent, Edit, EditKind, Subscription, TransactionId};
use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::{EditorModel, commands::origin};
use fe_text::{Bias, Text};

use crate::{
    Change, Pending, Step, VimCommands, VimKey, VimState, VimTarget,
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

/// The U line, and what keeps it up to date: one tracker, which a clone of
/// the state shares.
#[derive(Clone, Debug, Default)]
pub(crate) struct LineUndo(Arc<Mutex<Tracker>>);

#[derive(Debug, Default)]
struct Tracker {
    events: Option<Events>,
    /// The text after the last batch read.
    text: Option<Text>,
    line: Option<Line>,
    /// The transaction `U` made, whose batch isn't a change to follow.
    own: Option<TransactionId>,
    /// `VimState::changes` as of the last read.
    changes: u64,
}

struct Events(Subscription);

impl fmt::Debug for Events {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Events")
    }
}

impl LineUndo {
    fn lock(&self) -> MutexGuard<'_, Tracker> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
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
    let normal = ContextPredicate::parse(kiln_l10n::verbatim!("editor && vim.mode == normal"))
        .expect("vim's context predicate parses");
    registry
        .register(
            kiln_l10n::verbatim!("vim.line_undo"),
            &crate::l10n::command_title("vim.line_undo"),
            |target: &mut VimTarget<'_>, actor: &Actor| run(target, actor),
        )
        .expect("vim command IDs are valid and distinct")
        .description(&crate::l10n::command_description("vim.line_undo"))
        .context(normal)
        .mutates_buffer(true);
}

/// Starts following `model`'s buffer if this hasn't, and reads the batches
/// since the last call.
pub(crate) fn follow(state: &mut VimState, model: &mut EditorModel) {
    let mut tracker = state.line_undo.lock();
    if tracker.events.is_none() {
        tracker.events = Some(Events(model.buffer_mut().subscribe()));
        tracker.text = Some(model.snapshot().text().clone());
        tracker.changes = state.changes;
    }
    // What the last command set to repeat, if it did: the edits of a put
    // or a join can look like an `O` or a `dd`.
    let keeps = tracker.changes != state.changes
        && matches!(
            state.last_change.as_ref().map(|last| &last.change),
            Some(Change::Put(_) | Change::Join(_))
        );
    tracker.changes = state.changes;
    tracker.read(&state.carets, keeps);
}

impl Tracker {
    fn read(&mut self, carets: &Carets, keeps: bool) {
        let Some(events) = &self.events else {
            return;
        };
        for event in events.0.drain() {
            let BufferEvent::Edited {
                transaction,
                kind,
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
            // Undo and redo save no line; `U`'s own batch isn't a change.
            if kind != EditKind::Edit || self.own == Some(transaction) {
                continue;
            }
            self.change(transaction, &edits, &before, &text, carets, keeps);
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
        keeps: bool,
    ) {
        let slice = |text: &Text, range: &Range<usize>| text.slice_to_string(range.clone());
        let broken = |text: &str| text.contains('\n');
        if edits
            .iter()
            .any(|edit| broken(&slice(before, &edit.old)) || broken(&slice(after, &edit.new)))
        {
            let clears = !keeps
                && edits.iter().any(|edit| {
                    let (old, new) = (slice(before, &edit.old), slice(after, &edit.new));
                    let blank = |text: &str| text.chars().all(char::is_whitespace);
                    // A row opened or split, or whole rows deleted.
                    (broken(&new) && blank(&old) && blank(&new))
                        || (new.is_empty() && whole_rows(before, &edit.old))
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

/// Whether `old` takes out whole rows of `text`: from a row's start through
/// a line break, or the break before the last rows through the end.
fn whole_rows(text: &Text, old: &Range<usize>) -> bool {
    let starts_row = old.start == 0 || text.char_at(old.start - 1) == Some('\n');
    let through_break = old.end > old.start && text.char_at(old.end - 1) == Some('\n');
    let to_end = old.end == text.len() && old.end > old.start;
    (starts_row && (through_break || to_end)) || (to_end && text.char_at(old.start) == Some('\n'))
}

/// `vim.line_undo`.
fn run(
    target: &mut VimTarget<'_>,
    actor: &Actor,
) -> Result<Dispatched<TransactionId>, CommandError> {
    crate::commands::follow(target);
    let VimTarget { model, state, .. } = target;
    let text = model.text().clone();
    let tracker = state.line_undo.clone();
    let Some(line) = tracker
        .lock()
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
    tracker.lock().own = transaction;
    // The caret goes where the run began; the next `U` goes where it was
    // now, if it was on the row.
    let after = snapshot.text();
    let offset = start + line.column.min(after.row_len(line.row) as usize);
    land(model, &snapshot, [after.clip_offset(offset, Bias::Left)]);
    tracker.lock().line = Some(Line {
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
