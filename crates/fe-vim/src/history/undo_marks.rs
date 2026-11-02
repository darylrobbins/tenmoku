//! The marks `u` and `<C-r>` put back.
//!
//! As nvim's undo block saves them, [`Moves`] keeps `a` to `z` and the last
//! visual extent from before each undoable change: `u` puts them back and
//! keeps those it replaced for `<C-r>`, which swaps them again. A mark the
//! record doesn't hold stays as it is. The other marks, the jumplist and the
//! changelist aren't saved.
//!
//! `commands::dispatch` records them, so every command's change has them:
//! [`before`] the command runs, then [`after`] it, for each transaction it
//! made. The first command of a transaction keeps its record: a `:g` or
//! `:normal` making several edits in one records once. An insert session
//! has the marks from before the command that opened it (`cc`'s, not
//! `<Esc>`'s); it isn't kept if it changed nothing, so it can't push a real
//! record out. A filter's output arrives outside any command, so
//! `apply_filter` and `apply_read_command` record their own ([`Moves::note`]).

use std::collections::{BTreeMap, VecDeque};

use fe_buffer::{TransactionId, Version};
use fe_editor::EditorModel;

use super::KEPT;
use crate::{VimState, VimTarget, marks::Place, visual::LastVisual};

/// The marks from before each of the last [`KEPT`] changes, by transaction.
#[derive(Clone, Debug, Default)]
pub(crate) struct Moves {
    records: VecDeque<(TransactionId, Saved)>,
    /// A session's, held apart until its transaction closes.
    session: Option<Session>,
    /// The transactions `u` and `<C-r>` stepped over in the command running:
    /// it reports them, but they aren't its changes.
    stepped: Vec<TransactionId>,
}

/// `a` to `z` and the last visual extent, as anchored as they were.
#[derive(Clone, Debug)]
pub(crate) struct Saved {
    named: BTreeMap<char, Place>,
    visual: Option<LastVisual>,
}

/// The marks from before the command that opened transaction `id`, and the
/// buffer's version then.
#[derive(Clone, Debug)]
struct Session {
    id: TransactionId,
    saved: Saved,
    version: Version,
}

/// What [`after`] needs from before a command.
pub(crate) struct Before {
    open: Option<TransactionId>,
    version: Version,
    /// `None` inside an open transaction, which the command would only join.
    saved: Option<Saved>,
}

impl Saved {
    /// As they are now; in a visual mode, `'<` and `'>` as leaving it would
    /// set them, as an operator or `:` does before it changes anything.
    pub(crate) fn of(state: &mut VimState, model: &EditorModel) -> Self {
        Self {
            named: state.marks.lowercase(),
            visual: crate::visual::extent_now(model, state),
        }
    }
}

impl Moves {
    fn holds(&self, id: TransactionId) -> bool {
        self.records.iter().any(|(at, _)| *at == id)
    }

    /// Keeps `saved` for `id`, unless it has some: those from before its
    /// first command.
    fn record(&mut self, id: TransactionId, saved: Saved) {
        if self.holds(id) {
            return;
        }
        if self.records.len() == KEPT {
            self.records.pop_front();
        }
        self.records.push_back((id, saved));
    }

    /// Keeps `saved`, from [`Saved::of`] before a change made outside any
    /// command, for its transaction.
    pub(crate) fn note(&mut self, id: TransactionId, saved: Saved) {
        self.record(id, saved);
    }
}

/// Before a command runs. One that `mutates` catches the marks up first, as
/// it would itself, so they're saved on the rows they're on now rather than
/// those of the last time they were caught up.
pub(crate) fn before(target: &mut VimTarget<'_>, mutates: bool) -> Before {
    let open = target.model.buffer().open_transaction();
    let version = target.model.buffer().version();
    let saved = open.is_none().then(|| {
        if mutates {
            crate::commands::follow(target);
        }
        Saved::of(target.state, target.model)
    });
    Before {
        open,
        version,
        saved,
    }
}

/// After a command that made the transactions `made`: each without a record
/// gets the marks from [`before`] it. One it left open (an insert session)
/// waits until it closes.
pub(crate) fn after(target: &mut VimTarget<'_>, before: Before, made: &[TransactionId]) {
    let buffer = target.model.buffer();
    let (open, version) = (buffer.open_transaction(), buffer.version());
    let moves = &mut target.state.moves;
    let stepped = std::mem::take(&mut moves.stepped);
    // A clone of the state may be what closed it, and then `made` won't say:
    // a session that changed nothing has the version it began with.
    if let Some(session) = moves.session.take_if(|session| open != Some(session.id))
        && (made.contains(&session.id) || version != session.version)
    {
        moves.record(session.id, session.saved);
    }
    let Some(saved) = before.saved else {
        return;
    };
    for &id in made.iter().filter(|id| !stepped.contains(id)) {
        moves.record(id, saved.clone());
    }
    if let Some(id) = open
        && before.open != open
        && !moves.holds(id)
    {
        moves.session = Some(Session {
            id,
            saved,
            version: before.version,
        });
    }
}

/// Before `u` or `<C-r>` takes a step: the marks as they are, which
/// [`reverted`] keeps for the step back; `None` with no record at all.
pub(super) fn outgoing(state: &mut VimState, model: &EditorModel) -> Option<Saved> {
    (!state.moves.records.is_empty()).then(|| Saved::of(state, model))
}

/// `u` (or `<C-r>`) just took back (or made again) transaction `id`, the
/// marks [`outgoing`] before it: `a` to `z` and the last visual extent saved
/// with it are put back (those it holds), and the outgoing ones kept in
/// their place for the step back.
pub(super) fn reverted(state: &mut VimState, id: TransactionId, outgoing: Option<Saved>) {
    state.moves.stepped.push(id);
    let Some(mut now) = outgoing else {
        return;
    };
    let records = &mut state.moves.records;
    let Some(at) = records.iter().position(|(saved, _)| *saved == id) else {
        return;
    };
    if records[at].1.visual.is_none() {
        now.visual = None;
    }
    let saved = std::mem::replace(&mut records[at].1, now);
    state.marks.restore_lowercase(saved.named);
    if saved.visual.is_some() {
        state.last_visual = saved.visual;
    }
}
