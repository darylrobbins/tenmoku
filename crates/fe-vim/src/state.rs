//! Vim's per-editor state and the context keys it publishes.

use fe_buffer::Version;
use fe_command::{ContextFrame, ContextStack};
use fe_editor::{EditorModel, commands::CONTEXT};

use crate::{
    LastChange, Motion, Pending, Step, VimKey, VisualKind,
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

/// The editing mode.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Normal,
    /// An insert session is open: typed text goes to the editor, not
    /// [`step`].
    Insert,
    /// `v` or `V`: keys go to [`step`], which reads operators as acting on
    /// the selection.
    Visual(VisualKind),
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
    pub(crate) carets: Carets,
    /// Set while `.` replays a change inside its own transaction.
    pub(crate) replaying: bool,
    /// Set in a visual mode: its anchor and cursor.
    pub(crate) visual: Option<Visual>,
    /// The last visual mode's extent, for `gv`, `'<` and `'>`.
    pub(crate) last_visual: Option<LastVisual>,
    /// How much text the last change over a selection took, for `.`.
    pub(crate) visual_extent: Option<Extent>,
}

impl VimState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn pending(&self) -> &Pending {
        &self.pending
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

    /// `vim.mode`: `normal`, `insert`, `visual`, `visual_line`, or
    /// `operator_pending` while an operator waits for its target.
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Normal if self.pending.has_operator() => "operator_pending",
            Mode::Normal => "normal",
            Mode::Insert => "insert",
            Mode::Visual(VisualKind::Char) => "visual",
            Mode::Visual(VisualKind::Line) => "visual_line",
        }
    }

    /// Runs [`step`] on the pending input, read in the mode it's in, and
    /// keeps what it leaves pending: nothing, unless it waits.
    pub fn feed(&mut self, key: VimKey) -> Step {
        let visual = matches!(self.mode, Mode::Visual(_));
        let pending = std::mem::take(&mut self.pending).in_visual(visual);
        let result = step(pending, key);
        if let Step::Wait(pending) = &result {
            self.pending = pending.clone();
        }
        result
    }

    /// Publishes the mode on `frame`, the editor's own: `vim.mode`, the flag
    /// `vim.visual` in a visual mode, plus the flag `vim.pending` and
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
