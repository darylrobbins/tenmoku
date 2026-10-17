//! Vim's command line, in the status bar's place: while the bar's editor
//! has a line open (`vim.mode` is `cmdline`, after `/`, `?` or `:`), the
//! bar shows a [`TextInput`] after the line's prefix, holding fe-vim's
//! prefill (`'<,'>`, `.,.+2`), and focuses it. Its element sets
//! [`KEY_CONTEXT`] and its input `text_input`, and neither carries
//! `vim.mode`, so keys typed there only type.
//!
//! `text_input.submit` (Enter) runs `vim.command_line_submit` with the
//! line's text in the editor; [`CANCEL`] (Escape), a backspace on an empty
//! line, and focus leaving the line for anything else run
//! `vim.command_line_cancel`. A key ending the line focuses the editor
//! again, and first tells it the line's text and that key, which a macro
//! recording takes ([`EditorView::record_command_line`]). Up and Down
//! recall the line's history, as Vim's `<Up>` and `<Down>` do: the next
//! older or newer entry starting with what was typed replaces the text (the
//! caret staying at its end), and Down past the newest puts back what was
//! typed ([`HistoryRecall`]). The [`fe_vim::HostCommand`] a submit may
//! leave (`:w`) is for the host to run, not the line: the bar emits it as a
//! [`VimHostCommand`].

use fe_command::{Invocation, Value};
use fe_gpui::{EditorView, commands::RunCommand, vim_registers};
use fe_ui::{ROW_HEIGHT, TextInput, TextInputEvent};
use fe_vim::{CommandLineKind, HistoryRecall, HostCommand, Mode, VimKey, VimState};
use gpui::{
    AppContext as _, Context, Entity, EntityId, EventEmitter, Focusable as _, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Styled as _,
    Subscription, WeakEntity, Window, div, px,
};

use super::StatusBar;
use crate::{
    dock::{self, DockOp},
    picker::{self, PaletteOp},
};

/// The key context the line's element sets.
pub(crate) const KEY_CONTEXT: &str = "vim_line";

/// Cancels the line, as Vim's `Esc` does.
pub(super) const CANCEL: &str = "vim_line.cancel";

/// A file or window command (`:w`, `:q` …) submitted on the line, for the
/// workbench to run for `editor`, the line's.
pub(crate) struct VimHostCommand {
    pub(crate) editor: WeakEntity<EditorView>,
    pub(crate) command: HostCommand,
}

impl EventEmitter<VimHostCommand> for StatusBar {}

/// The line's state in the bar.
#[derive(Default)]
pub(super) struct VimLine {
    /// Made when a line first opens.
    input: Option<Entity<TextInput>>,
    /// The editor whose line is open, and the line's prefix.
    open: Option<(WeakEntity<EditorView>, char)>,
    /// Where Up and Down are in the open line's history.
    recall: HistoryRecall,
    _blur: Option<Subscription>,
    _changed: Option<Subscription>,
    /// The mode segment's background and label colours in the last render.
    #[cfg(test)]
    drawn_mode: Option<(gpui::Hsla, gpui::Hsla)>,
}

/// The kind of line `editor` has open, if any.
fn open_kind(editor: &EditorView) -> Option<CommandLineKind> {
    match editor.vim_state()?.mode() {
        Mode::CmdLine(kind) => Some(kind),
        _ => None,
    }
}

fn prefix(kind: CommandLineKind) -> char {
    match kind {
        CommandLineKind::SearchForward => '/',
        CommandLineKind::SearchBackward => '?',
        CommandLineKind::Ex => ':',
    }
}

impl StatusBar {
    /// Whether the line is open, which shows the bar even if it's hidden.
    pub(crate) fn line_open(&self) -> bool {
        self.line.open.is_some()
    }

    /// Cancels a line open for an editor other than `editor`. The bar
    /// calls it as the workbench follows another editor, while the frame
    /// last drawn still holds the one the line is for, which a tab switch
    /// then takes out: an action reaches only an editor drawn.
    pub(super) fn leave_line(
        &mut self,
        editor: Option<EntityId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .line
            .open
            .as_ref()
            .is_some_and(|(open, _)| Some(open.entity_id()) != editor)
        {
            self.cancel_line(window, cx);
        }
    }

    /// Opens the line when the bar's editor has opened one, and closes it
    /// when the editor's has closed some other way (vim turned off). A line
    /// open for another editor is cancelled there.
    pub(super) fn sync_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.as_ref().and_then(WeakEntity::upgrade);
        self.leave_line(editor.as_ref().map(Entity::entity_id), window, cx);
        let Some(editor) = editor else {
            return;
        };
        match (open_kind(editor.read(cx)), self.line.open.is_some()) {
            (Some(kind), false) => self.open_line(kind, &editor, window, cx),
            (None, true) => {
                let focused = self
                    .line
                    .input
                    .as_ref()
                    .is_some_and(|input| input.focus_handle(cx).is_focused(window));
                self.line.open = None;
                if focused {
                    window.focus(&editor.focus_handle(cx), cx);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    fn open_line(
        &mut self,
        kind: CommandLineKind,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = match &self.line.input {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| TextInput::new("", &self.ui, cx));
                let focus = input.focus_handle(cx);
                self.line._blur = Some(cx.on_blur(&focus, window, Self::line_blurred));
                // An edit makes what the line then holds the prefix the
                // next Up or Down recalls by.
                self.line._changed = Some(cx.subscribe(&input, |bar, _, event, _| {
                    if *event == TextInputEvent::Changed {
                        bar.line.recall.edited();
                    }
                }));
                self.line.input = Some(input.clone());
                input
            }
        };
        let prefill = editor
            .read(cx)
            .vim_state()
            .map_or("", VimState::command_line_prefill)
            .to_owned();
        input.update(cx, |input, cx| input.set_text(prefill, cx));
        self.line.open = Some((editor.downgrade(), prefix(kind)));
        self.line.recall = HistoryRecall::default();
        window.focus(&input.focus_handle(cx), cx);
        cx.notify();
    }

    /// Focus left the line for something else, by a key or a command: the
    /// line is cancelled, focus staying there. Not when the window goes
    /// inactive, which leaves focus in the line for its return.
    fn line_blurred(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_window_active() {
            self.cancel_line(window, cx);
        }
    }

    /// A click outside the line cancels it before anything under the
    /// pointer takes the click, so the editor handles a click in the mode
    /// the line came from.
    /// Focus goes back to the editor first, for whatever's clicked to take
    /// it if it does.
    fn clicked_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self
            .line
            .input
            .as_ref()
            .is_some_and(|input| input.focus_handle(cx).is_focused(window));
        let editor = self
            .line
            .open
            .as_ref()
            .and_then(|(editor, _)| editor.upgrade());
        if let Some(editor) = editor.filter(|_| focused) {
            window.focus(&editor.focus_handle(cx), cx);
        }
        self.cancel_line(window, cx);
        // The cancel's dispatch cleared the flag GPUI's events share: the
        // click goes on.
        cx.propagate();
    }

    /// Cancels the open line in its editor, focus staying where it is.
    fn cancel_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((editor, _)) = self.line.open.take() {
            if let Some(editor) = editor.upgrade() {
                run_in(&editor, cancel(), window, cx);
            }
            cx.notify();
        }
    }

    /// Ends the line by `key`: submits `text`, or cancels with `None`, in
    /// its editor, focused again.
    fn end_line(
        &mut self,
        text: Option<String>,
        key: VimKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Closed first, so neither the blur nor the editor's notifying
        // reads it as open.
        let Some((editor, _)) = self.line.open.take() else {
            return;
        };
        cx.notify();
        let Some(editor) = editor.upgrade() else {
            return;
        };
        window.focus(&editor.focus_handle(cx), cx);
        // For a macro recording: what was typed, then the key.
        let typed = text.clone().or_else(|| {
            let input = self.line.input.as_ref()?;
            Some(input.read(cx).text().to_owned())
        });
        editor.update(cx, |editor, _| {
            editor.record_command_line(typed.as_deref().unwrap_or_default(), key)
        });
        let submit = text.is_some();
        let invocation = match text {
            Some(text) => Invocation::of("vim.command_line_submit")
                .expect("a valid command ID")
                .args(Value::from_iter([("text", text)])),
            // Esc keeps the cancelled text in the history, as nvim does.
            None if key == VimKey::Esc => cancel().args(Value::from_iter([(
                "text",
                typed.clone().unwrap_or_default(),
            )])),
            None => cancel(),
        };
        run_in(&editor, invocation, window, cx);
        // The submit ran synchronously: a host command it left is the
        // workbench's.
        let command = submit
            .then(|| editor.update(cx, |editor, _| editor.take_vim_host_command()))
            .flatten();
        if let Some(command) = command {
            cx.emit(VimHostCommand {
                editor: editor.downgrade(),
                command,
            });
        }
        // The editor's dispatch set the flag GPUI's events share: the key
        // that ended the line goes no further.
        cx.stop_propagation();
    }

    /// The line's commands and its input's; others go on to the workbench.
    fn on_line_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.line.input.clone() else {
            return cx.propagate();
        };
        if dock::op(run, cx) == Some(DockOp::Submit) {
            let text = input.read(cx).text().to_owned();
            return self.end_line(Some(text), VimKey::Enter, window, cx);
        }
        if super::is_cancel_line(run, cx) {
            return self.end_line(None, VimKey::Esc, window, cx);
        }
        let edit: fn(&mut TextInput, &mut Context<TextInput>) = match picker::op(run, cx) {
            Some(PaletteOp::Backspace) if input.read(cx).text().is_empty() => {
                return self.end_line(None, VimKey::Backspace, window, cx);
            }
            Some(PaletteOp::Backspace) => TextInput::backspace,
            Some(PaletteOp::DeleteWordLeft) => TextInput::delete_word_left,
            Some(PaletteOp::Clear) => TextInput::clear,
            Some(PaletteOp::Paste) => TextInput::paste,
            _ => return cx.propagate(),
        };
        input.update(cx, edit);
    }

    /// Up and Down (unmodified) replace the line's text with the next older
    /// or newer entry of its history, through fe-vim's
    /// [`fe_vim::Registers::recall`]; at either end the text stays, but for
    /// Down past the newest, which puts back what was typed.
    fn on_line_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let older = match keystroke.key.as_str() {
            "up" => true,
            "down" => false,
            _ => return,
        };
        if keystroke.modifiers.modified() {
            return;
        }
        let Some(input) = self.line.input.clone() else {
            return;
        };
        let editor = self
            .line
            .open
            .as_ref()
            .and_then(|(editor, _)| editor.upgrade());
        let Some(kind) = editor.and_then(|editor| open_kind(editor.read(cx))) else {
            return;
        };
        cx.stop_propagation();
        let line = input.read(cx).text().to_owned();
        let Some(registers) = vim_registers(cx) else {
            return;
        };
        if let Some(text) = self.line.recall.step(registers, kind, &line, older) {
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
    }

    /// The line, while it's open: a `COMMAND` segment in the editor
    /// theme's `vim_mode_command`, as kiln-ui draws a filled status entry,
    /// then its prefix, then its input.
    pub(super) fn render_line(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        #[cfg(test)]
        {
            self.line.drawn_mode = None;
        }
        let (editor, prefix) = self.line.open.as_ref()?;
        let prefix = *prefix;
        let input = self.line.input.clone()?;
        let (background, foreground) =
            editor
                .upgrade()
                .map_or((self.ui.panel_background, self.ui.text), |editor| {
                    let theme = editor.read(cx).theme();
                    (theme.vim_mode_command, theme.vim_mode_foreground)
                });
        #[cfg(test)]
        {
            self.line.drawn_mode = Some((background, foreground));
        }
        let mode = div()
            .debug_selector(|| "vim-line-mode".into())
            .flex()
            .flex_none()
            .items_center()
            .h_full()
            .px_3()
            .whitespace_nowrap()
            .bg(background)
            .text_color(foreground)
            .font_weight(FontWeight::SEMIBOLD)
            .child("COMMAND");
        let line = div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_line_command))
            .on_key_down(cx.listener(Self::on_line_key))
            .on_mouse_down_out(cx.listener(|bar, _, window, cx| bar.clicked_out(window, cx)))
            .debug_selector(|| "vim-line".into())
            .flex()
            .flex_none()
            .items_center()
            .w_full()
            .h(ROW_HEIGHT)
            .overflow_hidden()
            .bg(self.ui.panel_background)
            .border_t_1()
            .border_color(self.ui.border)
            .text_color(self.ui.text)
            .child(mode)
            .child(
                div()
                    .debug_selector(|| "vim-line-prefix".into())
                    .flex_none()
                    .ml_2()
                    .child(prefix.to_string()),
            )
            // The input pads its text; the line's follows the prefix, as
            // Vim's does.
            .child(div().flex_1().min_w_0().ml(px(-12.)).child(input));
        Some(line)
    }
}

fn cancel() -> Invocation {
    Invocation::of("vim.command_line_cancel").expect("a valid command ID")
}

/// Runs `invocation` in `editor`, wherever focus is, in the editor's own
/// context, if its line is still open: vim's registry then takes it, so
/// it goes no further than the editor, whichever view is being updated.
fn run_in(
    editor: &Entity<EditorView>,
    invocation: Invocation,
    window: &mut Window,
    cx: &mut Context<StatusBar>,
) {
    if open_kind(editor.read(cx)).is_none() {
        return;
    }
    let focus = editor.focus_handle(cx);
    focus.dispatch_action(&RunCommand(invocation), window, cx);
}

#[cfg(test)]
#[path = "vim_line/history_tests.rs"]
mod history_tests;

#[cfg(test)]
#[path = "vim_line/mode_segment_tests.rs"]
mod mode_segment_tests;

#[cfg(test)]
impl StatusBar {
    /// The open line's prefix and text.
    pub(crate) fn line(&self, cx: &gpui::App) -> Option<(char, String)> {
        let (_, prefix) = self.line.open.as_ref()?;
        let text = self.line.input.as_ref()?.read(cx).text().to_owned();
        Some((*prefix, text))
    }

    /// The mode segment's background and label colours in the last
    /// render, if it drew the line.
    pub(crate) fn line_mode_colors(&self) -> Option<(gpui::Hsla, gpui::Hsla)> {
        self.line.drawn_mode
    }

    /// The line's input, once a line has opened.
    pub(crate) fn line_input(&self) -> Option<&Entity<TextInput>> {
        self.line.input.as_ref()
    }
}
