//! Vim's per-editor state and the context keys it publishes.

use fe_buffer::Version;
use fe_command::{ContextFrame, ContextStack};
use fe_editor::{EditorModel, commands::CONTEXT};

use crate::ex::HostCommand;
use crate::search::Pattern;
use crate::{
    CommandLineKind, LastChange, Motion, Pending, Step, VimKey, VisualKind,
    cmdline::{self, CommandLine},
    history::Carets,
    insert::{Closing, Session},
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

/// The editing mode.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Normal,
    /// An insert session is open: typed text goes to the editor, not
    /// [`step`].
    Insert,
    /// `v`, `V` or `<C-v>`: keys go to [`step`], which reads operators as
    /// acting on the selection.
    Visual(VisualKind),
    /// A command line is open (`/`, `?`, `:`): the host's line takes the
    /// keys until it submits or cancels it.
    CmdLine(CommandLineKind),
}

/// Each caret's head and its goal column.
pub(crate) type Goals = Vec<(usize, Option<usize>)>;

/// One editor's vim state: its mode, the pending input, each caret's goal
/// column, the last character search, the open insert session, the last
/// change for `.`, and the carets before each change for `u` and `<C-r>`.
#[derive(Clone, Debug, Default)]
pub struct VimState {
    pub(crate) mode: Mode,
    pub(crate) pending: Pending,
    /// Each caret's head and the display column it keeps through `j`, `k`,
    /// `gg` and `G` (Vim's `curswant`; `usize::MAX` is the line end, after
    /// `$`; `None` is the caret's own column), as of a version of the text.
    pub(crate) goals: Option<(Version, Goals)>,
    pub(crate) last_find: Option<Motion>,
    /// Set in insert mode: the session `vim.normal_mode` ends.
    pub(crate) session: Option<Session>,
    /// A session this state left while a clone was ending it: the outcome
    /// is adopted once it's published (see `insert::adopt_closing`).
    pub(crate) closing: Option<Closing>,
    pub(crate) last_change: Option<LastChange>,
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
}

impl VimState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets what `.` repeats.
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

    /// The last `f t F T` motion run, for `;` and `,`.
    pub fn last_find(&self) -> Option<Motion> {
        self.last_find
    }

    /// Types a line break at every caret, as `Enter` does in insert mode:
    /// `EditorModel::newline`, which copies each row's indentation. An open
    /// insert session is told the edit was a typed line break, so the
    /// indentation it copied is 'autoindent', which `Esc` drops if nothing
    /// is typed after it on its row. Any other input, `EditorModel::newline`
    /// called directly included, is typed text, whatever it holds: only the
    /// host knows which keys typed what, so it calls this for `Enter` while
    /// the mode is insert.
    ///
    /// It catches the model up first, so `newline`'s edit is the batch from
    /// the version it then reads, whatever measuring text edits around it
    /// (see `EditorModel`'s char widths). The session is told before
    /// `newline` runs, since measuring may stamp the context through a clone
    /// of this state while it does, which follows the batch.
    pub fn line_break(&self, model: &mut EditorModel) {
        self.line_break_with(model, EditorModel::newline);
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
            session.typed_line_break(from);
        }
        newline(model);
        // With no caret to type at, it typed nothing: no batch is its.
        if let Some(session) = &self.session
            && model.buffer().version() == from
        {
            session.forget_line_break(from);
        }
    }

    /// What `.` repeats: the last change and the text its insert session
    /// typed.
    pub fn last_change(&self) -> Option<&LastChange> {
        self.last_change.as_ref()
    }

    /// `vim.mode`: `normal`, `insert`, `visual`, `visual_line`,
    /// `visual_block`, `cmdline`, or
    /// `operator_pending` while an operator waits for its target.
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Normal if self.pending.has_operator() => "operator_pending",
            Mode::Normal => "normal",
            Mode::Insert => "insert",
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
    pub fn feed(&mut self, key: VimKey) -> Step {
        self.message = None;
        let kind = match self.mode {
            Mode::Visual(kind) => Some(kind),
            Mode::CmdLine(_) => return cmdline::fallback(key),
            _ => None,
        };
        let pending = std::mem::take(&mut self.pending).in_visual(kind);
        let result = match pending.command_line(key) {
            Ok(opened) => opened,
            Err(pending) => step(pending, key),
        };
        if let Step::Wait(pending) = &result {
            self.pending = pending.clone();
        }
        result
    }

    /// Publishes the mode on `frame`, the editor's own: `vim.mode`, the flag
    /// `vim.visual` in a visual mode, `vim.cmdline_kind` in `cmdline` mode,
    /// plus the flag `vim.pending` and
    /// `vim.pending_kind` while anything is pending, a bare count or a
    /// completed `"a` included.
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
        match self.pending.kind() {
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
