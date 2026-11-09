//! `:{range}!{cmd}` and `:[line]r[ead] !{cmd}` (`:help :range!`, `:help
//! :r!`): a shell command fe-vim parses but never runs. The filter becomes
//! [`HostCommand::Filter`], the rows it names to go through the command,
//! and `:r !` (or `:r!`) [`HostCommand::ReadCommand`], the command's output
//! to go below the line as `:r {file}` puts a file's rows; the host runs
//! the command (off its UI thread) and hands the output back to
//! [`VimState::apply_filter`] or [`VimState::apply_read_command`].
//!
//! The `!` after a range isn't a command's name, so `ex.rs` hands the line
//! here before it looks names up. `:!{cmd}` with no range is refused, with
//! a message pointing to the terminal panel: its output would have nowhere
//! to go.
//!
//! The command is a [`ShellCommand`]: its text, with `%` and `#` taken out
//! as the current and alternate file ([`ShellPart`]), which the host
//! expands, each shell-escaped (nvim's `%` isn't). `\%` and `\#` are a
//! literal `%` and `#`, and `\!` a `!`; any other backslash stays. A `!` in
//! the command stays literal (nvim puts the last command there), but for a
//! command of `!` alone (`:.!!`), which as an empty one (`:.!`) repeats the
//! last shell command, or fails E34 with none. (nvim's `:.!` does nothing;
//! its `:.!!` repeats.) A filename modifier after `%` or `#` (`%:p`, `#:h`),
//! a buffer's number (`#2`, `#<2`), `##` and the `<cword>`-style words nvim
//! expands are refused with a message, not run with something else in
//! their place.
//!
//! Only the user's own keys run one: a line any other [`Actor`] submits
//! (`Host(..)`, a plugin's; `Ai {..}`; `Lsp`; `Format`), or one a `:g` or a
//! `:normal` runs (`global.rs`, `normal.rs`), or a map's command the host
//! runs while a `:normal` waits suspended on it, whoever asked for that,
//! gets [`crate::messages::shell_refused`] and no host command; so does
//! anything a macro's replay runs, its own keys or those of the maps they
//! run, a map's
//! `<Cmd>fe:…<CR>` among them, which the host runs later, and `@:`'s line.
//! Where it came from goes with each invocation, through all it runs
//! (`remap::Provenances`), not with the last key read. The host command
//! carries the actor, for the host to check again. A line that runs one becomes the last shell
//! command (nvim's for `:!!`), the rows and below line the range's, as for
//! any command (E16 past the last line, E493 backwards).

use std::ops::Range;

use fe_buffer::{BufferSnapshot, TransactionId};
use fe_command::Actor;
use fe_editor::{EditorModel, commands::origin};
use kiln_l10n::tr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::{Address, ExCommand, ExError, HostCommand, first_non_blank, global};
use crate::{VimState, feature::line_undo, history, insert, marks, operator, remap::Provenance};

/// The words nvim expands in a shell command, which fe-vim refuses.
const SPECIAL: &[&str] = &[
    "<cword>", "<cWORD>", "<cexpr>", "<cfile>", "<afile>", "<abuf>", "<amatch>", "<sfile>",
    "<stack>", "<script>", "<slnum>", "<sflnum>", "<client>",
];

/// A shell command as typed after `!`, in parts, for the host to put
/// together: the literal text, and the file names `%` and `#` stand for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ShellCommand {
    /// In order, literal text never next to more literal text.
    pub parts: Vec<ShellPart>,
}

/// One part of a [`ShellCommand`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShellPart {
    /// Text as the shell gets it (`\%` and `\#` already a `%` and `#`).
    Text(String),
    /// `%`: the buffer's file name, shell-escaped.
    CurrentFile,
    /// `#`: the alternate file's name, shell-escaped.
    AlternateFile,
}

impl ShellCommand {
    /// The command `text` (what follows the `!`, its leading blanks gone)
    /// is, as the module describes; with no parts for an empty one or `!`
    /// alone, which repeat the last.
    pub(crate) fn parse(text: &str) -> Result<Self, ExError> {
        let text = text.trim_start_matches([' ', '\t']);
        if text.trim_end_matches([' ', '\t']) == "!" {
            return Ok(Self::default());
        }
        let mut parts = Vec::new();
        let mut literal = String::new();
        let mut chars = text.char_indices().peekable();
        while let Some((at, ch)) = chars.next() {
            match ch {
                '\\' => match chars.peek() {
                    Some(&(_, escaped @ ('%' | '#' | '!'))) => {
                        literal.push(escaped);
                        chars.next();
                    }
                    _ => literal.push('\\'),
                },
                '%' | '#' => {
                    if let Some(refused) = modified(ch, &text[at + 1..]) {
                        return Err(ExError::unsupported(&refused));
                    }
                    if !literal.is_empty() {
                        parts.push(ShellPart::Text(std::mem::take(&mut literal)));
                    }
                    parts.push(match ch {
                        '%' => ShellPart::CurrentFile,
                        _ => ShellPart::AlternateFile,
                    });
                }
                '<' if let Some(word) =
                    SPECIAL.iter().find(|word| text[at..].starts_with(*word)) =>
                {
                    return Err(ExError::unsupported(word));
                }
                _ => literal.push(ch),
            }
        }
        if !literal.is_empty() {
            parts.push(ShellPart::Text(literal));
        }
        Ok(Self { parts })
    }

    /// Whether it has no parts: the last command, still to be filled in.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// What a `%` or `#` (`name`) followed by `rest` asks for that fe-vim
/// refuses, as typed: a filename modifier, or for `#` a buffer by number
/// (`#2`, `#<2`) or the argument list (`##`).
fn modified(name: char, rest: &str) -> Option<String> {
    let mut after = rest.chars();
    let refused = match (after.next(), after.next()) {
        (
            Some(':'),
            Some(modifier @ ('p' | '~' | '.' | 'h' | 't' | 'r' | 'e' | 's' | 'g' | 'S')),
        ) => {
            format!("{name}:{modifier}")
        }
        (Some(other @ ('0'..='9' | '<' | '#')), _) if name == '#' => format!("#{other}"),
        _ => return None,
    };
    Some(refused)
}

/// The `!` after `range` and what follows it (`after`): a filter of the
/// range's rows, filled in as it runs, or with no range refused; `check`
/// is `ex::command`'s.
pub(in crate::ex) fn parse(
    range: &[Address],
    after: &str,
    check: impl FnOnce(bool) -> Result<(), ExError>,
) -> Result<ExCommand, ExError> {
    if range.is_empty() {
        return Err(ExError::NoRangeShell);
    }
    check(true)?;
    Ok(ExCommand::Host(HostCommand::Filter {
        rows: 0..0,
        command: ShellCommand::parse(after)?,
        actor: Actor::User,
    }))
}

/// `:r !{cmd}`'s command (`text`, what follows its `!`), the line below
/// filled in as it runs.
pub(in crate::ex) fn parse_read(text: &str) -> Result<ExCommand, ExError> {
    Ok(ExCommand::Host(HostCommand::ReadCommand {
        below: 0,
        command: ShellCommand::parse(text)?,
        actor: Actor::User,
    }))
}

/// [`ExError::ShellRefused`] for a shell command `actor` asks for: anyone but the user,
/// or anyone while a `:g` or `:normal` runs, or a `:normal` waits
/// suspended for the host to run a map's command (which may be this), or
/// for what runs from a macro's replay or a `:normal`, through its maps, a
/// map's fastedit command or `@:` too, whatever invocation it comes
/// through (`remap::Provenances`).
pub(crate) fn refusal(state: &VimState, actor: &Actor) -> Result<(), ExError> {
    let normal = global::running() || state.normal_suspended.is_some();
    let own = state.provenance.scope() == Provenance::User;
    match *actor == Actor::User && !normal && own {
        true => Ok(()),
        false => Err(ExError::ShellRefused),
    }
}

/// `command` as it runs: the last shell command if it's empty (E34 with
/// none), which it then becomes.
pub(in crate::ex) fn fill(state: &mut VimState, command: &mut ShellCommand) -> Result<(), ExError> {
    if command.is_empty() {
        *command = state
            .filter
            .last
            .clone()
            .ok_or(ExError::NoPreviousCommand)?;
    }
    state.filter.last = Some(command.clone());
    Ok(())
}

/// A command's output as rows: split at line breaks, the last (if any)
/// ending the last row rather than starting another. Empty, it's none.
fn output_rows(output: &str) -> Vec<&str> {
    let rows: Vec<&str> = match output.strip_suffix('\n').unwrap_or(output) {
        "" if !output.starts_with('\n') => Vec::new(),
        rows => rows.split('\n').collect(),
    };
    // Output whose every line ends in CRLF, as Windows' commands write it,
    // is read as DOS lines, as vim's 'fileformats' detection reads it.
    let breaks = output.matches('\n').count();
    if breaks > 0 && output.matches("\r\n").count() == breaks {
        return (rows.into_iter())
            .map(|row| row.strip_suffix('\r').unwrap_or(row))
            .collect();
    }
    rows
}

/// Catches the marks up with the text before an edit, and follows the
/// buffer from there, as a vim command does first.
fn follow(state: &mut VimState, model: &mut EditorModel) {
    marks::catch_up(state);
    state.marks.follow(model.snapshot());
    state.marks.watch(model);
    line_undo::follow(state, model);
}

/// One step of an [`apply`]: edits planned from the text the steps before
/// left (with the state, for the marks to be set on it first).
type Step<'a> = Box<dyn FnOnce(&mut VimState, &BufferSnapshot) -> Vec<(Range<usize>, String)> + 'a>;

/// `steps` as one transaction, the user's (only the user's keys ask for
/// one), each step's edits a batch of their own; with none, still an undo
/// step, as nvim's is: a character put in at the text's end and taken out
/// again, which no mark or caret sees (its batches set no mark).
fn apply(
    state: &mut VimState,
    model: &mut EditorModel,
    steps: Vec<Step<'_>>,
) -> (Option<TransactionId>, BufferSnapshot) {
    let origin = origin(&Actor::User);
    let applied = {
        let mut buffer = model.buffer_mut();
        buffer.start_transaction(origin.clone());
        let mut changed = false;
        for step in steps {
            let edits = step(state, &buffer.snapshot());
            if !edits.is_empty() {
                buffer.edit(edits, origin.clone());
                changed = true;
            }
        }
        if !changed {
            let end = buffer.snapshot().text().len();
            for edits in [
                vec![(end..end, " ".to_string())],
                vec![(end..end + 1, String::new())],
            ] {
                buffer.edit(edits, origin.clone());
                state
                    .marks
                    .note(buffer.snapshot().version(), marks::Note::Unchanged);
            }
        }
        (buffer.end_transaction(), buffer.snapshot())
    };
    model.sync_display();
    applied
}

impl VimState {
    /// Replaces `rows` (from 0, within the text) of `model` with the rows
    /// of `output`, what [`HostCommand::Filter`]'s command printed for
    /// them (none if it's empty; a last line without a line break is a row
    /// all the same), as nvim's filter does: one undo step, even if nothing
    /// changes, which `u` undoes with the caret at the start of the
    /// range's last row. Each row it keeps (as many as the output has, from
    /// the first) is rewritten in place, its marks staying on it; the marks
    /// on the rows it drops go, as nvim's. The caret goes to the first
    /// non-blank of the range's first row (with no rows left, the one that
    /// took its place), and of more than 2 rows ('report') the message is
    /// `{n} lines filtered`. For the host, once the command has run.
    pub fn apply_filter(&mut self, model: &mut EditorModel, rows: Range<u32>, output: &str) {
        insert::adopt_closing(self);
        model.sync_display();
        follow(self, model);
        // Output arrives outside any command, so no dispatch records these.
        let saved = history::Saved::of(self, model);
        let text = model.text().clone();
        let count = text.row_count();
        let first = rows.start.min(count - 1);
        let last = rows.end.clamp(first + 1, count) - 1;
        let lines = output_rows(output);
        let old = last - first + 1;
        let new = u32::try_from(lines.len()).unwrap_or(u32::MAX);
        let kept = old.min(new);
        // The rows dropped go first, whole, so no edit takes a kept row's
        // text and line break together (which would take its marks); then
        // the kept rows are rewritten in place, and the rows past the
        // range's put in after its last.
        let mut steps: Vec<Step<'_>> = Vec::new();
        if kept < old {
            self.marks.deleting(model.snapshot(), first + kept, last);
            steps.push(Box::new(
                move |_: &mut VimState, snapshot: &BufferSnapshot| {
                    let text = snapshot.text();
                    let range = match kept {
                        _ if last + 1 < count => {
                            text.row_start(first + kept)..text.row_start(last + 1)
                        }
                        0 if first > 0 => operator::row_end(text, first - 1)..text.len(),
                        0 => 0..text.len(),
                        _ => operator::row_end(text, first + kept - 1)..text.len(),
                    };
                    vec![(range, String::new())]
                },
            ));
        }
        let rewrites = &lines[..kept as usize];
        steps.push(Box::new(
            move |_: &mut VimState, snapshot: &BufferSnapshot| {
                let text = snapshot.text();
                (first..)
                    .zip(rewrites)
                    .filter_map(|(row, &line)| {
                        let range = text.row_start(row)..operator::row_end(text, row);
                        (text.slice_to_string(range.clone()) != line)
                            .then(|| (range, line.to_string()))
                    })
                    .collect()
            },
        ));
        if kept < new {
            let put = format!("\n{}", lines[kept as usize..].join("\n"));
            // The kept rows rewritten (an emptied one too) keep their marks,
            // set on them anew, so the rows put in after don't take them.
            let refilled: Vec<u32> = (first..)
                .zip(rewrites)
                .filter(|&(row, &line)| {
                    let range = text.row_start(row)..operator::row_end(&text, row);
                    text.slice_to_string(range) != line
                })
                .map(|(row, _)| row)
                .collect();
            steps.push(Box::new(
                move |state: &mut VimState, snapshot: &BufferSnapshot| {
                    state.marks.refilled(snapshot, &refilled);
                    let end = operator::row_end(snapshot.text(), last);
                    vec![(end..end, put)]
                },
            ));
        }
        let undone = (last, 0);
        let (transaction, snapshot) = apply(self, model, steps);
        if let Some(id) = transaction {
            self.carets.record(id, vec![undone]);
            self.moves.note(id, saved);
        }
        // nvim's `']` is on the last row put in, or with none on the row
        // above the range.
        let end = match new {
            0 => first.checked_sub(1),
            new => Some(first + new - 1),
        };
        marks::filtered(self, model, transaction, first, end);
        let text = snapshot.text();
        let row = first.min(text.row_count() - 1);
        operator::land(model, &snapshot, [first_non_blank(text, row)]);
        self.goals = None;
        self.message = (old > 2).then(|| tr!("vim-lines-filtered", count = old).to_string());
    }

    /// Puts the rows of `output`, what [`HostCommand::ReadCommand`]'s
    /// command printed (none if it's empty; a last line without a line
    /// break is a row all the same), below line `below` of `model` (from
    /// 1; `0` above the first), as `:r {file}` puts a file's: one undo step,
    /// even with none to put, which `u` undoes with the carets where they
    /// were. As nvim's `:r !`, the caret goes to the first non-blank of the
    /// last row put in, or with none of line `below` (the first for `0`).
    /// For the host, once the command has run.
    pub fn apply_read_command(&mut self, model: &mut EditorModel, below: u32, output: &str) {
        insert::adopt_closing(self);
        model.sync_display();
        follow(self, model);
        let saved = history::Saved::of(self, model);
        let text = model.text().clone();
        let below = below.min(text.row_count());
        let lines = output_rows(output);
        let edits = match (below, lines.is_empty()) {
            (_, true) => Vec::new(),
            (0, false) => {
                self.marks.opening(model.snapshot(), 0);
                vec![(0..0, format!("{}\n", lines.join("\n")))]
            }
            (below, false) => {
                let end = operator::row_end(&text, below - 1);
                vec![(end..end, format!("\n{}", lines.join("\n")))]
            }
        };
        let before = history::pairs(model);
        let (transaction, snapshot) = apply(
            self,
            model,
            vec![Box::new(move |_: &mut VimState, _: &BufferSnapshot| edits)],
        );
        if let Some(id) = transaction {
            self.carets.record(id, before);
            self.moves.note(id, saved);
        }
        let put = u32::try_from(lines.len()).unwrap_or(u32::MAX);
        // nvim's `'[` and `']` are on the first and last rows put in; with
        // none, on the line below and the line.
        marks::filtered(
            self,
            model,
            transaction,
            below,
            (below + put).checked_sub(1),
        );
        let row = match put {
            0 => below.max(1) - 1,
            put => below + put - 1,
        };
        let text = snapshot.text();
        let row = row.min(text.row_count() - 1);
        operator::land(model, &snapshot, [first_non_blank(text, row)]);
        self.goals = None;
        self.message = None;
    }
}
