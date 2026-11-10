//! What a host such as a diff view sets on an editor beyond its buffer: a
//! read-only switch, blocks of rows between the text's rows, and the scroll
//! position in screen rows, with an event when it moves.
//!
//! Read-only refuses every edit: an editor or vim command whose
//! `mutates_buffer` is set, typing, an input method's text and a middle-click
//! paste. With vim on, a refused edit, and any key that would open an insert
//! or replace session, stays in normal mode with nvim's message for a buffer
//! that isn't 'modifiable' ([`e21`]). Motions, the visual modes, yanks,
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
//! scroll position counts. A reserve block paints nothing: the host draws
//! over it, where [`EditorView::reserved_blocks`] says each frame put it.

use std::ops::Range;

use fe_buffer::Version;
use fe_command::{Invocation, Value};
use fe_editor::decorations::Tint;
use fe_vim::{CommandLineKind, Ex, ExCommand, Mode, Step, VimKey, VimState};
use gpui::{Bounds, Context, EventEmitter, Pixels, SharedString, px};

use super::EditorView;
use crate::vim;

/// nvim's message for an edit to a buffer that isn't 'modifiable': `E21:`,
/// an id, before its text in the active locale.
pub(crate) fn e21() -> String {
    format!("E21: {}", kiln_l10n::tr!("gpui-vim-e21"))
}

/// [`e21`] in en-US, which the tests compare with.
#[cfg(test)]
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
    /// in the text colour: lines past `rows` aren't shown. `words` has, by
    /// line, byte ranges of it highlighted in `tint` over the band, as a
    /// diff's changed words; a line past its end has none.
    Lines {
        lines: Vec<SharedString>,
        tint: Tint,
        words: Vec<Vec<Range<usize>>>,
    },
    /// Blank rows the view paints nothing on, for the host to draw an
    /// element over: see [`EditorView::reserved_blocks`].
    Reserve,
}

/// Where a frame laid out a [`HostBlockKind::Reserve`] block, in window
/// coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReservedBlock {
    /// The host's.
    pub id: u64,
    /// Its rows, across the editor's width, gutter and all: above or below
    /// the view, in part or whole, when it's scrolled out.
    pub bounds: Bounds<Pixels>,
    /// The editor's text and gutter, which clip it.
    pub clip: Bounds<Pixels>,
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
    /// The caret jumped within the text, from `from` (a byte offset in it):
    /// a click on a sticky scroll row. A host keeping a navigation history
    /// records `from`, as before a go-to-definition.
    Jumped { from: usize },
}

impl EventEmitter<EditorViewEvent> for EditorView {}

/// What the user put into an [`EditorView`] themselves, typed or pasted,
/// for a host that acts on it, such as formatting on type or on paste.
/// Neither is told of a vim replay (a macro, `.`, a map's expansion), an
/// input method's composition, nor a command another actor (an agent, a
/// host) runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorInputEvent {
    /// `text` was typed from the keyboard and changed the buffer, to
    /// `version`: a key's text, as an input method hands it in with nothing
    /// composing (not a composition's commit), or `"\n"` for `editor.newline`
    /// from its key, Enter, with vim off or in insert mode.
    Typed { text: String, version: Version },
    /// `editor.paste`, or vim's put (`p`, `P`, a visual-mode put), changed
    /// the buffer, to `version`: `range` of it is the text put in, which
    /// takes up `rows` rows. For a put, that's the register's text at its
    /// first caret as vim landed it ([`fe_vim::PutLanded`]): not a line
    /// break it adds to start or end its rows, nor the one ending its last
    /// row, whose rows count a linewise register's empty last line. For a
    /// paste, the span of every edit it made, and the rows that touches.
    Pasted {
        range: Range<usize>,
        rows: u32,
        version: Version,
    },
}

impl EventEmitter<EditorInputEvent> for EditorView {}

/// The commands a paste is: [`EditorInputEvent::Pasted`].
const PASTES: [&str; 3] = ["editor.paste", "vim.put", "vim.visual_put"];

impl EditorView {
    /// Tells the host `text` was typed, if it changed the buffer since
    /// `before`.
    pub(crate) fn emit_typed(&mut self, text: &str, before: Version, cx: &mut Context<Self>) {
        let version = self.model.buffer().version();
        if version != before && !text.is_empty() {
            cx.emit(EditorInputEvent::Typed {
                text: text.to_owned(),
                version,
            });
        }
    }

    /// Tells the host `invocation` pasted, if it's a paste the user ran
    /// that changed the buffer since `before`, outside a vim replay. A vim
    /// put says what it put ([`VimState::put_landed`]); an `editor.paste`
    /// put its text from `at`, the primary selection's start, to the
    /// primary caret it leaves, so a mirror it edits too (a linked range, a
    /// snippet's) isn't in it.
    pub(crate) fn emit_pasted(
        &mut self,
        invocation: &Invocation,
        before: Version,
        at: Option<fe_buffer::Anchor>,
        cx: &mut Context<Self>,
    ) {
        if self.vim_replaying
            || invocation.actor != fe_command::Actor::User
            || !PASTES.contains(&invocation.command.as_str())
        {
            return;
        }
        let version = self.model.buffer().version();
        if version == before {
            return;
        }
        let landed = match at {
            Some(at) => self.pasted_from(&at),
            None => (self.vim.as_ref())
                .and_then(|state| state.put_landed(version))
                .map(|put| (put.range.clone(), put.rows)),
        };
        if let Some((range, rows)) = landed {
            cx.emit(EditorInputEvent::Pasted {
                range,
                rows,
                version,
            });
        }
    }

    /// Tells the host text was pasted from `at` to the primary caret, if
    /// that changed the buffer since `before`: for a paste that isn't a
    /// command, a middle click's.
    pub(crate) fn emit_pasted_at(
        &mut self,
        at: &fe_buffer::Anchor,
        before: Version,
        cx: &mut Context<Self>,
    ) {
        let version = self.model.buffer().version();
        if version == before {
            return;
        }
        if let Some((range, rows)) = self.pasted_from(at) {
            cx.emit(EditorInputEvent::Pasted {
                range,
                rows,
                version,
            });
        }
    }

    /// The text from `at` to the primary caret, and the rows it takes up: a
    /// line break ending it doesn't reach the next.
    fn pasted_from(&mut self, at: &fe_buffer::Anchor) -> Option<(Range<usize>, u32)> {
        self.model.sync_display();
        let start = self.model.snapshot().resolve(at);
        let end = self.model.primary_selection().head();
        let text = self.model.text().slice_to_string(start..end.max(start));
        if text.is_empty() {
            return None;
        }
        let rows = text.matches('\n').count() + 1 - usize::from(text.ends_with('\n'));
        Some((start..end, rows as u32))
    }

    /// Where the last frame drew the caret at byte `offset`, in window
    /// coordinates: `None` before a frame, or for an offset it didn't lay
    /// out. For a host placing something at the text, or a test pointing
    /// at it.
    pub fn caret_bounds_at(&mut self, offset: usize) -> Option<gpui::Bounds<Pixels>> {
        self.layout.as_mut()?.caret_bounds(offset)
    }

    /// Where a paste puts its text: an anchor before the primary
    /// selection's start, for [`EditorView::emit_pasted`].
    pub(crate) fn paste_anchor(&mut self) -> fe_buffer::Anchor {
        self.model.sync_display();
        let start = self.model.primary_selection().range.start;
        self.model.snapshot().anchor_before(start)
    }
}

#[cfg(test)]
#[path = "input_event_tests.rs"]
mod input_event_tests;

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

    /// Adds `flag` to the editor's key context, or takes it out: for a
    /// host's keys while it shows something in the editor, such as a
    /// widget over a reserve block.
    pub fn set_key_context_flag(&mut self, flag: &'static str, on: bool, cx: &mut Context<Self>) {
        let had = self.key_flags.contains(&flag);
        match (on, had) {
            (true, false) => self.key_flags.push(flag),
            (false, true) => self.key_flags.retain(|&had| had != flag),
            _ => return,
        }
        cx.notify();
    }

    /// The reserve blocks the frame being drawn places, in view or not
    /// (one above a row a fold hides isn't placed), set as the editor is
    /// prepainted: an element prepainted after it in the same frame finds
    /// them where they are now, scrolled with the text. Those of the last
    /// frame until then.
    pub fn reserved_blocks(&self) -> &[ReservedBlock] {
        &self.reserved
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
        self.band_spares_caret = false;
        self.clear_band = false;
        self.sticky_reveal = None;
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

    /// With vim on, leaves [`e21`] as vim's message; with vim off a refused
    /// edit does nothing.
    pub(crate) fn refuse_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = &mut self.vim {
            state.set_host_message(e21());
        }
        cx.notify();
    }

    /// Whether read-only refuses `invocation`, one of vim's: one that edits,
    /// or opens an insert or replace session. Of those that may edit or not,
    /// an operator refuses all but a yank or a fold, and a command line's submission,
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
            "vim.operate" => !spares_text(args),
            "vim.jump" => {
                args.get("operator").is_some_and(|op| !op.is_null()) && !spares_text(args)
            }
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
                operator.is_some_and(|op| !op.is_null()) && !spares_text(&invocation.args);
        }
    }

    /// Refuses `invocation`, one of vim's, in a read-only view, saying
    /// [`e21`]: a command line it would have submitted closes.
    pub(crate) fn refuse_vim(&mut self, invocation: &Invocation, cx: &mut Context<Self>) {
        if invocation.command.as_str() == "vim.command_line_submit" {
            let cancel = Invocation::of("vim.command_line_cancel").expect("a valid command ID");
            let context = vim::stamp(invocation.context.clone(), self.vim.as_ref());
            self.run_vim_once(&cancel.context(context), cx);
        }
        self.refuse_edit(cx);
    }
}

/// Whether an operator's args name one that changes no text: a yank, or a
/// fold (`zf`).
fn spares_text(args: &Value) -> bool {
    matches!(
        args.get("operator").and_then(Value::as_str),
        Some("yank" | "fold")
    )
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
        "vim.operate" => spares_text(args),
        "vim.jump" => args.get("operator").is_none_or(Value::is_null) || spares_text(args),
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
