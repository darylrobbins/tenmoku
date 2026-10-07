//! A vim editor without a UI, for tests and the Neovim oracle.

use fe_buffer::{Buffer, Point, TransactionId};
use fe_command::{DispatchError, Dispatched, Invocation, Registry};
use fe_editor::{EditorModel, commands::HeadlessHost};

use crate::{
    KeyError, Step, VimCommands, VimKey, VimState, VimTarget, context, parse_keys, registry,
};

/// An [`EditorModel`] with a [`HeadlessHost`], a [`VimState`] and vim's
/// registry, fed keys the way the GPUI host routes them in normal mode.
pub struct HeadlessVim {
    model: EditorModel,
    host: HeadlessHost,
    state: VimState,
    registry: Registry<VimCommands>,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum FeedError {
    #[error(transparent)]
    Keys(#[from] KeyError),
    /// Nothing handled the key: it would have gone back to the keymap.
    #[error("{0:?} isn't vim's: it was forwarded")]
    Forwarded(VimKey),
    #[error(transparent)]
    Dispatch(#[from] DispatchError),
}

impl HeadlessVim {
    /// `text` with a single caret at `cursor`, a row and a byte column.
    pub fn new(text: &str, cursor: (u32, usize)) -> Self {
        let mut model = EditorModel::new(Buffer::new(text), None);
        let offset = model
            .text()
            .point_to_offset(Point::new(cursor.0, cursor.1 as u32));
        model.set_cursor(offset);
        Self {
            model,
            host: HeadlessHost::new(),
            state: VimState::new(),
            registry: registry(),
        }
    }

    /// Feeds `keys` (vim notation) one at a time. A committed invocation is
    /// stamped with the context as it is once the reducer has cleared its
    /// pending input, then dispatched. Stops at the first key forwarded or
    /// invocation that fails.
    pub fn feed(&mut self, keys: &str) -> Result<(), FeedError> {
        for key in parse_keys(keys)? {
            match self.state.feed(key) {
                Step::Wait(_) | Step::Cancel => {}
                Step::Forward => return Err(FeedError::Forwarded(key)),
                Step::Commit(invocation) => {
                    let invocation = invocation.context(context(&self.state));
                    self.dispatch(&invocation)?;
                }
            }
        }
        Ok(())
    }

    /// Runs one invocation as given, context included.
    pub fn dispatch(
        &mut self,
        invocation: &Invocation,
    ) -> Result<Dispatched<TransactionId>, DispatchError> {
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut self.host,
            state: &mut self.state,
        };
        self.registry.dispatch(&mut target, invocation)
    }

    pub fn text(&self) -> String {
        let text = self.model.text();
        text.slice_to_string(0..text.len())
    }

    /// The primary caret's head as a row and a byte column.
    pub fn cursor(&self) -> (u32, usize) {
        let point = self
            .model
            .text()
            .offset_to_point(self.model.primary_selection().head());
        (point.row, point.column as usize)
    }

    /// `vim.mode`'s value.
    pub fn mode(&self) -> &'static str {
        self.state.mode_name()
    }

    pub fn state(&self) -> &VimState {
        &self.state
    }

    pub fn model(&self) -> &EditorModel {
        &self.model
    }

    pub fn model_mut(&mut self) -> &mut EditorModel {
        &mut self.model
    }
}
