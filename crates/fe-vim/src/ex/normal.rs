//! `:norm[al][!] {keys}` (`:help :normal`): `keys`, in vim notation
//! ([`parse_keys`]), run on the cursor's row, or on each line of the range
//! with the cursor at its start: by line number, as nvim does, so a line
//! the keys delete or put in moves the rest, and a number past the last
//! line is the last. Each key goes through [`crate::VimState::feed`] and
//! vim's own registry, with the line's target, before the line ends: typed
//! in insert and replace mode through the model, and onto a command line
//! of its own in `cmdline` mode, as `HeadlessVim` feeds a key; a register a
//! `@` plays goes in first. A key that fails drops the rest of that line's
//! keys, and a command left unfinished ends as `<Esc>` would: an insert or
//! replace session ends, a command line is dropped, a `:s` asking (`c`)
//! ends replacing no more, an operator or count cancelled. A visual mode stays (over a range, its cursor goes to each
//! line, and `<Esc>` ends it after the last), and on a command line
//! `<Esc>` runs it, as in a register. Going to a line keeps the goal
//! column, as nvim's curswant, unless a change on the line before left it
//! to be taken from where the cursor is: then it's the line's start. The keys
//! go through the remaps ([`crate::VimState::set_remaps`]), an expansion
//! running to the end of its line like the rest of the line's keys; `!`
//! skips them, and a prefix held for a longer map when the line's keys
//! end resolves, as a timeout would. The whole is one undo step, `.` and
//! the registers as the keys leave them (an undo or redo among the keys
//! undoes what the line did so far, and what follows is the step);
//! `:normal` itself never fails, but for a map's `<Cmd>fe:{command id}<CR>`
//! among its keys: there it stops, the command not run and nothing after it
//! (the rest of the line's keys, the lines after it) either, with
//! [`FE_REFUSED`] as its error, as fastedit commands run only from the
//! host's replay; a `:normal` it runs in stops the same way. Its keys aren't typed: a recording doesn't
//! get them and a replay under way goes on after them, but a recording or
//! `@` they start stays. Typed text goes in as the `:normal`'s actor.
//!
//! Known limit: nvim's `:normal` reads its keys as typed, so `<Esc>` there
//! is five characters.

use std::{collections::VecDeque, ops::RangeInclusive};

use fe_buffer::TransactionId;
use fe_command::{Actor, DispatchError, Dispatched, Effect, Invocation, Registry, Source};
use fe_editor::commands::origin;

use super::{
    ExCommand, ExError, Handled,
    global::{self, Depth, Group},
    to_row_start,
};
use crate::{
    Answer, AnswerArgs, Change, Mode, OperateArgs, Operator, Pending, Step, SubmitArgs,
    VimCommands, VimKey, VimState, VimTarget, context, history, parse_keys, registry,
};

/// What a `:normal` that reached a map's `<Cmd>fe:…<CR>` says.
pub(crate) const FE_REFUSED: &str = "fe: commands don't run inside :normal";

thread_local! {
    /// Vim's registry, which `:normal`'s keys run through, built once.
    static REGISTRY: Registry<VimCommands> = registry();
}

/// Whether `name` is `:norm[al]`.
pub(super) fn is_normal(name: &str) -> bool {
    name.len() >= 4 && "normal".starts_with(name)
}

/// A `:normal` line, from what follows its name: `[!]`, blanks, then the
/// keys (E471 if none); `check` is `ex::command`'s.
pub(super) fn parse(
    after: &str,
    check: impl FnOnce(bool) -> Result<(), ExError>,
) -> Result<ExCommand, ExError> {
    let (args, remap) = match after.strip_prefix('!') {
        Some(args) => (args, false),
        None => (after, true),
    };
    check(true)?;
    let keys = args.trim_start_matches([' ', '\t']);
    if keys.is_empty() {
        return Err(ExError::ArgumentRequired);
    }
    Ok(ExCommand::Normal {
        keys: parse_keys(keys)?,
        remap,
    })
}

/// `:normal`: `keys` on the cursor's row, or on each of `rows` (by number,
/// the last row past the end), as the module describes.
pub(super) fn run(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    rows: Option<RangeInclusive<u32>>,
    keys: &[VimKey],
    remap: bool,
) -> Result<Handled, ExError> {
    let _depth = Depth::enter()?;
    let outer = target.state.remap_save();
    // Fastedit commands left before it, which the host hasn't taken, wait
    // for it after: only the `:normal`'s own stop it.
    let outer_commands = std::mem::take(&mut target.state.remap_commands);
    target.state.normal_refused = false;
    let unmapped = target.state.remap_off(!remap);
    // The keys aren't typed: what a recording or a replay under way counts
    // goes on once they've run.
    let held = target.state.macros.hold();
    let group = Group::begin(target, actor);
    let mut runner = Runner {
        actor: actor.clone(),
        line: String::new(),
        refused: false,
    };
    match rows {
        None => runner.run(target, keys),
        Some(rows) => {
            let first = *rows.start();
            for row in rows {
                to_row_start(target, row, row == first);
                runner.run(target, keys);
                if runner.refused {
                    break;
                }
            }
            if matches!(target.state.mode(), Mode::Visual(_)) {
                runner.key(target, VimKey::Esc).ok();
            }
        }
    }
    let transactions: Vec<TransactionId> = group.end(target).into_iter().collect();
    target.state.macros.stop();
    target.state.macros.release(held);
    target.state.remap_off(unmapped);
    target.state.remap_restore(outer);
    target.state.remap_commands = outer_commands;
    target.state.failed = false;
    // For a `:normal` this one ran in, which stops at the key that ran it.
    target.state.normal_refused = runner.refused;
    if runner.refused {
        target.state.fail(FE_REFUSED);
    }
    Ok(Ok(
        Dispatched::new(Effect::Changed).with_transactions(transactions)
    ))
}

/// Feeds keys as `HeadlessVim` does, with its own command line.
struct Runner {
    actor: Actor,
    /// The command line's text, in `cmdline` mode.
    line: String,
    /// A key left a fastedit command, or ran a `:normal` that stopped at one
    /// ([`FE_REFUSED`]): this one stops.
    refused: bool,
}

impl Runner {
    /// `keys` once, then ends what they left unfinished.
    fn run(&mut self, target: &mut VimTarget<'_>, keys: &[VimKey]) {
        let mut queue: VecDeque<VimKey> = keys.iter().copied().collect();
        while let Some(key) = queue.pop_front().or_else(|| held_keys(target, &mut queue)) {
            let before = history::pairs(target.model);
            let fed = self.key(target, key);
            Group::note(target, before);
            // A command this one's keys left, or a `:normal` they ran
            // that stopped at one.
            if target.state.has_remap_command() || target.state.normal_refused {
                // Dropped with the rest: not run, nor left for the host.
                self.refused = true;
                target.state.stop_replay();
                break;
            }
            if fed.is_err() || target.state.failed() {
                target.state.stop_replay();
                break;
            }
            for key in target.state.take_replay().into_iter().rev() {
                queue.push_front(key);
            }
        }
        let before = history::pairs(target.model);
        let ended = match target.state.mode() {
            Mode::Insert => self.dispatch(target, "vim.normal_mode", None),
            Mode::Replace => self.dispatch(target, "vim.replace_normal_mode", None),
            Mode::CmdLine(_) => self.dispatch(target, "vim.command_line_cancel", None),
            _ if target.state.confirm.is_some() => {
                let quit = AnswerArgs {
                    answer: Answer::Quit,
                };
                let args = serde_json::to_value(quit).expect("vim args serialize");
                self.dispatch(target, "vim.substitute_answer", Some(args))
            }
            _ if *target.state.pending() != Pending::default() => {
                // The `<Esc>` is ours, not typed: past the maps.
                let unmapped = target.state.remap_off(true);
                target.state.feed(VimKey::Esc);
                target.state.remap_off(unmapped);
                Ok(())
            }
            _ => Ok(()),
        };
        ended.ok();
        target.state.remap_clear();
        Group::note(target, before);
    }

    /// One key, in the mode it's read in; a map's `<Cmd>…<CR>` by the
    /// reducer in every mode ([`crate::VimState::remap_command_next`]).
    fn key(&mut self, target: &mut VimTarget<'_>, key: VimKey) -> Result<(), ()> {
        target.state.sync_visual(target.model);
        let state = &mut target.state;
        let mode = match state.remap_command_next() {
            true => Mode::Normal,
            false => state.mode(),
        };
        match mode {
            Mode::Insert | Mode::Replace | Mode::CmdLine(_) => state.record_key(key),
            _ => {
                return match state.feed(key) {
                    Step::Wait(_) | Step::Cancel => Ok(()),
                    Step::Forward => Err(()),
                    Step::Commit(invocation) => self.run_invocation(target, invocation),
                };
            }
        }
        let replace = state.mode() == Mode::Replace;
        match (state.mode(), key) {
            (Mode::CmdLine(_), VimKey::Char(ch)) => self.line.push(ch),
            (Mode::CmdLine(_), VimKey::Backspace) if self.line.pop().is_some() => {}
            (Mode::CmdLine(_), VimKey::Backspace) => {
                return self.dispatch(target, "vim.command_line_cancel", None);
            }
            // `<Esc>` runs the line, as a register's does.
            (Mode::CmdLine(_), VimKey::Enter | VimKey::Ctrl('j') | VimKey::Esc) => {
                let text = std::mem::take(&mut self.line);
                let args = serde_json::to_value(SubmitArgs { text }).expect("vim args serialize");
                return self.dispatch(target, "vim.command_line_submit", Some(args));
            }
            (Mode::CmdLine(_), _) => return Err(()),
            // Typed as the `:normal`'s actor, in its transaction.
            (_, VimKey::Char(_) | VimKey::Enter | VimKey::Ctrl('j') | VimKey::Backspace) => {
                let (state, host) = (&mut *target.state, &*target.host);
                target.model.run_as(origin(&self.actor), |model| match key {
                    VimKey::Char(ch) if replace => {
                        state.replace_text(model, ch.encode_utf8(&mut [0; 4]));
                    }
                    VimKey::Char(ch) => model.insert(ch.encode_utf8(&mut [0; 4])),
                    VimKey::Backspace if replace => state.replace_backspace(model),
                    VimKey::Backspace => model.backspace(),
                    _ => state.line_break_in(model, host),
                });
            }
            (_, VimKey::Esc) if replace => {
                return self.dispatch(target, "vim.replace_normal_mode", None);
            }
            (_, VimKey::Esc) => return self.dispatch(target, "vim.normal_mode", None),
            _ => return Err(()),
        }
        Ok(())
    }

    /// Runs `command` (with `args`) as the key that ends a mode does.
    fn dispatch(
        &mut self,
        target: &mut VimTarget<'_>,
        command: &str,
        args: Option<serde_json::Value>,
    ) -> Result<(), ()> {
        let invocation = Invocation::of(command)
            .expect("vim command IDs are valid")
            .source(Source::Keyboard);
        let invocation = match args {
            Some(args) => invocation.args(args),
            None => invocation,
        };
        self.run_invocation(target, invocation)
    }

    /// Runs `invocation` for the `:normal`'s actor, stamped with the
    /// context as it is now. One that opens a command line starts this
    /// one's text from its prefill; one that ends it empties it.
    fn run_invocation(
        &mut self,
        target: &mut VimTarget<'_>,
        invocation: Invocation,
    ) -> Result<(), ()> {
        let was_open = matches!(target.state.mode(), Mode::CmdLine(_));
        let joins = joins(&invocation, target.state);
        let from = target.model.buffer().version();
        let invocation = invocation
            .actor(self.actor.clone())
            .context(context(target.state));
        let mut reborrowed = VimTarget {
            model: &mut *target.model,
            host: &mut *target.host,
            state: &mut *target.state,
            registers: &mut *target.registers,
        };
        let dispatched: Result<_, DispatchError> =
            REGISTRY.with(|registry| registry.dispatch(&mut reborrowed, &invocation));
        if joins {
            let until = target.model.buffer().version();
            global::noted(from, until, global::Change::Joined);
        }
        if !was_open || !matches!(target.state.mode(), Mode::CmdLine(_)) {
            self.line.clear();
            self.line.push_str(target.state.command_line_prefill());
        }
        dispatched.map(|_| ()).map_err(|_| ())
    }
}

/// The keys a prefix held for a longer map resolves to, as a timeout
/// would, when the line's keys ran out; the first of them, the rest queued.
fn held_keys(target: &mut VimTarget<'_>, queue: &mut VecDeque<VimKey>) -> Option<VimKey> {
    if !target.state.remap_waiting() {
        return None;
    }
    target.state.remap_timeout();
    queue.extend(target.state.take_replay());
    queue.pop_front()
}

/// Whether `invocation` joins rows: `J` (`vim.join`), `gJ` (`vim.join_raw`),
/// a visual `J` or `gJ`, or `.` repeating any of them, for a `:g` following its rows ([`global::Change`]).
fn joins(invocation: &Invocation, state: &VimState) -> bool {
    let operates_join = |args: &serde_json::Value| {
        serde_json::from_value::<OperateArgs>(args.clone())
            .is_ok_and(|args| matches!(args.operator, Operator::Join | Operator::JoinRaw))
    };
    match invocation.command.as_str() {
        "vim.join" | "vim.join_raw" => true,
        "vim.operate" => operates_join(&invocation.args),
        "vim.repeat" => match state.last_change().map(|last| last.change()) {
            Some(Change::Join(_)) => true,
            Some(Change::Operate(args)) => {
                matches!(args.operator, Operator::Join | Operator::JoinRaw)
            }
            _ => false,
        },
        _ => false,
    }
}
