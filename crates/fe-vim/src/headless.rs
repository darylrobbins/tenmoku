//! A vim editor without a UI, for tests and the Neovim oracle.

use std::collections::VecDeque;

use fe_buffer::{Buffer, Point, TransactionId};
use fe_command::{DispatchError, Dispatched, Invocation, Registry, Source};
use fe_editor::{
    EditorModel,
    commands::{EditorHost, HeadlessHost, ScrollRequest, Viewport},
};

use crate::{
    CancelArgs, HistoryRecall, KeyError, Mode, Registers, Splits, Step, SubmitArgs, VimCommands,
    VimKey, VimState, VimTarget, context, jump::JumpOverlay, parse_keys, registry, scroll,
};

/// An [`EditorModel`] with a [`HeadlessHost`], a [`VimState`], registers
/// and vim's registry, fed keys the way the GPUI host routes them: through
/// [`crate::step`] in normal mode, in insert mode as the editor's text
/// input and the keymap would take them, and in `cmdline` mode to a line of
/// its own, as the host's command line.
///
/// It keeps a view of the text too, as a window of nvim's would (by
/// default 22 rows from the first; [`HeadlessVim::set_window_rows`],
/// [`HeadlessVim::top_row`]): its commands see it through
/// [`EditorHost::viewport`] and change it through [`EditorHost::scroll`],
/// and before and after each key it scrolls to keep the cursor in view as
/// nvim's window does with 'scrolloff' 0.
pub struct HeadlessVim {
    model: EditorModel,
    host: HeadlessHost,
    view: Viewport,
    state: VimState,
    registers: Registers,
    registry: Registry<VimCommands>,
    /// The command line's text, in `cmdline` mode.
    line: String,
    /// Where `<Up>` and `<Down>` are in the line's history.
    recall: HistoryRecall,
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
            view: Viewport {
                top_row: 0,
                rows: 22,
            },
            state: VimState::new(),
            registers: Registers::new(),
            registry: registry(),
            line: String::new(),
            recall: HistoryRecall::default(),
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
    /// [`EditorModel::backspace`], `<C-t>` and `<C-d>` to
    /// [`VimState::indent_in`] and [`VimState::indent_out`], `<C-o>` to
    /// [`VimState::one_command`], `<C-n>` and `<C-p>` to
    /// [`VimState::complete_next`] and [`VimState::complete_previous`],
    /// `<C-e>` and `<C-y>` while completing to [`VimState::complete_cancel`]
    /// and [`VimState::complete_accept`], and `<Esc>` dispatches `vim.normal_mode`;
    /// any other key goes to [`VimState::insert_key`] (`<C-w>`, `<C-u>`),
    /// and is forwarded if it doesn't take it. In replace mode the same, but a character
    /// goes to [`VimState::replace_text`], `<BS>` to
    /// [`VimState::replace_backspace`] and `<Esc>` dispatches
    /// `vim.replace_normal_mode`. Nor in `cmdline` mode: a character goes
    /// on the line, `<BS>` deletes its last (or on an empty line dispatches
    /// `vim.command_line_cancel`, as `<Esc>` does), `<CR>` dispatches
    /// `vim.command_line_submit` with the line, and `<Up>` and `<Down>`
    /// replace it with the next older or newer entry of its history that
    /// starts with what was typed ([`HistoryRecall`]); any other key is
    /// forwarded. In both, `<C-j>` (`<NL>`, a register's line break) is `<CR>`.
    /// Either way the key goes to [`VimState::record_key`] first. A map's
    /// `<Cmd>…<CR>` goes to [`VimState::feed`] in every mode
    /// ([`VimState::remap_command_next`]).
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
        self.follow_cursor();
        let fed = self.feed_mode_key(key, replayed);
        self.follow_cursor();
        fed
    }

    fn feed_mode_key(&mut self, key: VimKey, replayed: bool) -> Result<(), FeedError> {
        // A map's `<Cmd>…<CR>` goes to the reducer in every mode.
        let mode = match self.state.remap_command_next() {
            true => Mode::Normal,
            false => self.state.mode(),
        };
        match mode {
            Mode::Insert => {
                self.state.record_key(key);
                return self.type_key(key);
            }
            Mode::Replace => {
                self.state.record_key(key);
                return self.replace_key(key);
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
            VimKey::Ctrl('t') => self.state.indent_in(&mut self.model),
            VimKey::Ctrl('d') => self.state.indent_out(&mut self.model),
            VimKey::Ctrl('o') => self.state.one_command(&mut self.model),
            VimKey::Ctrl('n') => self.state.complete_next(&mut self.model),
            VimKey::Ctrl('p') => self.state.complete_previous(&mut self.model),
            VimKey::Ctrl('e') if self.state.complete_cancel(&mut self.model) => {}
            VimKey::Ctrl('y') if self.state.complete_accept(&mut self.model) => {}
            VimKey::Esc => {
                let invocation = Invocation::of("vim.normal_mode")
                    .expect("vim command IDs are valid")
                    .source(Source::Keyboard)
                    .context(context);
                self.dispatch(&invocation)?;
            }
            _ if self.state.insert_key(&mut self.model, &self.host, key) => {}
            _ => return Err(FeedError::Forwarded(key)),
        }
        Ok(())
    }

    /// One key in replace mode, as [`HeadlessVim::type_key`] takes one in
    /// insert mode, but through [`VimState::replace_text`] and
    /// [`VimState::replace_backspace`], with `<Esc>` dispatching
    /// `vim.replace_normal_mode`; [`VimState::insert_key`] takes the same keys.
    fn replace_key(&mut self, key: VimKey) -> Result<(), FeedError> {
        let context = context(&self.state);
        match key {
            VimKey::Char(ch) => self
                .state
                .replace_text(&mut self.model, ch.encode_utf8(&mut [0; 4])),
            VimKey::Tab => self.state.replace_text(&mut self.model, "\t"),
            VimKey::Enter | VimKey::Ctrl('j') => {
                self.state.line_break(&mut self.model);
                self.state.restore_indent(&mut self.model);
            }
            VimKey::Backspace => self.state.replace_backspace(&mut self.model),
            VimKey::Ctrl('t') => self.state.indent_in(&mut self.model),
            VimKey::Ctrl('d') => self.state.indent_out(&mut self.model),
            VimKey::Ctrl('o') => self.state.one_command(&mut self.model),
            VimKey::Esc => {
                let invocation = Invocation::of("vim.replace_normal_mode")
                    .expect("vim command IDs are valid")
                    .source(Source::Keyboard)
                    .context(context);
                self.dispatch(&invocation)?;
            }
            _ if self.state.insert_key(&mut self.model, &self.host, key) => {}
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
                self.recall.edited();
                return Ok(());
            }
            VimKey::Backspace if self.line.pop().is_some() => {
                self.recall.edited();
                return Ok(());
            }
            VimKey::Up | VimKey::Down => {
                let Mode::CmdLine(kind) = self.state.mode() else {
                    unreachable!("the line is open in cmdline mode")
                };
                let older = key == VimKey::Up;
                if let Some(text) = self.recall.step(&self.registers, kind, &self.line, older) {
                    self.line = text;
                }
                return Ok(());
            }
            VimKey::Ctrl('c') | VimKey::Esc if !replayed || key == VimKey::Ctrl('c') => {
                Invocation::of("vim.command_line_cancel").map(|invocation| {
                    let text = Some(std::mem::take(&mut self.line));
                    let args =
                        serde_json::to_value(CancelArgs { text }).expect("vim args serialize");
                    invocation.args(args)
                })
            }
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
    /// [`VimState::command_line_prefill`], at the end of its history; one
    /// that ends it empties it.
    pub fn dispatch(
        &mut self,
        invocation: &Invocation,
    ) -> Result<Dispatched<TransactionId>, DispatchError> {
        let was_open = matches!(self.state.mode(), Mode::CmdLine(_));
        let mut host = ViewHost {
            host: &mut self.host,
            view: &mut self.view,
        };
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut host,
            state: &mut self.state,
            registers: &mut self.registers,
        };
        let dispatched = crate::dispatch(&self.registry, &mut target, invocation);
        // The line starts from its prefill, whichever way the last one
        // ended.
        if !was_open || !matches!(self.state.mode(), Mode::CmdLine(_)) {
            self.line.clear();
            self.line.push_str(self.state.command_line_prefill());
            self.recall = HistoryRecall::default();
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

    /// 'ignorecase' and 'smartcase' for the registers ([`Registers::set_search_case`]).
    pub fn set_search_case(&mut self, ignore_case: bool, smart_case: bool) {
        self.registers.set_search_case(ignore_case, smart_case);
    }

    /// The host, whose `clipboard` is `+` and `*`.
    pub fn host(&self) -> &HeadlessHost {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut HeadlessHost {
        &mut self.host
    }

    /// Lets `s` and `S` start the jump ([`VimState::set_jump_on_s`]).
    pub fn set_jump_on_s(&mut self, on: bool) {
        self.state.set_jump_on_s(on);
    }

    /// Gives `gr` to ReplaceWithRegister
    /// ([`VimState::set_replace_with_register`]).
    pub fn set_replace_with_register(&mut self, on: bool) {
        self.state.set_replace_with_register(on);
    }

    /// The labels a host would paint for a pending jump over the view's
    /// rows ([`VimState::jump_overlay`]), those the jump's session takes
    /// ([`crate::jump::view_rows`]).
    pub fn jump_overlay(&self) -> Option<JumpOverlay> {
        let rows = crate::jump::view_rows(&self.model, self.view, None);
        self.state.jump_overlay(&self.model, rows)
    }

    /// The first row the view shows.
    pub fn top_row(&self) -> u32 {
        self.view.top_row
    }

    /// Gives the view `rows` rows (at least 1), scrolling to keep the
    /// cursor in it.
    pub fn set_window_rows(&mut self, rows: u32) {
        self.view.rows = rows.max(1);
        self.state.viewport_height_changed(self.view.rows);
        self.follow_cursor();
    }

    /// Scrolls the view to show the cursor, as nvim's window does with
    /// 'scrolloff' 0 (found by probing windows of 2 to 22 rows), and not
    /// past the text's last row. Counted in display rows, so a closed fold
    /// or a wrapped row in view takes the rows it shows; the top is always
    /// a text row's first.
    fn follow_cursor(&mut self) {
        let row = self.cursor().0;
        let screen = scroll::Screen::new(&self.model);
        let last = screen.count - 1;
        let Viewport { top_row, rows } = self.view;
        let top_row = top_row.min(self.model.text().row_count().saturating_sub(1));
        let top = screen.first(top_row);
        // The cursor's row's display rows.
        let (row, bottom) = (screen.first(row), screen.end(row));
        // Centring doesn't scroll further than showing the last row at the
        // bottom.
        let halfway = |above: u32| {
            row.saturating_sub(above)
                .min((last + 1).saturating_sub(rows))
        };
        let new_top = if row < top {
            // Close above it puts the cursor at the top, further centres it.
            match top - row >= (rows / 2).saturating_sub(1).max(2) {
                true => halfway((rows - 1) / 2),
                false => row,
            }
        } else if bottom >= top.saturating_add(rows) {
            // Below it: at the bottom if close, centred if further (a row
            // more above the cursor than below, to within a view's
            // height).
            let after = (last + 1).saturating_sub(top.saturating_add(rows));
            match bottom - (top.saturating_add(rows)) + 1 {
                // Where the rest of the text fits in one view's height, the
                // cursor goes to the bottom whatever the gap.
                gap if gap <= rows.div_ceil(2) || after <= rows => {
                    (bottom + 1).saturating_sub(rows)
                }
                gap if gap <= rows + 1 => halfway(rows / 2),
                _ => halfway((rows - 1) / 2),
            }
        } else {
            top
        };
        self.view.top_row = match new_top == top {
            true => top_row,
            false => screen.round_up(new_top, screen.at(row)),
        };
    }
}

/// The host [`HeadlessVim`] dispatches with: its [`HeadlessHost`], and its
/// view as the viewport.
struct ViewHost<'a> {
    host: &'a mut HeadlessHost,
    view: &'a mut Viewport,
}

impl EditorHost for ViewHost<'_> {
    fn page_rows(&self) -> u32 {
        self.host.page_rows()
    }

    fn read_clipboard(&mut self) -> Option<String> {
        self.host.read_clipboard()
    }

    fn write_clipboard(&mut self, text: String) {
        self.host.write_clipboard(text);
    }

    fn fold_range_at(&self, row: u32) -> Option<std::ops::RangeInclusive<u32>> {
        self.host.fold_range_at(row)
    }

    fn fold_ranges(&self, rows: std::ops::Range<u32>) -> Vec<std::ops::RangeInclusive<u32>> {
        self.host.fold_ranges(rows)
    }

    fn viewport(&self) -> Option<Viewport> {
        Some(*self.view)
    }

    fn view_columns(&self) -> Option<std::ops::Range<u32>> {
        Some(0..80)
    }

    fn scroll(&mut self, request: ScrollRequest) {
        match request {
            ScrollRequest::TopRow(row) => self.view.top_row = row,
        }
    }
}
