//! Vim's per-editor state and the context keys it publishes.

use fe_buffer::Version;
use fe_command::{ContextFrame, ContextStack};
use fe_editor::commands::CONTEXT;

use crate::{Motion, Pending, Step, VimKey, step};

/// The context key holding the mode's name.
const MODE: &str = "vim.mode";
/// The flag set while a command is incomplete.
const PENDING: &str = "vim.pending";
/// What the incomplete command waits on: [`crate::PendingKind::name`].
const PENDING_KIND: &str = "vim.pending_kind";

/// The editing mode. Insert comes with card 3, the visual modes later.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Normal,
}

/// Each caret's head and its goal column.
pub(crate) type Goals = Vec<(usize, Option<usize>)>;

/// One editor's vim state: its mode, the pending input, each caret's goal
/// column and the last character search.
#[derive(Clone, Debug, Default)]
pub struct VimState {
    mode: Mode,
    pending: Pending,
    /// Each caret's head and the display column it keeps through `j`, `k`,
    /// `gg` and `G` (Vim's `curswant`; `usize::MAX` is the line end, after
    /// `$`; `None` is the caret's own column), as of a version of the text.
    pub(crate) goals: Option<(Version, Goals)>,
    pub(crate) last_find: Option<Motion>,
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

    /// `vim.mode`: `normal`, or `operator_pending` while an operator waits
    /// for its target.
    pub fn mode_name(&self) -> &'static str {
        match self.mode {
            Mode::Normal if self.pending.has_operator() => "operator_pending",
            Mode::Normal => "normal",
        }
    }

    /// Runs [`step`] on the pending input and keeps what it leaves pending:
    /// nothing, unless it waits.
    pub fn feed(&mut self, key: VimKey) -> Step {
        let result = step(std::mem::take(&mut self.pending), key);
        if let Step::Wait(pending) = &result {
            self.pending = pending.clone();
        }
        result
    }

    /// Publishes the mode on `frame`, the editor's own: `vim.mode`, plus the
    /// flag `vim.pending` and `vim.pending_kind` while anything is pending,
    /// a bare count or a completed `"a` included.
    pub fn stamp(&self, frame: ContextFrame) -> ContextFrame {
        let mut frame = frame.set(MODE, self.mode_name());
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
