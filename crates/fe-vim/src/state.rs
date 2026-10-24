//! Vim's per-editor state and the context keys it publishes.

use std::sync::Arc;

use fe_buffer::{TransactionId, Version};
use fe_command::{ContextFrame, ContextStack, Dispatched, Effect, Invocation, Source};
use fe_editor::{
    EditorModel,
    commands::{CONTEXT, EditorHost},
};

use crate::ex::HostCommand;
use crate::feature::replace_register;
use crate::macros::REPLAY_CAP;
use crate::remap::{
    Origin, Provenance, Provenances, RemapCommand, RemapSet, RemapState, Staged, TYPED,
};
use crate::search::Pattern;
use crate::{
    CommandLineKind, LastChange, Motion, Pending, Step, VimKey, VisualKind,
    cmdline::{self, CommandLine},
    history::Carets,
    insert::{Closing, Completion, OneCommand, Session},
    jump,
    macros::Macros,
    marks::Marks,
    replace::Replace,
    step,
    visual::{Extent, LastVisual, Visual},
};

/// The context key holding the mode's name.
const MODE: &str = "vim.mode";
/// The flag set while a command is incomplete.
const PENDING: &str = "vim.pending";
/// What the incomplete command waits on: [`crate::PendingKind::name`].
const PENDING_KIND: &str = "vim.pending_kind";
/// The flag set in every visual mode, so a binding can name them all.
const VISUAL: &str = "vim.visual";
/// The command line's kind, in `cmdline` mode: [`CommandLineKind::name`].
const CMDLINE_KIND: &str = "vim.cmdline_kind";
/// The register being recorded into, while `q` records.
const RECORDING: &str = "vim.recording";
/// The flag set while `<C-o>` left insert or replace mode for one command.
const INSERT_PENDING: &str = "vim.insert_pending";
/// The flag set while `gr` is ReplaceWithRegister's
/// ([`VimState::set_replace_with_register`]).
const REPLACE_WITH_REGISTER: &str = "vim.replace_with_register";

/// The editing mode.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Normal,
    /// An insert session is open: typed text goes to the editor, not
    /// [`step`].
    Insert,
    /// `R`: an insert session is open whose typing overwrites the row
    /// ([`VimState::replace_text`]).
    Replace,
    /// `v`, `V` or `<C-v>`: keys go to [`step`], which reads operators as
    /// acting on the selection.
    Visual(VisualKind),
    /// A command line is open (`/`, `?`, `:`): the host's line takes the
    /// keys until it submits or cancels it.
    CmdLine(CommandLineKind),
}

/// What insert mode's `<C-r>` has read so far ([`VimState::insert_register_start`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum RegisterKey {
    /// `<C-r>`: a register's name, or `<C-r>`, `<C-o>` or `<C-p>` for a
    /// literal insert, comes next.
    Start,
    /// `<C-r><C-r>`, `<C-r><C-o>` or `<C-r><C-p>`: the register's name
    /// comes next.
    Literal(LiteralInsert),
}

/// How `<C-r><C-r>`, `<C-r><C-o>` and `<C-r><C-p>` insert a register.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum LiteralInsert {
    /// `<C-r><C-r>`: the text as it is.
    Plain,
    /// `<C-r><C-o>`: as it is, whole lines above the caret's row.
    Above,
    /// `<C-r><C-p>`: as it is, whole lines above the row, with their
    /// indent made the row's.
    FixIndent,
}

/// Each caret's head and its goal column.
pub(crate) type Goals = Vec<(usize, Option<usize>)>;

/// One editor's vim state: its mode, the pending input, each caret's goal
/// column, the last character search, the open insert session, the last
/// change for `.`, the carets before each change for `u` and `<C-r>`, and
/// the marks: they're per editor view, not per buffer as in Vim.
#[derive(Clone, Debug, Default)]
pub struct VimState {
    pub(crate) mode: Mode,
    pub(crate) pending: Pending,
    /// Each caret's head and the display column it keeps through `j`, `k`,
    /// `gg` and `G` (Vim's `curswant`; `usize::MAX` is the line end, after
    /// `$`; `None` is the caret's own column), as of a version of the text.
    pub(crate) goals: Option<(Version, Goals)>,
    /// The display column along its row each caret's last `gj` or `gk`
    /// aimed for, with the goal it came with: only they set it, and it
    /// holds only while that goal does in the same version of the text.
    pub(crate) display_goals: Option<(Version, Vec<crate::line_motions::DisplayGoal>)>,
    pub(crate) last_find: Option<Motion>,
    /// Set in insert mode: the session `vim.normal_mode` ends.
    pub(crate) session: Option<Session>,
    /// Set in replace mode: what typing overwrote, for `<BS>`.
    pub(crate) replace: Option<Replace>,
    /// Set while `<C-o>` left a session for one normal-mode command: what
    /// resumes once it's done ([`VimState::one_command`]).
    pub(crate) one_command: Option<OneCommand>,
    /// Set while insert mode waits for the register `<C-r>` names
    /// ([`VimState::insert_register_start`]), and the session it began in,
    /// which a prefix left over from an ended one doesn't belong to.
    pub(crate) register_key: Option<(RegisterKey, usize)>,
    /// Set while `<C-n>` or `<C-p>` completes a keyword in insert mode
    /// ([`VimState::completion`]).
    pub(crate) completion: Option<Completion>,
    /// A session this state left while a clone was ending it: the outcome
    /// is adopted once it's published (see `insert::adopt_closing`).
    pub(crate) closing: Option<Closing>,
    pub(crate) last_change: Option<LastChange>,
    /// The text the last insert session typed, until the registers take it
    /// for `.` ([`Registers::adopt_inserted`]).
    pub(crate) pending_inserted: Option<(u64, String)>,
    /// How many times `last_change` was set, to order it against a session
    /// a clone is still closing (see `insert::Closing`).
    pub(crate) changes: u64,
    pub(crate) carets: Carets,
    /// Set while `.` replays a change inside its own transaction.
    pub(crate) replaying: bool,
    /// Set in a visual mode: its anchor and cursor.
    pub(crate) visual: Option<Visual>,
    /// The last visual mode's extent, for `gv`, `'<` and `'>`.
    pub(crate) last_visual: Option<LastVisual>,
    /// How much text the last change over a selection took, for `.`.
    pub(crate) visual_extent: Option<Extent>,
    /// Set in `cmdline` mode: what the line is for and the mode it came
    /// from.
    pub(crate) cmdline: Option<CommandLine>,
    /// The last error (or `:s`'s report), for the host to show until the
    /// next key.
    pub(crate) message: Option<String>,
    /// The last search's pattern, painted while 'hlsearch' is on.
    pub(crate) highlight: Option<Pattern>,
    /// The file or window command the last ex line gave, for the host.
    pub(crate) host_command: Option<HostCommand>,
    /// `a` to `z` and the jump mark.
    pub(crate) marks: Marks,
    /// The recording under way and the replay the host is feeding.
    pub(crate) macros: Macros,
    /// The last key read failed, as Vim's beeps and errors do: a motion
    /// that couldn't move, an operator whose target didn't resolve, an
    /// error message, a key that cancelled what was pending.
    pub(crate) failed: bool,
    /// 'scroll', as a count before `<C-d>` or `<C-u>` set it, with the
    /// view's rows then: a view of another height goes back to half its
    /// rows, as nvim's does (see [`crate::scroll`]).
    pub(crate) scroll: Option<(u32, u32)>,
    /// The jump's labels waiting, the last jump and the `s` opt-in.
    pub(crate) jump: jump::Jumps,
    /// The buffer's version when a linewise delete last left its text
    /// empty. As in nvim, such a buffer has no rows to take, so while the
    /// version still equals this a delete or change writes no register (a
    /// yank still does). fe-buffer drops edits that change nothing, so
    /// `i<Esc>`, `>>` and `cc<Esc>` keep it; any edit putting text in moves
    /// it on. Known limit: nvim counts the buffer as emptied again when `u`
    /// or `<C-r>` returns it to a state a linewise delete emptied, which a
    /// version check can't see.
    pub(crate) emptied: Option<Version>,
    /// The last yank's ranges (one per row of a block) with its serial, for
    /// a host to flash.
    pub(crate) yank: Option<(u64, Vec<std::ops::Range<usize>>)>,
    /// Set while a `:s` with `c` waits for an answer: the keys
    /// [`VimState::feed`] reads answer it.
    pub(crate) confirm: Option<Box<crate::substitute::Confirm>>,
    /// The remaps, the keys held for a longer match and the keys they
    /// resolved to, for the host to feed.
    pub(crate) remap: RemapState,
    /// The last key read, through [`VimState::feed`] or
    /// [`VimState::record_key`], was a map's.
    pub(crate) expanded: bool,
    /// The fastedit commands maps' `<Cmd>fe:…<CR>` ran, for the host, in
    /// order.
    pub(crate) remap_commands: std::collections::VecDeque<(RemapCommand, Provenance)>,
    /// The last `:normal` to end stopped at a map's `<Cmd>fe:…<CR>`
    /// ([`crate::ex::normal`]): one it ran in stops too.
    pub(crate) normal_refused: bool,
    /// The host resumes a `:normal` that stops at a map's `<Cmd>fe:…<CR>`
    /// ([`VimState::set_normal_resumes`]).
    pub(crate) normal_resumes: bool,
    /// The `:normal` waiting for the host to run its fastedit command
    /// ([`crate::ex::normal`]).
    pub(crate) normal_suspended: Option<crate::ex::normal::Suspended>,
    /// The row `U` undoes the latest changes on.
    pub(crate) line_undo: crate::feature::line_undo::LineUndo,
    /// 'incsearch' is off ([`VimState::set_incsearch`]): on by default, as
    /// in nvim.
    pub(crate) incsearch_off: bool,
    /// `gr` is ReplaceWithRegister's ([`VimState::set_replace_with_register`]).
    pub(crate) replace_with_register: bool,
    /// The region the first `cx` marked, waiting for the second.
    pub(crate) exchange: Option<crate::feature::exchange::Marked>,
    /// `!`'s line opened and the last shell command (`feature/filter.rs`).
    pub(crate) filter: crate::feature::filter::Filters,
    /// Where what runs came from: the user's keys, a macro's or a
    /// `:normal`'s, with each invocation (shell commands run only from the
    /// user's, `ex/filter.rs`).
    pub(crate) provenance: Provenances,
}

impl VimState {
    /// The last yank's serial (new for every yank) and the ranges it took,
    /// in the text as yanked.
    pub fn last_yank(&self) -> Option<(u64, &[std::ops::Range<usize>])> {
        self.yank
            .as_ref()
            .map(|(serial, ranges)| (*serial, ranges.as_slice()))
    }

    /// Records a yank of `ranges` under the next serial.
    pub(crate) fn record_yank(&mut self, ranges: Vec<std::ops::Range<usize>>) {
        let serial = self.yank.as_ref().map_or(1, |(serial, _)| serial + 1);
        self.yank = Some((serial, ranges));
    }

    /// Whether the buffer is as a linewise delete left it, emptied.
    pub(crate) fn is_emptied(&self, model: &EditorModel) -> bool {
        self.emptied == Some(model.snapshot().version())
    }
}

impl VimState {
    /// The host's view now has `rows` rows: a counted 'scroll' set for
    /// another height is dropped at once, as nvim's window does, so a
    /// resize away and back can't revive it. A host calls this as its
    /// height changes; `rows` is what [`EditorHost::viewport`] would report.
    pub fn viewport_height_changed(&mut self, rows: u32) {
        let rows = rows.max(1);
        if self.scroll.is_some_and(|(_, set_for)| set_for != rows) {
            self.scroll = None;
        }
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Gives `gr` to ReplaceWithRegister
    /// (`feature/replace_register.rs`): `gr{motion}`, `grr` and visual `gr`
    /// replace text with a register's, and the context frame carries the
    /// flag `vim.replace_with_register` for the keymap's `gr` rows. Off by
    /// default, when `gr` and the key after it cancel, as the keys
    /// [`VimState::set_jump_on_s`] gives are only theirs when it's on;
    /// `vim.operate` with `replace_with_register` replaces either way.
    pub fn set_replace_with_register(&mut self, on: bool) {
        self.replace_with_register = on;
    }

    /// Leaves the error `message` for the host, failing the key: what
    /// changed is only that.
    pub(crate) fn fail(&mut self, message: impl Into<String>) -> Dispatched<TransactionId> {
        self.message = Some(message.into());
        self.failed = true;
        Dispatched::new(Effect::Notify)
    }

    /// Sets what `.` repeats.
    /// Offers the text a session that ended in place `order` typed, for
    /// the `.` register; an older offer than one held is dropped.
    pub(crate) fn offer_inserted(&mut self, order: u64, text: String) {
        if self
            .pending_inserted
            .as_ref()
            .is_none_or(|(held, _)| order > *held)
        {
            self.pending_inserted = Some((order, text));
        }
    }

    pub(crate) fn set_last_change(&mut self, change: LastChange) {
        self.last_change = Some(change);
        self.changes += 1;
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn pending(&self) -> &Pending {
        &self.pending
    }

    /// The last error, such as a search's `E486: Pattern not found: foo`,
    /// or `:s`'s report ("3 substitutions on 2 lines"), for the host to
    /// show; the next key [`VimState::feed`] reads, or the next command
    /// line, clears it.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Clears [`VimState::message`], as the next key [`VimState::feed`]
    /// reads does: for a host whose keys don't all go through `feed`, such
    /// as those its keymap binds to commands.
    pub fn clear_message(&mut self) {
        self.message = None;
    }

    /// The file or window command (`:w`, `:q`, `:e` and the like) an ex line
    /// submitted, for the host to run: returned once, then `None`.
    pub fn take_host_command(&mut self) -> Option<HostCommand> {
        self.host_command.take()
    }

    /// The host command [`VimState::take_host_command`] would return, left
    /// where it is.
    pub fn host_command(&self) -> Option<&HostCommand> {
        self.host_command.as_ref()
    }

    /// Whether an ex line left a file or window command
    /// ([`VimState::take_host_command`]) the host hasn't taken.
    pub fn has_host_command(&self) -> bool {
        self.host_command.is_some()
    }

    /// The pattern a host paints as matches ('hlsearch', on as in nvim): the
    /// last search's, set by every search, `n`, `N`, `*` and `#` (even one
    /// that finds no match) and a pattern in an ex line's range, or `None`
    /// before the first and after `:noh` until the next.
    pub fn highlight(&self) -> Option<&Pattern> {
        self.highlight.as_ref()
    }

    /// The text an open command line starts with, for the host to put on
    /// it: `'<,'>` on an ex line from a visual mode, `.,.+{count-1}` on one
    /// after a count (`.` for 1), and otherwise (or with no line open)
    /// nothing.
    pub fn command_line_prefill(&self) -> &str {
        self.cmdline
            .as_ref()
            .map_or("", |line| line.prefill.as_str())
    }

    /// Makes `text` what the open command line starts with: for a host
    /// whose map typed it into a line it doesn't show, so its own line
    /// opens showing it, and a recording takes only what's typed after it
    /// ([`VimState::command_line_prefill`]). With no line open it does
    /// nothing.
    pub fn set_command_line_prefill(&mut self, text: impl Into<String>) {
        if let Some(line) = &mut self.cmdline {
            line.prefill = text.into();
        }
    }

    /// The last `f t F T` motion run, for `;` and `,`.
    pub fn last_find(&self) -> Option<Motion> {
        self.last_find
    }

    /// Types a line break at every caret, as `Enter` does in insert mode
    /// for a host without syntax: `EditorModel::newline`, which copies each
    /// row's indentation ([`VimState::line_break_in`] indents by the host's
    /// language). An open insert session is told the edit was a typed line
    /// break, so the indentation it made is 'autoindent', which `Esc` drops
    /// if nothing is typed after it on its row. Any other input,
    /// `EditorModel::newline` called directly included, is typed text,
    /// whatever it holds: only the host knows which keys typed what, so it
    /// calls this for `Enter` while the mode is insert or replace. In replace mode the line break
    /// overwrites nothing and drops the blanks it moves to the new row, as
    /// nvim's 'autoindent' does, and [`VimState::replace_backspace`] takes
    /// it out again, putting them back.
    ///
    /// It catches the model up first, so `newline`'s edits are the batches
    /// from the version it then reads to the one it leaves, whatever
    /// measuring text edits around them (see `EditorModel`'s char widths).
    /// The session is told before `newline` runs, since measuring may stamp
    /// the context through a clone of this state while it does, which
    /// follows the batches.
    pub fn line_break(&self, model: &mut EditorModel) {
        if self.mode == Mode::Replace {
            return crate::replace::line_break(self, model);
        }
        self.line_break_with(model, EditorModel::newline);
    }

    /// [`VimState::line_break`], indenting each new row as `host` suggests
    /// for the buffer's language (`EditorModel::newline_in`), as nvim's
    /// `Enter` does with a filetype's 'indentexpr', and copying the row's
    /// indentation where it has no suggestion. The indentation is the line
    /// break's, in every batch `newline_in` makes, for `Esc` to drop if it
    /// goes unused. Replace mode doesn't indent by language.
    pub fn line_break_in(&self, model: &mut EditorModel, host: &dyn EditorHost) {
        if self.mode == Mode::Replace {
            return crate::replace::line_break(self, model);
        }
        self.line_break_with(model, |model| model.newline_in(host));
    }

    /// [`VimState::line_break`], with `newline` typing the line break:
    /// `EditorModel::newline`, or in tests, that and what measuring text
    /// does meanwhile.
    pub(crate) fn line_break_with(
        &self,
        model: &mut EditorModel,
        newline: impl FnOnce(&mut EditorModel),
    ) {
        model.sync_display();
        let from = model.buffer().version();
        if let Some(session) = &self.session {
            session.typing_line_break(from);
        }
        newline(model);
        if let Some(session) = &self.session {
            // With no caret to type at, it typed nothing: no batch is its.
            match model.buffer().version() {
                to if to == from => session.forget_line_break(from),
                to => session.line_break_typed(from, to),
            }
            self.restore_indent(model);
        }
    }

    /// What `.` repeats: the last change and the text its insert session
    /// typed.
    pub fn last_change(&self) -> Option<&LastChange> {
        self.last_change.as_ref()
    }

    /// `vim.mode`: `normal`, `insert`, `replace`, `visual`, `visual_line`,
    /// `visual_block`, `cmdline`, or
    /// `operator_pending` while an operator waits for its target.
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Normal if self.pending.has_operator() => "operator_pending",
            Mode::Normal => "normal",
            Mode::Insert => "insert",
            Mode::Replace => "replace",
            Mode::Visual(VisualKind::Char) => "visual",
            Mode::Visual(VisualKind::Line) => "visual_line",
            Mode::Visual(VisualKind::Block) => "visual_block",
            Mode::CmdLine(_) => "cmdline",
        }
    }

    /// Runs [`step`] on the pending input, read in the mode it's in, and
    /// keeps what it leaves pending: nothing, unless it waits. It clears
    /// the [`VimState::message`].
    ///
    /// `/` and `?` (not as a prefix's key: `f/` is a find) open the command
    /// line, which [`step`] can't (it forwards them): they commit
    /// `vim.command_line`, carrying what was pending (`:` is [`step`]'s).
    /// In `cmdline` mode the
    /// host's line takes the keys, not this: `<Esc>` commits
    /// `vim.command_line_cancel`, for a host with no line, and every other
    /// key is forwarded.
    ///
    /// The jump's keys are read here too ([`crate::jump`]): `{c2}` with no
    /// count commits `vim.jump`, `<CR>` as `{c1}` repeats the last jump's
    /// chars, `s` and `S` start a jump when [`VimState::set_jump_on_s`] is
    /// on (as `/` is taken, with no prefix pending), and while labels wait
    /// every key goes to them first.
    ///
    /// Before the `q` that ends a recording, the key goes through the
    /// remaps ([`VimState::set_remaps`]), in Normal and the visual modes
    /// with nothing pending but a count, a register or an operator: while
    /// it starts a longer map this returns [`Step::Wait`] with the pending
    /// input as it was ([`VimState::remap_waiting`]), and a map that runs
    /// leaves its keys for the host to feed, as a `@` does
    /// ([`VimState::take_replay`]). A map's `<Cmd>…<CR>` among them is a key
    /// of its own, which the host feeds here whatever the mode
    /// ([`VimState::remap_command_next`]): an ex line commits `vim.ex_line`,
    /// and `fe:{command id}` waits, leaving the command for the host
    /// ([`VimState::take_remap_command`]). It answers no `:s` confirmation
    /// and picks no jump label: running, it ends them.
    ///
    /// After `<C-o>` ([`VimState::one_command`]), a key that ends the
    /// command without committing one (`<Esc>` with nothing pending, or
    /// after a count or an operator, a key that fails) commits
    /// `vim.resume_insert`, which resumes the session.
    pub fn feed(&mut self, key: VimKey) -> Step {
        self.provenance.start_read();
        let step = self.feed_key(key);
        match step {
            Step::Commit(invocation) => {
                self.provenance.commit(&invocation);
                Step::Commit(invocation)
            }
            _ if self.one_command_done() => {
                let resume =
                    Invocation::of("vim.resume_insert").expect("vim command IDs are valid");
                self.provenance.commit(&resume);
                Step::Commit(resume.source(Source::Keyboard))
            }
            _ => step,
        }
    }

    /// [`VimState::feed`], but for the end of `<C-o>`'s command.
    fn feed_key(&mut self, key: VimKey) -> Step {
        let taken = self.remap.next_tag();
        let tag = taken.clone().unwrap_or(TYPED);
        self.expanded = tag.origin.is_some_and(Origin::expansion);
        let cmd = tag.cmd.is_some();
        if !cmd && let Some(step) = self.confirm_key(key, tag.origin) {
            return step;
        }
        self.message = None;
        self.failed = false;
        let origin = tag.origin.unwrap_or_else(|| Origin::Typed {
            replayed: self.macros.count(),
        });
        self.provenance.read_from(origin);
        if taken.is_some() && self.remap.over_cap() {
            self.pending = Pending::new();
            self.fail(format!(
                "macro stopped: it would replay more than {REPLAY_CAP} keys"
            ));
            return Step::Cancel;
        }
        if !cmd && let Some(step) = self.unrecorded(origin, |state| jump::label_key(state, key)) {
            if let Step::Wait(pending) = &step {
                self.pending = pending.clone();
            }
            return step;
        }
        let remap = tag.remap && !self.remap.off;
        match crate::remap::stage(self, key, origin, remap, tag.cmd) {
            Staged::Key(key, origin) => self.unrecorded(origin, |state| state.reduce(key, origin)),
            Staged::Cmd(cmd) => crate::remap::run_cmd(self, &cmd),
            Staged::Done(step) => step,
        }
    }

    /// An answer to a `:s` with `c` ([`crate::substitute::confirm_key`]).
    /// A key handed out with its `origin` was counted already, or is a
    /// map's: the macros are set aside while it's read, and it's recorded
    /// only if typed.
    fn confirm_key(&mut self, key: VimKey, origin: Option<Origin>) -> Option<Step> {
        let Some(origin) = origin else {
            return crate::substitute::confirm_key(self, key);
        };
        let held = self.macros.hold();
        let answer = crate::substitute::confirm_key(self, key);
        self.macros.release(held);
        if answer.is_some() && origin.recorded() {
            self.macros.command_start();
            self.macros.record(key);
        }
        answer
    }

    /// Runs `read` with the macros set aside if `origin` is a map's or a
    /// replay's, as `:normal` does: no recording takes its keys, nor a jump
    /// they make. A replay's key released late (by a timeout) was counted
    /// when it was first read, so the macros don't count it again.
    fn unrecorded<R>(&mut self, origin: Origin, read: impl FnOnce(&mut Self) -> R) -> R {
        if origin.recorded() {
            return read(self);
        }
        let held = self.macros.hold();
        let result = read(self);
        self.macros.release(held);
        result
    }

    /// [`VimState::feed`] past the remaps: `key`, from `origin`.
    fn reduce(&mut self, key: VimKey, origin: Origin) -> Step {
        // A map's `q` acts as typed.
        let replayed = origin == Origin::Typed { replayed: true };
        // `q` with nothing pending, in normal or a visual mode, ends a
        // recording (and isn't in it). A replay's `q` (after a count or a
        // register too, which it drops) does nothing, as nvim doesn't record
        // or stop recording while it plays a register.
        if key == VimKey::Char('q') && matches!(self.mode, Mode::Normal | Mode::Visual(_)) {
            if replayed && self.pending.only_count_or_register() {
                self.pending = Pending::new();
                return Step::Wait(Pending::new());
            }
            if !replayed && self.pending.kind().is_none() && self.macros.recording().is_some() {
                let stop = Invocation::of("vim.record_stop").expect("vim command IDs are valid");
                return Step::Commit(stop.source(Source::Keyboard));
            }
        }
        let before = self.pending.kind();
        if before.is_none() {
            self.macros.command_start();
        }
        if origin.recorded() {
            self.macros.record(key);
        }
        let kind = match self.mode {
            Mode::Visual(kind) => Some(kind),
            Mode::CmdLine(_) => return cmdline::fallback(key),
            _ => None,
        };
        // An arrow with nothing pending is `h`, `l`, `k` or `j`, as the
        // keymap's `vim.move` makes a typed one: here one a map or a replay
        // left. After anything pending, `step` reads it so itself.
        let key = match (key, self.pending.kind().is_none() && kind.is_none()) {
            (VimKey::Left, true) => VimKey::Char('h'),
            (VimKey::Right, true) => VimKey::Char('l'),
            (VimKey::Up, true) => VimKey::Char('k'),
            (VimKey::Down, true) => VimKey::Char('j'),
            (key, _) => key,
        };
        let pending = std::mem::take(&mut self.pending).in_visual(kind);
        let result = match jump::key(self, pending, key).or_else(|p| p.command_line(key)) {
            Ok(step) => step,
            Err(pending) => step(pending, key),
        };
        // `gr` is the host's while ReplaceWithRegister is off.
        let result = match result {
            result if !self.replace_with_register && replace_register::takes(&result) => {
                Step::Cancel
            }
            result => result,
        };
        jump::fed(self, before, &result);
        match &result {
            Step::Wait(pending) => self.pending = pending.clone(),
            // `<Esc>` cancelling isn't an error in Vim.
            Step::Cancel if key == VimKey::Esc => {}
            Step::Cancel | Step::Forward => self.failed = true,
            Step::Commit(_) => {}
        }
        result
    }

    /// Reads a key the host takes itself rather than through
    /// [`VimState::feed`]: in insert mode (typed text, `<CR>`, `<BS>`,
    /// `<Esc>`) and on the command line. The host calls it for each such
    /// key, before running what the key does: it's how a recording gets
    /// them, and how a replay's keys are counted.
    ///
    /// A map's keys aren't recorded, nor counted as a replay's. A key
    /// other than `<C-n>`, `<C-p>`, `<C-e>` and `<C-y>` ends a keyword
    /// completion ([`VimState::completion`]).
    pub fn record_key(&mut self, key: VimKey) {
        self.provenance.start_read();
        self.failed = false;
        self.completion_key(key);
        let origin = self
            .remap
            .next_tag()
            .and_then(|tag| tag.origin)
            .unwrap_or_else(|| Origin::Typed {
                replayed: self.macros.count(),
            });
        self.expanded = origin.expansion();
        self.provenance.read_from(origin);
        if matches!(self.mode, Mode::CmdLine(_))
            && matches!(key, VimKey::Enter | VimKey::Ctrl('j') | VimKey::Esc)
        {
            self.provenance.commit_line();
        }
        self.remap.read(origin);
        if origin.recorded() {
            self.macros.record(key);
        }
    }

    /// Asks the remaps what `key` comes to, typed in insert, replace or
    /// cmdline mode: the host calls this before it types or edits for the
    /// key, then types what it returns, calling [`VimState::record_key`]
    /// for each key as it does. [`RemapInsert::Wait`]: the key starts a
    /// longer map or ran one, and the host types nothing yet. A map's right
    /// side, and the keys held for it that go on as typed, are the host's
    /// to feed ([`VimState::take_replay`]), as under [`VimState::feed`];
    /// so are held keys after [`VimState::remap_timeout`]. A key that
    /// breaks a held prefix returns the prefix's first key, the rest
    /// matched again in the mode it leaves.
    pub fn remap_insert(&mut self, key: VimKey) -> crate::RemapInsert {
        crate::remap::insert(self, key)
    }

    /// Replaces the remaps, at once: keys held for a longer match go on as
    /// typed, unmapped, before the next key read
    /// ([`VimState::take_replay`]).
    pub fn set_remaps(&mut self, set: Arc<RemapSet>) {
        self.remap.replace(set);
    }

    pub fn remaps(&self) -> &RemapSet {
        self.remap.set()
    }

    /// Whether keys are held for a longer map: [`VimState::feed`] waits
    /// for the next key or [`VimState::remap_timeout`], and `vim.pending`
    /// is set with `vim.pending_kind` `remap`.
    pub fn remap_waiting(&self) -> bool {
        self.remap.waiting()
    }

    /// The keys held for a longer map, in order: empty unless
    /// [`VimState::remap_waiting`].
    pub fn remap_held(&self) -> Vec<VimKey> {
        self.remap.held_keys()
    }

    /// No key came for `CHORD_TIMEOUT` (nvim's 'timeoutlen') while keys
    /// were held for a longer map: the longest complete map among them
    /// runs, the keys after it to be matched again, or with none, the
    /// first goes on as typed and the rest are matched again. Either way
    /// the keys are the host's to feed ([`VimState::take_replay`]).
    pub fn remap_timeout(&mut self) {
        crate::remap::timeout(self);
    }

    /// Whether the next key the host feeds of those it took
    /// ([`VimState::take_replay`]) stands for a map's `<Cmd>…<CR>`: the host
    /// feeds it through [`VimState::feed`] in any mode, insert and
    /// `cmdline` too, where it would type or forward any other key.
    pub fn remap_command_next(&self) -> bool {
        self.remap.cmd_next()
    }

    /// The next fastedit command a map's `<Cmd>fe:{command id}<CR>` ran
    /// ([`VimState::feed`]), each once, then `None`: the host dispatches it
    /// as a keymap binding, from the keyboard in the editor's context,
    /// before it feeds the map's next key. Those a `:normal` ran wait until
    /// the command that ran it is over.
    ///
    /// One a macro's replay ran (through its maps or not) has the source
    /// [`Source::Replay`], and runs as the replay's, as one a `:normal` ran
    /// runs as the `:normal`'s: a shell command it asks for is refused, as
    /// the replay's own keys' are, when [`crate::dispatch`] runs it (until a
    /// key of the user's is read, or the host says it runs it now,
    /// [`VimState::running_remap_command`]).
    pub fn take_remap_command(&mut self) -> Option<RemapCommand> {
        let (command, provenance) = self.remap_commands.pop_front()?;
        self.provenance.take(&command.0, provenance);
        Some(command)
    }

    /// The host runs `command`, which [`VimState::take_remap_command`]
    /// gave it last, now: one a macro's replay or a `:normal` left runs as
    /// theirs, whatever was read since it was taken.
    pub fn running_remap_command(&mut self, command: &RemapCommand) {
        self.provenance.running(&command.0);
    }

    /// Whether the host runs the fastedit command a `:normal` reaches, and
    /// then resumes the `:normal` (`vim.normal_resume`): it suspends there
    /// with its remaining keys and range rows instead of stopping with
    /// "fe: commands don't run inside :normal". Off by default.
    pub fn set_normal_resumes(&mut self, resumes: bool) {
        self.normal_resumes = resumes;
    }

    /// Whether a `:normal` waits for its fastedit command
    /// ([`VimState::take_remap_command`]) to be run, to be resumed by
    /// `vim.normal_resume`.
    pub fn has_normal_suspended(&self) -> bool {
        self.normal_suspended.is_some()
    }

    /// Whether a map left a fastedit command the host hasn't taken
    /// ([`VimState::take_remap_command`]).
    pub fn has_remap_command(&self) -> bool {
        !self.remap_commands.is_empty()
    }

    /// Whether the last key read, through [`VimState::feed`] or
    /// [`VimState::record_key`], was a map's right side: not typed, nor a
    /// `@`'s. A host tells by it a command line a map ran, whose file or
    /// window command ([`VimState::take_host_command`]) it runs, from a
    /// macro's, whose it drops.
    pub fn expanding(&self) -> bool {
        self.expanded
    }

    /// Sets aside the remap stream's keys for a `:normal` (see
    /// [`crate::remap::RemapState::save`]).
    pub(crate) fn remap_save(&mut self) -> crate::remap::RemapState {
        self.remap.save()
    }

    pub(crate) fn remap_restore(&mut self, saved: crate::remap::RemapState) {
        self.remap.restore(saved);
    }

    /// Drops the keys the remap stage holds or has resolved.
    pub(crate) fn remap_clear(&mut self) {
        self.remap.held_clear();
    }

    /// Sets whether keys skip the maps (`:normal!`), returning what it was.
    pub(crate) fn remap_off(&mut self, off: bool) -> bool {
        std::mem::replace(&mut self.remap.off, off)
    }

    /// The register `q` is recording into (`a` for `qA` too), while it
    /// records: `vim.recording` on the context.
    pub fn recording(&self) -> Option<char> {
        self.macros.recording()
    }

    /// The keys a `@` (`vim.play`) left for the host to feed, then none
    /// until the next. The host feeds them as typed keys, through
    /// [`VimState::feed`] or [`VimState::record_key`], before any key it
    /// still holds, and after each takes any new ones (a `@` inside the
    /// macro) to feed first; once it has fed them all and takes none, the
    /// replay is over. A replay's keys aren't recorded, and its `q` does
    /// nothing (dropping a count or register before it). A line break in the register is `<C-j>` (nvim's `<NL>`),
    /// which the host takes as `<CR>` in insert mode and on the command
    /// line. On the command line a replay's `<Esc>` runs the line, as in
    /// nvim (`:help c_<Esc>`).
    ///
    /// If a key of the replay fails ([`VimState::failed`], a key the host
    /// forwards or a command that returns an error), the host drops the
    /// rest of the replay's keys and calls [`VimState::stop_replay`], as
    /// nvim's error stops it. A `@` that would take a replay past
    /// 1,000,000 keys in all fails with a message, so recursive macros end.
    ///
    /// The keys a map resolved to come after a `@`'s, and go the same way:
    /// its right side, and the keys held for it that go on as typed. A map
    /// whose key fails drops the rest of the map, so the host calls
    /// [`VimState::stop_replay`] then too.
    pub fn take_replay(&mut self) -> Vec<VimKey> {
        let replay = self.macros.take();
        self.remap.take(replay)
    }

    /// The host dropped the rest of a replay or a map's keys: it's over,
    /// and the keys read from now are typed. The fastedit commands maps left
    /// ([`VimState::take_remap_command`]) are dropped too.
    pub fn stop_replay(&mut self) {
        self.macros.stop();
        self.remap.stop();
        self.remap_commands.clear();
        self.normal_suspended = None;
        self.provenance.stop();
    }

    /// Whether the last key read failed, as nvim's errors and beeps do,
    /// which stops a replay: a motion that couldn't move (`j` on the last
    /// row, `l` at a row's end, `w` at the text's end or a counted `w`
    /// running out, but not `t` onto the next character), an operator whose
    /// target didn't resolve, a visual text object that selects nothing or
    /// runs out part way, `J` on the last row, `~` on an empty row, `r` past
    /// a row's end, `u` or `<C-r>` running out of changes (even part way),
    /// `.` or `gv` with nothing to act on, a put or `@` of an empty
    /// register, a command that left an error in [`VimState::message`], or
    /// a key that cancelled what was pending (not `<Esc>`) or was
    /// forwarded.
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Publishes the mode on `frame`, the editor's own: `vim.mode`, the flag
    /// `vim.visual` in a visual mode, `vim.cmdline_kind` in `cmdline` mode,
    /// the flag `vim.insert_pending` while `<C-o>`'s command runs, the flag
    /// `vim.replace_with_register` while `gr` is ReplaceWithRegister's,
    /// plus the flag `vim.pending` and
    /// `vim.pending_kind` while anything is pending, a bare count or a
    /// completed `"a` included, and `remap` while keys are held for a
    /// longer map.
    ///
    /// The host stamps the context for every key (and each frame), so an
    /// open insert session follows the buffer's edits made since then,
    /// rather than holding them all until `Esc`.
    pub fn stamp(&self, frame: ContextFrame) -> ContextFrame {
        if let Some(session) = &self.session {
            session.follow_edits();
        }
        let mut frame = frame.set(MODE, self.mode_name());
        match self.mode {
            Mode::Visual(_) => frame = frame.flag(VISUAL),
            _ => {
                frame.flags.remove(VISUAL);
            }
        }
        match self.mode {
            Mode::CmdLine(kind) => frame = frame.set(CMDLINE_KIND, kind.name()),
            _ => {
                frame.values.remove(CMDLINE_KIND);
            }
        }
        match self.replace_with_register {
            true => frame = frame.flag(REPLACE_WITH_REGISTER),
            false => {
                frame.flags.remove(REPLACE_WITH_REGISTER);
            }
        }
        match self.one_command {
            Some(_) => frame = frame.flag(INSERT_PENDING),
            None => {
                frame.flags.remove(INSERT_PENDING);
            }
        }
        match self.macros.recording() {
            Some(register) => frame = frame.set(RECORDING, register.to_string()),
            None => {
                frame.values.remove(RECORDING);
            }
        }
        match self.pending.kind() {
            _ if self.remap.waiting() => frame.flag(PENDING).set(PENDING_KIND, "remap"),
            Some(kind) => frame.flag(PENDING).set(PENDING_KIND, kind.name()),
            None => {
                frame.flags.remove(PENDING);
                frame.values.remove(PENDING_KIND);
                frame
            }
        }
    }
}

/// `fe_editor::commands::context()` with its editor frame stamped by
/// `state`: the context a focused vim editor dispatches in.
pub fn context(state: &VimState) -> ContextStack {
    fe_editor::commands::context()
        .frames()
        .iter()
        .map(|frame| match frame.name == CONTEXT {
            true => state.stamp(frame.clone()),
            false => frame.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::HeadlessVim;

    #[test]
    fn clear_message_clears_only_the_message() {
        let mut vim = HeadlessVim::new("one\nfoo", (0, 0));
        vim.feed("/zzz<CR>").unwrap();
        assert_eq!(vim.state().message(), Some("E486: Pattern not found: zzz"));
        let mut expected = vim.state().clone();
        expected.message = None;
        vim.state_mut().clear_message();
        assert_eq!(vim.state().message(), None);
        assert_eq!(format!("{:?}", vim.state()), format!("{expected:?}"));
        // The search it left is still the last one.
        vim.feed("n").unwrap();
        assert_eq!(vim.state().message(), Some("E486: Pattern not found: zzz"));
    }
}
