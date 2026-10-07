//! `.`: the last change, replayed (§9.5 of `docs/decisions/vim-engine.md`).

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, CommandId, Dispatched, Effect, Invocation};
use fe_editor::commands::origin;
use serde::Serialize;

use crate::{
    CountArgs, InsertArgs, Mode, OperateArgs, PutArgs, ReplaceCharArgs, VimTarget, edit, insert,
    operator, visual,
};

/// A change `.` can repeat: a committed `vim.operate` (not a yank),
/// `vim.put`, `vim.insert`, `vim.join`, `vim.replace_char` or
/// `vim.toggle_case`, by its args.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Operate(OperateArgs),
    Put(PutArgs),
    Insert(InsertArgs),
    Join(CountArgs),
    ReplaceChar(ReplaceCharArgs),
    ToggleCase(CountArgs),
}

impl Change {
    /// The invocation it was committed as.
    pub fn invocation(&self) -> Invocation {
        fn of(command: &str, args: impl Serialize) -> Invocation {
            let command = CommandId::parse(command).expect("vim command IDs are valid");
            Invocation::new(command).args(serde_json::to_value(args).expect("vim args serialize"))
        }
        match self {
            Self::Operate(args) => of("vim.operate", args),
            Self::Put(args) => of("vim.put", args),
            Self::Insert(args) => of("vim.insert", args),
            Self::Join(args) => of("vim.join", args),
            Self::ReplaceChar(args) => of("vim.replace_char", args),
            Self::ToggleCase(args) => of("vim.toggle_case", args),
        }
    }

    /// The same change with `count` in place of its own.
    fn counted(mut self, count: Option<u32>) -> Self {
        match &mut self {
            Self::Operate(args) => args.count = count,
            Self::Put(args) => args.count = count,
            Self::Insert(args) => args.count = count,
            Self::Join(args) | Self::ToggleCase(args) => args.count = count,
            Self::ReplaceChar(args) => args.count = count,
        }
        self
    }
}

/// What `.` repeats: the last change, and the text typed in the insert
/// session it opened (`None` if it opened none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastChange {
    pub(crate) change: Change,
    pub(crate) inserted: Option<String>,
    /// The session typed on the row it began on, even if that typing was
    /// deleted since: `.` keeps the indentation there too, as typing the
    /// keys again would.
    pub(crate) typed_first: bool,
}

impl From<Change> for LastChange {
    fn from(change: Change) -> Self {
        Self {
            change,
            inserted: None,
            typed_first: false,
        }
    }
}

impl LastChange {
    pub fn change(&self) -> &Change {
        &self.change
    }

    pub fn inserted(&self) -> Option<&str> {
        self.inserted.as_deref()
    }
}

/// `vim.repeat`: the last change, with `count` in place of its own if one
/// is given, and the text its session typed, as one transaction with the
/// repeating actor's origin. A put from register `1`–`8` takes the next
/// one, and `.` remembers that.
pub(crate) fn repeat(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: CountArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    crate::insert::adopt_closing(target.state);
    let Some(last) = target.state.last_change.clone() else {
        return Ok(Dispatched::new(Effect::Nothing));
    };
    let mut change = match args.count {
        Some(_) => last.change.clone().counted(args.count),
        None => last.change.clone(),
    };
    if let Change::Put(PutArgs {
        register: Some(name @ '1'..='8'),
        ..
    }) = &mut change
    {
        *name = char::from(*name as u8 + 1);
    }
    // The change it replays notes the carets where it begins.
    target.model.sync_display();
    target.model.buffer_mut().start_transaction(origin(actor));
    target.state.replaying = true;
    let replayed = replay(target, actor, &change, &last);
    target.state.replaying = false;
    let ended = target.model.buffer_mut().end_transaction();
    target.state.carets.close(ended);
    let dispatched = replayed?;
    Ok(Dispatched::new(dispatched.effect).with_transactions(ended.into_iter().collect()))
}

/// Runs `change`, then types `last`'s text into the session it opened, if
/// any, and ends that.
fn replay(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    change: &Change,
    last: &LastChange,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let dispatched = match *change {
        Change::Operate(args) if args.target.is_selection() => visual::repeat(target, actor, args)?,
        Change::Operate(args) => operator::operate(target, actor, args)?,
        Change::Put(args) => operator::put(target, actor, args)?,
        Change::Insert(args) => insert::insert(target, actor, args)?,
        Change::Join(args) => edit::join(target, actor, args)?,
        Change::ReplaceChar(args) => edit::replace_char(target, actor, args)?,
        Change::ToggleCase(args) => edit::toggle_case(target, actor, args)?,
    };
    if target.state.mode == Mode::Insert {
        if let Some(text) = last.inserted.as_deref().filter(|text| !text.is_empty()) {
            insert::type_at_carets(target.model, text, origin(actor));
        }
        if let Some(session) = target.state.session.as_ref().filter(|_| last.typed_first) {
            session.typed_on_first_rows();
        }
        insert::normal_mode(target)?;
        return Ok(Dispatched::new(Effect::Changed));
    }
    Ok(dispatched)
}
