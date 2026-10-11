//! A vim editor without a UI, for tests and the Neovim oracle.

use std::collections::VecDeque;

use fe_buffer::{Buffer, Point, TransactionId};
use fe_command::{DispatchError, Dispatched, Invocation, Registry, Source};
use fe_editor::{EditorModel, commands::HeadlessHost};

use crate::{
    KeyError, Mode, Registers, Splits, Step, SubmitArgs, VimCommands, VimKey, VimState, VimTarget,
    context, parse_keys, registry,
};

/// An [`EditorModel`] with a [`HeadlessHost`], a [`VimState`], registers
/// and vim's registry, fed keys the way the GPUI host routes them: through
/// [`crate::step`] in normal mode, in insert mode as the editor's text
/// input and the keymap would take them, and in `cmdline` mode to a line of
/// its own, as the host's command line.
pub struct HeadlessVim {
    model: EditorModel,
    host: HeadlessHost,
    state: VimState,
    registers: Registers,
    registry: Registry<VimCommands>,
    /// The command line's text, in `cmdline` mode.
    line: String,
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
            line: String::new(),
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
    /// any other key is forwarded. Nor in `cmdline` mode: a character goes
    /// on the line, `<BS>` deletes its last (or on an empty line dispatches
    /// `vim.command_line_cancel`, as `<Esc>` does), `<CR>` dispatches
    /// `vim.command_line_submit` with the line; any other key is forwarded.
    /// In both, `<C-j>` (`<NL>`, a register's line break) is `<CR>`.
    /// Either way the key goes to [`VimState::record_key`] first.
    ///
    /// The keys a `@` leaves ([`VimState::take_replay`]) are fed next, as
    /// typed keys, before the rest of `keys`; a replay's `<Esc>` on the
    /// command line submits it, as in nvim. A replay's key that fails (one
    /// forwarded, an invocation that errs, or [`VimState::failed`]) drops
    /// the rest of the replay, and feeding goes on with `keys`.
    pub fn feed(&mut self, keys: &str) -> Result<(), FeedError> {
        let mut queue: VecDeque<(VimKey, bool)> = parse_keys(keys)?
            .into_iter()
            .map(|key| (key, false))
            .collect();
        while let Some((key, replayed)) = queue.pop_front() {
            let fed = self.feed_key(key, replayed);
            if replayed && (fed.is_err() || self.state.failed()) {
                while queue.front().is_some_and(|&(_, replayed)| replayed) {
                    queue.pop_front();
                }
                self.state.stop_replay();
                continue;
            }
            fed?;
            for key in self.state.take_replay().into_iter().rev() {
                queue.push_front((key, true));
            }
        }
        Ok(())
    }

    fn feed_key(&mut self, key: VimKey, replayed: bool) -> Result<(), FeedError> {
        self.sync_visual();
        match self.state.mode() {
            Mode::Insert => {
                self.state.record_key(key);
                return self.type_key(key);
            }
            Mode::CmdLine(_) => {
                self.state.record_key(key);
                return self.line_key(key, replayed);
            }
            _ => {}
        }
        match self.state.feed(key) {
            Step::Wait(_) | Step::Cancel => {}
            Step::Forward => return Err(FeedError::Forwarded(key)),
            Step::Commit(invocation) => {
                let invocation = invocation.context(context(&self.state));
                self.dispatch(&invocation)?;
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
            // `<C-j>` (`<NL>`) is `<CR>` here.
            VimKey::Enter | VimKey::Ctrl('j') => self.state.line_break(&mut self.model),
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

    /// One key in `cmdline` mode, as the host's line takes it; a replay's
    /// `<Esc>` submits it.
    fn line_key(&mut self, key: VimKey, replayed: bool) -> Result<(), FeedError> {
        let invocation = match key {
            VimKey::Char(ch) => {
                self.line.push(ch);
                return Ok(());
            }
            VimKey::Backspace if self.line.pop().is_some() => return Ok(()),
            VimKey::Esc if !replayed => Invocation::of("vim.command_line_cancel"),
            VimKey::Backspace => Invocation::of("vim.command_line_cancel"),
            VimKey::Enter | VimKey::Ctrl('j') | VimKey::Esc => {
                Invocation::of("vim.command_line_submit").map(|invocation| {
                    let text = std::mem::take(&mut self.line);
                    let args =
                        serde_json::to_value(SubmitArgs { text }).expect("vim args serialize");
                    invocation.args(args)
                })
            }
            _ => return Err(FeedError::Forwarded(key)),
        };
        let invocation = invocation
            .expect("vim command IDs are valid")
            .source(Source::Keyboard)
            .context(context(&self.state));
        self.dispatch(&invocation)?;
        Ok(())
    }

    /// Runs `command` as the host runs an editor command in insert mode,
    /// splitting the session around it ([`VimState::split_session`]).
    pub fn split_session<R>(
        &mut self,
        splits: Splits,
        command: impl FnOnce(&mut EditorModel) -> R,
    ) -> R {
        self.state.split_session(&mut self.model, splits, command)
    }

    /// Paints the visual selection again from the vim cursor and anchor, as
    /// the host does once the model catches up with other origins' edits
    /// ([`VimState::sync_visual`]); `feed` does it before each key.
    pub fn sync_visual(&mut self) {
        self.state.sync_visual(&mut self.model);
    }

    /// Runs one invocation as given, context included. One that opens a
    /// command line starts the line `feed` types on from
    /// [`VimState::command_line_prefill`]; one that ends it empties it.
    pub fn dispatch(
        &mut self,
        invocation: &Invocation,
    ) -> Result<Dispatched<TransactionId>, DispatchError> {
        let was_open = matches!(self.state.mode(), Mode::CmdLine(_));
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut self.host,
            state: &mut self.state,
            registers: &mut self.registers,
        };
        let dispatched = self.registry.dispatch(&mut target, invocation);
        // The line starts from its prefill, whichever way the last one
        // ended.
        if !was_open || !matches!(self.state.mode(), Mode::CmdLine(_)) {
            self.line.clear();
            self.line.push_str(self.state.command_line_prefill());
        }
        dispatched
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

    /// A mark as a row and a byte column ([`VimState::mark`]): `None` if
    /// it isn't set or its row was deleted.
    pub fn mark(&self, name: char) -> Option<(u32, usize)> {
        self.state.mark(name)?.get(self.model.snapshot())
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

    /// The state, for what a host takes from it ([`VimState::take_host_command`]).
    pub fn state_mut(&mut self) -> &mut VimState {
        &mut self.state
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

    /// `clipboard=unnamedplus` for the registers ([`Registers::set_clipboard_unnamed`]).
    pub fn set_clipboard_unnamed(&mut self, on: bool) {
        self.registers.set_clipboard_unnamed(on);
    }

    /// The host, whose `clipboard` is `+` and `*`.
    pub fn host(&self) -> &HeadlessHost {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut HeadlessHost {
        &mut self.host
    }
}
