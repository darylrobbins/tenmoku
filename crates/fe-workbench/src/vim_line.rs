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
//!
//! After each edit or recall of a `/` or `?` line, the editor shows the
//! match submitting it would land on ('incsearch',
//! [`EditorView::preview_search`]), scrolled into view; cancelling the line
//! puts the view back.
//!
//! A line cancelled other than by a key (focus leaving it, a click outside
//! it, a tab switch) tells the editor its text and `<C-c>`, which a replay
//! cancels with too.

use std::collections::VecDeque;

use fe_command::{Invocation, Value};
use fe_gpui::{EditorView, commands::RunCommand, keymap::CHORD_TIMEOUT, vim_registers};
use fe_ui::{ROW_HEIGHT, SettleEdit, Settled, TextInput, TextInputEvent, Typed};
use fe_vim::{CommandLineKind, Ex, HistoryRecall, HostCommand, Mode, VimKey, VimState};
use gpui::{
    AppContext as _, Context, Entity, EntityId, EventEmitter, Focusable as _, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Styled as _,
    Subscription, Task, WeakEntity, Window, div, px,
};

use super::StatusBar;
use crate::{
    picker::{self, PaletteOp},
    search_panel::{self, SearchOp},
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
    /// The name completion a `tab` began, while the line still holds what
    /// it put there.
    complete: Option<NameCycle>,
    _blur: Option<Subscription>,
    /// Ends the wait for the rest of a `c` map after [`CHORD_TIMEOUT`].
    remap_timer: Option<Task<()>>,
    /// Keys a map gave are still on their way into the input: its text
    /// isn't what the editor's echo of the line says yet.
    typing: bool,
    _changed: Option<Subscription>,
    /// The mode segment's background and label colours in the last render.
    #[cfg(test)]
    drawn_mode: Option<(gpui::Hsla, gpui::Hsla)>,
}

/// A cycle of command-name completions, as nvim's wildmenu steps through
/// them: the text as typed, then each completion, then the text again.
struct NameCycle {
    /// The text as typed, and where its command name starts.
    typed: String,
    start: usize,
    names: Vec<&'static str>,
    /// 0 is the text as typed, `n` the `n`th name.
    at: usize,
    /// What the line holds for `at`: any other text ends the cycle.
    shown: String,
}

impl NameCycle {
    /// The cycle for `line`, if it ends in a command name with completions.
    fn of(line: &str) -> Option<Self> {
        let (start, names) = Ex::completions(line)?;
        (!names.is_empty()).then(|| Self {
            typed: line.to_owned(),
            start,
            names,
            at: 0,
            shown: line.to_owned(),
        })
    }

    /// Steps forward or back, and returns the text the line then holds.
    fn step(&mut self, forward: bool) -> &str {
        let count = self.names.len() + 1;
        self.at = if forward {
            (self.at + 1) % count
        } else {
            (self.at + count - 1) % count
        };
        self.shown = match self.at {
            0 => self.typed.clone(),
            at => format!("{}{}", &self.typed[..self.start], self.names[at - 1]),
        };
        &self.shown
    }
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
                let input = cx.new(|cx| TextInput::new("", &fe_ui::ui_theme(cx), cx));
                let focus = input.focus_handle(cx);
                self.line._blur = Some(cx.on_blur(&focus, window, Self::line_blurred));
                // An edit makes what the line then holds the prefix the
                // next Up or Down recalls by.
                self.line._changed = Some(cx.subscribe(&input, |bar, _, event, cx| {
                    if *event == TextInputEvent::Changed {
                        bar.line.recall.edited();
                        bar.preview_line(cx);
                        bar.sync_line_edit(cx);
                    }
                }));
                let bar = cx.weak_entity();
                let settle = bar.clone();
                input.update(cx, |input, _| {
                    input.set_typing_filter(Some(
                        move |text: &str, window: &mut Window, cx: &mut gpui::App| {
                            let Some(ch) = text.chars().next() else {
                                return Typed::Pass;
                            };
                            let typed = bar.update(cx, |bar, cx| bar.line_typed(ch, window, cx));
                            typed.unwrap_or(Typed::Pass)
                        },
                    ));
                    input.set_before_platform_input(Some(
                        move |text: &str,
                              platform: &str,
                              window: &mut Window,
                              cx: &mut gpui::App| {
                            let held = settle
                                .update(cx, |bar, cx| bar.line_settle(text, platform, window, cx));
                            held.unwrap_or_default()
                        },
                    ));
                });
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
        self.line.typing = false;
        self.line.complete = None;
        self.line.remap_timer = None;
        editor.update(cx, |editor, _| editor.begin_command_line());
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

    /// Cancels the open line in its editor, focus staying where it is. A
    /// macro recording takes what was typed, then `<C-c>`, which cancels a
    /// replayed line as this did (a replay's `<Esc>` would run it).
    fn cancel_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.resolve_line_prefix(window, cx);
        if let Some((editor, _)) = self.line.open.take() {
            if let Some(editor) = editor.upgrade() {
                let typed = (self.line.input.as_ref())
                    .map(|input| input.read(cx).text().to_owned())
                    .unwrap_or_default();
                editor.update(cx, |editor, _| {
                    editor.record_command_line(&typed, VimKey::Ctrl('c'))
                });
                run_in(&editor, cancel(), window, cx);
            }
            cx.notify();
        }
    }

    /// A character typed on the line, asked of the editor's `c` maps. The
    /// leading chars they give go in with the answer; the rest, which edit
    /// the line or end it, once the input's update is over.
    fn line_typed(&mut self, ch: char, window: &mut Window, cx: &mut Context<Self>) -> Typed {
        let Some(editor) = (self.line.open.as_ref()).and_then(|(editor, _)| editor.upgrade())
        else {
            return Typed::Pass;
        };
        let Some(line) = editor.update(cx, |editor, _| editor.remap_line_key(ch)) else {
            return Typed::Pass;
        };
        self.arm_remap_timer(&editor, window, cx);
        let mut keys: VecDeque<VimKey> = line.typed.into_iter().map(|(key, _)| key).collect();
        let mut text = String::new();
        while let Some(VimKey::Char(ch)) = keys.front().copied() {
            text.push(ch);
            keys.pop_front();
        }
        if !keys.is_empty() || !line.rest.is_empty() {
            self.line.typing = true;
            let bar = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                bar.update(cx, |bar, cx| {
                    bar.type_line_keys(keys, line.rest, window, cx)
                })
                .ok();
            });
        }
        match text.is_empty() {
            true => Typed::Swallow,
            false => Typed::Insert(text),
        }
    }

    /// An edit from the platform is about to go into the line (several
    /// characters at once, a range replaced or deleted, or an IME
    /// composition starting; `platform` is its text), the line holding
    /// `text`: the keys held for a longer `c` map resolve first, and the
    /// input makes the edits they come to before the platform's, so they
    /// come before it. A key that ends the line takes the platform's edit
    /// over: the input drops it, and it goes in once the line's keys have
    /// run (to the editor, if the line ended).
    fn line_settle(
        &mut self,
        text: &str,
        platform: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Settled {
        self.line.remap_timer = None;
        let mut settled = Settled::default();
        let Some(editor) = (self.line.open.as_ref()).and_then(|(editor, _)| editor.upgrade())
        else {
            return settled;
        };
        let mut len = text.chars().count();
        while editor.read(cx).remap_line_waiting() {
            let line = editor.update(cx, |editor, _| editor.remap_line_timeout());
            let mut keys: VecDeque<VimKey> = line.typed.into_iter().map(|(key, _)| key).collect();
            let mut ends = !line.rest.is_empty();
            while !ends && let Some(&key) = keys.front() {
                match key {
                    VimKey::Char(ch) => {
                        settled.edits.push(SettleEdit::Insert(ch.to_string()));
                        len += 1;
                    }
                    VimKey::Backspace if len > 0 => {
                        settled.edits.push(SettleEdit::Backspace);
                        len -= 1;
                    }
                    VimKey::Backspace | VimKey::Enter | VimKey::Esc => ends = true,
                    VimKey::Ctrl('j' | 'c') => ends = true,
                    _ => {}
                }
                if !ends {
                    keys.pop_front();
                }
            }
            if ends {
                // The platform's text follows the keys, as typed after them.
                keys.extend(platform.chars().map(VimKey::Char));
                settled.swallow = true;
                self.line.typing = true;
                let bar = cx.weak_entity();
                window.defer(cx, move |window, cx| {
                    bar.update(cx, |bar, cx| {
                        bar.type_line_keys(keys, line.rest, window, cx)
                    })
                    .ok();
                });
                break;
            }
        }
        settled
    }

    /// Tells the editor of an edit to the line that the `c` maps never saw,
    /// so a macro records it in the order it was typed.
    fn sync_line_edit(&mut self, cx: &mut Context<Self>) {
        let (Some((editor, _)), Some(input)) = (&self.line.open, &self.line.input) else {
            return;
        };
        let (Some(editor), false) = (editor.upgrade(), self.line.typing) else {
            return;
        };
        let text = input.read(cx).text().to_owned();
        editor.update(cx, |editor, _| editor.sync_line_text(&text));
    }

    /// Waits [`CHORD_TIMEOUT`] for the next key if `editor` holds keys for
    /// a longer `c` map.
    fn arm_remap_timer(
        &mut self,
        editor: &Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.line.remap_timer = editor.read(cx).remap_line_waiting().then(|| {
            cx.spawn_in(window, async move |bar, cx| {
                cx.background_executor().timer(CHORD_TIMEOUT).await;
                bar.update_in(cx, |bar, window, cx| bar.resolve_line_prefix(window, cx))
                    .ok();
            })
        });
    }

    /// Ends the wait for the rest of a `c` map: what the held keys come to
    /// goes into the line, or ends it. Keys that resolving leaves held
    /// resolve in turn.
    fn resolve_line_prefix(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.line.remap_timer = None;
        loop {
            let Some(editor) = (self.line.open.as_ref()).and_then(|(editor, _)| editor.upgrade())
            else {
                return;
            };
            if !editor.read(cx).remap_line_waiting() {
                return;
            }
            let line = editor.update(cx, |editor, _| editor.remap_line_timeout());
            let keys = line.typed.into_iter().map(|(key, _)| key).collect();
            self.type_line_keys(keys, line.rest, window, cx);
        }
    }

    /// Types `keys`, which maps gave, into the line, as the line takes
    /// them: text goes in, `<BS>` takes a char out, and `<CR>`, `<Esc>`
    /// and `<C-c>` end it, the keys after it then the editor's with `rest`.
    /// Keys the line has no use for are dropped.
    fn type_line_keys(
        &mut self,
        mut keys: VecDeque<VimKey>,
        mut rest: Vec<VimKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = (self.line.open.as_ref()).and_then(|(editor, _)| editor.upgrade());
        self.line.typing = false;
        while let Some(key) = keys.pop_front() {
            let Some(input) = self.line.input.clone().filter(|_| self.line.open.is_some()) else {
                break;
            };
            match key {
                VimKey::Char(ch) => input.update(cx, |input, cx| {
                    input.insert(ch.encode_utf8(&mut [0; 4]), cx)
                }),
                VimKey::Backspace if input.read(cx).text().is_empty() => {
                    self.end_line(None, VimKey::Backspace, window, cx);
                }
                VimKey::Backspace => input.update(cx, TextInput::backspace),
                VimKey::Enter | VimKey::Ctrl('j') => {
                    let text = input.read(cx).text().to_owned();
                    self.end_line(Some(text), VimKey::Enter, window, cx);
                }
                VimKey::Esc => self.end_line(None, VimKey::Esc, window, cx),
                VimKey::Ctrl('c') => self.cancel_line(window, cx),
                _ => {}
            }
        }
        // The line ended on a key: whatever is left of the keys is the
        // editor's now. With the line open, it's nothing but a `<Cmd>`'s.
        rest.splice(0..0, keys);
        if let Some(editor) = editor.filter(|_| !rest.is_empty()) {
            editor.update(cx, |editor, cx| editor.replay_line_rest(rest, cx));
        }
    }

    /// Shows in its editor what the open `/` or `?` line would land on.
    fn preview_line(&self, cx: &mut Context<Self>) {
        let Some((editor, prefix)) = &self.line.open else {
            return;
        };
        let (Some(editor), Some(input)) = (editor.upgrade(), &self.line.input) else {
            return;
        };
        if *prefix == ':' {
            return;
        }
        let text = input.read(cx).text().to_owned();
        editor.update(cx, |editor, cx| editor.preview_search(&text, cx));
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
        // Keys held for a longer `c` map go in first, and may end the line
        // themselves; the text is read after them.
        self.resolve_line_prefix(window, cx);
        let text = match (&text, &self.line.input) {
            (Some(_), Some(input)) => Some(input.read(cx).text().to_owned()),
            _ => text,
        };
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
        if search_panel::op(run, cx) == Some(SearchOp::Submit) {
            let text = input.read(cx).text().to_owned();
            return self.end_line(Some(text), VimKey::Enter, window, cx);
        }
        if super::is_cancel_line(run, cx) {
            return self.end_line(None, VimKey::Esc, window, cx);
        }
        // An edit of its own breaks a held `c` map: what's held goes in first.
        if picker::op(run, cx).is_some() {
            self.resolve_line_prefix(window, cx);
            if self.line.open.is_none() {
                return;
            }
        }
        let op = picker::op(run, cx);
        if op == Some(PaletteOp::Backspace) && input.read(cx).text().is_empty() {
            return self.end_line(None, VimKey::Backspace, window, cx);
        }
        let Some(edit) = op.and_then(PaletteOp::edit) else {
            return cx.propagate();
        };
        input.update(cx, edit);
    }

    /// Up and Down (unmodified) replace the line's text with the next older
    /// or newer entry of its history, through fe-vim's
    /// [`fe_vim::Registers::recall`]; at either end the text stays, but for
    /// Down past the newest, which puts back what was typed.
    fn on_line_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let older = match keystroke.key.as_str() {
            "up" => true,
            "down" => false,
            "tab" => {
                self.resolve_line_prefix(window, cx);
                return self.on_line_tab(event, cx);
            }
            _ => return,
        };
        self.resolve_line_prefix(window, cx);
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
            // set_text raises no Changed: the editor is told of the recall
            // before it sees the keys typed next.
            self.sync_line_edit(cx);
            self.preview_line(cx);
        }
    }

    /// `tab` and `shift-tab` on a `:` line replace the command name being
    /// typed with the next or previous completion, cycling back to the text
    /// as typed. Text the cycle didn't put there starts a new one.
    fn on_line_tab(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        let shift = modifiers.shift;
        if modifiers.modified() && modifiers != gpui::Modifiers::shift() {
            return;
        }
        let Some(input) = self.line.input.clone() else {
            return;
        };
        let ex = self
            .line
            .open
            .as_ref()
            .and_then(|(editor, _)| editor.upgrade())
            .and_then(|editor| open_kind(editor.read(cx)))
            == Some(CommandLineKind::Ex);
        if !ex {
            return;
        }
        cx.stop_propagation();
        let line = input.read(cx).text().to_owned();
        if self
            .line
            .complete
            .as_ref()
            .is_none_or(|cycle| cycle.shown != line)
        {
            self.line.complete = NameCycle::of(&line);
        }
        let Some(cycle) = self.line.complete.as_mut() else {
            return;
        };
        let text = cycle.step(!shift).to_owned();
        input.update(cx, |input, cx| input.set_text(text, cx));
        self.sync_line_edit(cx);
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
        let ui = fe_ui::ui_theme(cx);
        let (background, foreground) =
            editor
                .upgrade()
                .map_or((ui.panel_background, ui.text), |editor| {
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
            .bg(ui.panel_background)
            .border_t_1()
            .border_color(ui.border)
            .text_color(ui.text)
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
#[path = "vim_line/remap_tests.rs"]
mod remap_tests;

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
