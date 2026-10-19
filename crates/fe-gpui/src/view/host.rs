//! What a host such as a diff view sets on an editor beyond its buffer: a
//! read-only switch, blocks of rows between the text's rows, and the scroll
//! position in screen rows, with an event when it moves.
//!
//! Read-only refuses every edit: an editor or vim command whose
//! `mutates_buffer` is set, typing, an input method's text and a middle-click
//! paste. With vim on, a refused edit, and any key that would open an insert
//! or replace session, stays in normal mode with nvim's message for a buffer
//! that isn't 'modifiable' ([`E21`]). Motions, the visual modes, yanks,
//! searches and `:` lines that don't edit work as usual.
//!
//! Vim is checked where every vim command runs, however its keys came
//! (typed, replayed by the keymap or a macro, or dispatched as an action):
//! [`EditorView::run_vim_once`] refuses one that would edit
//! ([`EditorView::refuses_vim`]). A refused key is taken with nothing
//! pending, and a macro's replay stops at it. A command line's submission is
//! decided from what it would run, without running it: a search by the
//! operator it was opened with, which the view notes as every line opens
//! ([`EditorView::note_command_line`]); an ex line by its command
//! ([`ex_edits`]), and `:normal` by the commands its keys commit
//! ([`keys_edit`]).
//!
//! Blocks are laid out by fe-display's block layer and painted by
//! [`crate::element`]'s `host_blocks`: their rows are screen rows, which the
//! scroll position counts.

use fe_command::{Invocation, Value};
use fe_editor::decorations::Tint;
use fe_vim::{CommandLineKind, Ex, ExCommand, Mode, Step, VimKey, VimState};
use gpui::{Context, EventEmitter, SharedString, px};

use super::EditorView;
use crate::vim;

/// nvim's message for an edit to a buffer that isn't 'modifiable'.
pub(crate) const E21: &str = "E21: Cannot make changes, 'modifiable' is off";

/// Rows a host places between an editor's rows, such as a diff's filler
/// across from the other side's added lines.
#[derive(Clone, Debug, PartialEq)]
pub struct HostBlock {
    /// The buffer row it's above: its first display row. The row count
    /// places it after the last row. Above a row a fold hides, it isn't
    /// shown.
    pub above: u32,
    /// How many rows tall it is.
    pub rows: u32,
    pub kind: HostBlockKind,
    /// The host's, for its own bookkeeping.
    pub id: u64,
}

/// What a [`HostBlock`] shows.
#[derive(Clone, Debug, PartialEq)]
pub enum HostBlockKind {
    /// Blank rows with a faint diagonal hatch, as a diff's filler.
    Filler,
    /// Text rows on a band tinted `tint`, one line a row, tabs expanded and
    /// in the text colour: lines past `rows` aren't shown.
    Lines {
        lines: Vec<SharedString>,
        tint: Tint,
    },
}

/// What an [`EditorView`] tells its subscribers.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorViewEvent {
    /// The first row in view moved, to `top` in screen rows (block rows
    /// counted, as [`EditorView::scroll_top`]), other than by
    /// [`EditorView::set_scroll_top`]: so two views can follow each other
    /// without echo.
    Scrolled { top: f32 },
    /// The git change bar was clicked, on the change at `rows`: buffer rows
    /// of the text, empty for a deletion, at its row. The caret is at the
    /// change's first row.
    ChangeBarClicked { rows: std::ops::Range<u32> },
}

impl EventEmitter<EditorViewEvent> for EditorView {}

impl EditorView {
    /// Refuses or allows edits. Made read-only in vim's insert or replace
    /// mode, the session ends first, as `Esc` ends it, while edits are
    /// still allowed: a count's copies or a block's rows go in. Any
    /// composition and the completion menu go.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only == read_only {
            return;
        }
        if read_only {
            self.sync_vim(cx);
            self.cancel_composition(cx);
            self.close_completion(cx);
            if let Some(replace) = self.typing_mode() {
                let command = match replace {
                    true => vim::REPLACE_NORMAL_MODE,
                    false => vim::NORMAL_MODE,
                };
                let normal = Invocation::of(command).expect("a valid command ID");
                let context = vim::stamp(fe_editor::commands::context(), self.vim.as_ref());
                self.run_vim(&normal.context(context), cx);
            }
        }
        self.read_only = read_only;
        cx.notify();
    }

    /// Drops a pending vim command and leaves a visual mode, as a click
    /// does; insert and replace mode stay. For a host done with the
    /// selection, such as a find bar's replace that found nothing to select.
    pub fn leave_vim_visual_mode(&mut self, cx: &mut Context<Self>) {
        self.cancel_vim(cx);
        cx.notify();
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Places `blocks` between the rows from the next frame, in place of
    /// those set before.
    pub fn set_blocks(&mut self, blocks: Vec<HostBlock>, cx: &mut Context<Self>) {
        if self.blocks != blocks {
            self.blocks = blocks;
            cx.notify();
        }
    }

    pub fn blocks(&self) -> &[HostBlock] {
        &self.blocks
    }

    /// How far down the view is scrolled, in screen rows: display rows and
    /// the block rows above them. Fractional partway down a row.
    pub fn scroll_top(&self) -> f32 {
        f32::from(self.scroll.y) / self.line_height()
    }

    /// Scrolls so `top`, in screen rows, is first in view from the next
    /// frame, which clamps it, leaving the caret where it is. Neither this
    /// nor the frame's clamp emits [`EditorViewEvent::Scrolled`].
    pub fn set_scroll_top(&mut self, top: f32, cx: &mut Context<Self>) {
        let top = top.max(0.);
        self.scroll.y = px(top * self.line_height());
        self.autoscroll = false;
        // A vim scroll since the last frame is overtaken.
        self.scrolled_top = None;
        self.scrolled_display_top = None;
        self.top_set = Some(self.scroll.y);
        self.reported_top = top;
        self.set_hover(None, cx);
        self.hide_popover(cx);
        cx.notify();
    }

    /// The buffer row at the top of the view plus the fraction of it
    /// scrolled past: a soft-wrapped row counts its screen rows, and folds
    /// and blocks are skipped. `None` before the first frame.
    pub fn top_buffer_line(&self) -> Option<f32> {
        let frame = &self.layout.as_ref()?.frame;
        let display_top = frame.block_map.to_display_top(self.scroll_top());
        let first = display_top.floor().max(0.) as u32;
        let display = self.model.display();
        let buffer_row = display.row(first).buffer_row;
        let start = display.row_for_buffer_row(buffer_row);
        let rows = wrapped_rows(display, buffer_row, start);
        let within = (display_top - start as f32).max(0.);
        Some(buffer_row as f32 + (within / rows as f32).min(1.))
    }

    /// Scrolls so [`EditorView::top_buffer_line`] is `line` from the next
    /// frame, which clamps it, leaving the caret where it is. Like
    /// [`EditorView::set_scroll_top`] it emits no
    /// [`EditorViewEvent::Scrolled`]. Nothing before the first frame.
    pub fn scroll_to_buffer_line(&mut self, line: f32, cx: &mut Context<Self>) {
        let Some(layout) = &self.layout else { return };
        let display = self.model.display();
        // Within the last row too, so a soft-wrapped last row scrolls
        // through its screen rows: the frame clamps what can't be reached.
        let rows_in_text = self.model.text().row_count().max(1);
        let line = line.clamp(0., rows_in_text as f32);
        let buffer_row = (line.floor() as u32).min(rows_in_text - 1);
        let start = display.row_for_buffer_row(buffer_row);
        let rows = wrapped_rows(display, buffer_row, start);
        let within = (line - buffer_row as f32).min(1.);
        let display_top = start as f32 + within * rows as f32;
        let top = layout.frame.block_map.to_screen_top(display_top);
        self.set_scroll_top(top, cx);
    }

    /// Whether the frame about to be laid out starts where the host last
    /// put it ([`EditorView::set_scroll_top`]), with nothing scrolled since:
    /// no wheel, vim scroll or caret to reveal. Its clamp of that top is
    /// then the host's too.
    pub(crate) fn host_top_holds(&self) -> bool {
        self.top_set == Some(self.scroll.y) && !self.autoscroll
    }

    /// A frame settled the scroll position: emits
    /// [`EditorViewEvent::Scrolled`] if its top moved since the last one
    /// reported, unless `held`, as [`EditorView::host_top_holds`] said
    /// before the frame was laid out.
    pub(crate) fn report_scroll_top(&mut self, held: bool, cx: &mut Context<Self>) {
        let top = self.scroll_top();
        self.top_set = None;
        if top != self.reported_top {
            self.reported_top = top;
            if !held {
                cx.emit(EditorViewEvent::Scrolled { top });
            }
        }
    }

    /// A row's height in pixels, as the frame lays it out.
    fn line_height(&self) -> f32 {
        f32::from((self.theme.font_size * self.theme.line_height).round()).max(1.)
    }

    /// With vim on, leaves [`E21`] as vim's message; with vim off a refused
    /// edit does nothing.
    pub(crate) fn refuse_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = &mut self.vim {
            state.set_host_message(E21);
        }
        cx.notify();
    }

    /// Whether read-only refuses `invocation`, one of vim's: one that edits,
    /// or opens an insert or replace session. Of those that may edit or not,
    /// an operator refuses all but a yank, and a command line's submission,
    /// or a map's `<Cmd>` ex line (`vim.ex_line`), one that would edit
    /// ([`EditorView::line_edits`]).
    pub(crate) fn refuses_vim(&self, invocation: &Invocation) -> bool {
        if !self.read_only {
            return false;
        }
        let args = &invocation.args;
        match invocation.command.as_str() {
            // Ending a session, which read-only never opens.
            vim::NORMAL_MODE | vim::REPLACE_NORMAL_MODE => false,
            "vim.operate" => !yanks(args),
            "vim.jump" => args.get("operator").is_some_and(|op| !op.is_null()) && !yanks(args),
            // A map's `<Cmd>…<CR>` line, by what it runs, as a typed one.
            "vim.ex_line" => ex_edits(args.get("text").and_then(Value::as_str).unwrap_or_default()),
            "vim.command_line_submit" => match self.vim.as_ref().map(VimState::mode) {
                Some(Mode::CmdLine(CommandLineKind::Ex)) => {
                    ex_edits(args.get("text").and_then(Value::as_str).unwrap_or_default())
                }
                Some(Mode::CmdLine(_)) => self.line_operates,
                _ => false,
            },
            _ => vim::registry()
                .get(&invocation.command)
                .is_some_and(|meta| meta.mutates_buffer),
        }
    }

    /// Notes, as `invocation` runs, whether the command line it opens is a
    /// search an operator other than a yank waits on, as `d/` opens: its
    /// submission edits. Whatever the view's read-only state then, so a
    /// line opened before it's set is known too.
    pub(crate) fn note_command_line(&mut self, invocation: &Invocation) {
        if invocation.command.as_str() == "vim.command_line" {
            let operator = invocation.args.get("operator");
            self.line_operates =
                operator.is_some_and(|op| !op.is_null()) && !yanks(&invocation.args);
        }
    }

    /// Refuses `invocation`, one of vim's, in a read-only view, saying
    /// [`E21`]: a command line it would have submitted closes.
    pub(crate) fn refuse_vim(&mut self, invocation: &Invocation, cx: &mut Context<Self>) {
        if invocation.command.as_str() == "vim.command_line_submit" {
            let cancel = Invocation::of("vim.command_line_cancel").expect("a valid command ID");
            let context = vim::stamp(invocation.context.clone(), self.vim.as_ref());
            self.run_vim_once(&cancel.context(context), cx);
        }
        self.refuse_edit(cx);
    }
}

/// Whether an operator's args name a yank.
fn yanks(args: &Value) -> bool {
    args.get("operator").and_then(Value::as_str) == Some("yank")
}

/// Whether the ex line `text` can edit: if any of the commands `|` separates
/// does ([`command_edits`]).
fn ex_edits(text: &str) -> bool {
    Ex::commands(text).into_iter().any(command_edits)
}

/// Whether the ex command `text` can edit, by its command. One that doesn't
/// parse can't: it fails with its own error. `:g` edits if the command it
/// runs does, and `:normal` if its keys do ([`keys_edit`]).
fn command_edits(text: &str) -> bool {
    let Ok(ex) = Ex::parse(text) else {
        return false;
    };
    match ex.command {
        ExCommand::Goto | ExCommand::Yank { .. } | ExCommand::NoHighlight | ExCommand::Host(_) => {
            false
        }
        ExCommand::Global { command, .. } => ex_edits(&command),
        ExCommand::Normal { keys, .. } => keys_edit(&keys),
        ExCommand::Table(command) => command.edits(),
        ExCommand::Delete { .. }
        | ExCommand::Join { .. }
        | ExCommand::Move { .. }
        | ExCommand::Copy { .. }
        | ExCommand::Shift { .. }
        | ExCommand::Sort { .. }
        | ExCommand::Substitute(_) => true,
    }
}

/// Whether `:normal`'s `keys` can edit: read from normal mode by the
/// reducer alone, they commit anything but a motion, a scroll, a fold, a
/// mark or a yank ([`reads_only`]). Those leave the mode and the text as
/// they are, so the reducer reads each key after them as `:normal` would;
/// anything else, such as `u`, `"+p`, `v` or `/`, counts as editing,
/// without reading on.
fn keys_edit(keys: &[VimKey]) -> bool {
    let mut state = VimState::new();
    keys.iter().any(|&key| match state.feed(key) {
        Step::Commit(invocation) => !reads_only(&invocation),
        Step::Wait(_) | Step::Cancel | Step::Forward => false,
    })
}

/// Whether `invocation`, one of vim's, only moves, scrolls, folds, sets a
/// mark or yanks.
fn reads_only(invocation: &Invocation) -> bool {
    let args = &invocation.args;
    match invocation.command.as_str() {
        "vim.move"
        | "vim.scroll"
        | "vim.fold"
        | "vim.set_mark"
        | "vim.jump_older"
        | "vim.jump_newer"
        | vim::NORMAL_MODE => true,
        "vim.operate" => yanks(args),
        "vim.jump" => args.get("operator").is_none_or(Value::is_null) || yanks(args),
        _ => false,
    }
}

/// The display rows `buffer_row` takes, from its first, `start`: more than
/// one when it is soft-wrapped.
fn wrapped_rows(display: &fe_display::DisplayMap, buffer_row: u32, start: u32) -> u32 {
    let mut rows = 1;
    while start + rows < display.row_count() && display.row(start + rows).buffer_row == buffer_row {
        rows += 1;
    }
    rows
}

#[cfg(test)]
mod scroll_line_tests;
