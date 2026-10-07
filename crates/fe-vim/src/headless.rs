//! A vim editor without a UI, for tests and the Neovim oracle.

use fe_buffer::{Buffer, Point, TransactionId};
use fe_command::{DispatchError, Dispatched, Invocation, Registry, Source};
use fe_editor::{EditorModel, commands::HeadlessHost};

use crate::{
    KeyError, Mode, Registers, Step, VimCommands, VimKey, VimState, VimTarget, context, parse_keys,
    registry,
};

/// An [`EditorModel`] with a [`HeadlessHost`], a [`VimState`], registers
/// and vim's registry, fed keys the way the GPUI host routes them: through
/// [`crate::step`] in normal mode, and in insert mode as the editor's text
/// input and the keymap would take them.
pub struct HeadlessVim {
    model: EditorModel,
    host: HeadlessHost,
    state: VimState,
    registers: Registers,
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
            registers: Registers::new(),
            registry: registry(),
        }
    }

    /// Feeds `keys` (vim notation) one at a time, the visual selection
    /// painted again before each ([`HeadlessVim::sync_visual`]). A committed invocation is
    /// stamped with the context as it is once the reducer has cleared its
    /// pending input, then dispatched. Stops at the first key forwarded or
    /// invocation that fails.
    ///
    /// In insert mode [`crate::step`] isn't called: a character goes to
    /// [`EditorModel::insert`], `<CR>` to [`EditorModel::newline`] (through
    /// [`VimState::line_break`], as the host's `Enter` does), `<BS>` to
    /// [`EditorModel::backspace`], and `<Esc>` dispatches `vim.normal_mode`;
    /// any other key is forwarded.
    pub fn feed(&mut self, keys: &str) -> Result<(), FeedError> {
        for key in parse_keys(keys)? {
            self.sync_visual();
            if self.state.mode() == Mode::Insert {
                self.type_key(key)?;
                continue;
            }
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

    /// One key in insert mode. The keymap looks it up in the stamped
    /// context first, as the host's does.
    fn type_key(&mut self, key: VimKey) -> Result<(), FeedError> {
        let context = context(&self.state);
        match key {
            VimKey::Char(ch) => self.model.insert(ch.encode_utf8(&mut [0; 4])),
            VimKey::Enter => self.state.line_break(&mut self.model),
            VimKey::Backspace => self.model.backspace(),
            VimKey::Esc => {
                let invocation = Invocation::of("vim.normal_mode")
                    .expect("vim command IDs are valid")
                    .source(Source::Keyboard)
                    .context(context);
                self.dispatch(&invocation)?;
            }
            _ => return Err(FeedError::Forwarded(key)),
        }
        Ok(())
    }

    /// Paints the visual selection again from the vim cursor and anchor, as
    /// the host does once the model catches up with other origins' edits
    /// ([`VimState::sync_visual`]); `feed` does it before each key.
    pub fn sync_visual(&mut self) {
        self.state.sync_visual(&mut self.model);
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
            registers: &mut self.registers,
        };
        self.registry.dispatch(&mut target, invocation)
    }

    pub fn text(&self) -> String {
        let text = self.model.text();
        text.slice_to_string(0..text.len())
    }

    /// The primary caret's head as a row and a byte column; in a visual
    /// mode, the vim cursor ([`VimState::visual_cursor`]).
    pub fn cursor(&self) -> (u32, usize) {
        let head = self.state.visual_cursor(&self.model);
        self.point(head.unwrap_or_else(|| self.model.primary_selection().head()))
    }

    /// In a visual mode, the anchor as a row and a byte column.
    pub fn visual_anchor(&self) -> Option<(u32, usize)> {
        let anchor = self.state.visual_anchor(&self.model)?;
        Some(self.point(anchor))
    }

    /// `'<` or `'>` as a row and a byte column ([`VimState::mark`]).
    pub fn mark(&self, name: char) -> Option<(u32, usize)> {
        let mark = self.state.mark(name)?;
        Some(mark.position(self.model.snapshot()))
    }

    fn point(&self, offset: usize) -> (u32, usize) {
        let point = self.model.text().offset_to_point(offset);
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

    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    pub fn registers_mut(&mut self) -> &mut Registers {
        &mut self.registers
    }

    /// The host, whose `clipboard` is `+` and `*`.
    pub fn host(&self) -> &HeadlessHost {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut HeadlessHost {
        &mut self.host
    }
}
