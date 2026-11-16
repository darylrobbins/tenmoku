//! Card D8's commands (docs/decisions/r1b9dw3g0g-debugger.md §§5-6), which
//! the debug toolbar's buttons and the F-keys dispatch too, so neither has
//! logic of its own:
//!
//! - **Continue, Pause, the steps and Restart** ask the session, which
//!   refuses any its control-state query has disabled (D2b): a refused one
//!   does nothing, as its button shows it disabled. With no session they
//!   say "No debug session is running".
//! - **Run to cursor** (§5) adds a hidden one-shot breakpoint at the caret's
//!   line on disk to its file's `setBreakpoints`, and continues once the
//!   adapter has answered it, and every send taking an earlier hidden
//!   breakpoint out, from the stop it was asked in only (checked as the
//!   resume goes, `Session::resume_at`). A send that fails or times out is
//!   said, the breakpoint goes and nothing runs. A hidden breakpoint taken
//!   out stays unconfirmed until a send of its file without it is
//!   answered. The first stop after the one it was asked in takes it
//!   out again, whatever that stop's reason, and so does the session's end.
//!   It needs no adapter support.
//! - **Start with arguments** asks for the extra arguments (split as a
//!   shell would, quotes kept together), then shows `debug.start`'s
//!   configuration picker: the configuration picked starts with them after
//!   its own, and its reruns add them again.
//! - **Show session** lists the window's session, the one there is in v1,
//!   with its state; picking it shows where it's stopped.
//! - **The breakpoint commands** act at the active editor's caret, through
//!   the root's [`BreakpointStore`], which sends the file's set again while
//!   a session runs.

use std::path::PathBuf;

use fe_buffer::BufferId;
use fe_dap::{Resume, SessionState};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div,
};
use kiln_l10n::{text, tr};
use kiln_shell::{ModalId, picker::joined_label};

use super::{Debugger, OneShot, SentThen, canonical};
use crate::{
    Workbench,
    picker::{Picker, PickerDelegate},
    workbench::toast::{self, Toast},
};

/// The arguments prompt's name in the modal layer.
pub(crate) const ARGS_PICKER: ModalId = ModalId("debug-args");
/// The session picker's.
pub(crate) const SESSION_PICKER: ModalId = ModalId("debug-session");

impl Workbench {
    /// The window's debugger, if its session is live: else says there's
    /// none.
    fn live_debugger(&self, cx: &mut Context<Self>) -> Option<Entity<Debugger>> {
        let debugger = self.debugger(cx)?;
        let live = (debugger.read(cx).session.as_ref()).is_some_and(|running| running.live());
        if !live {
            toast::show(cx, Toast::info(tr!("workbench-debug-not-running")));
            return None;
        }
        Some(debugger)
    }

    /// `debug.continue` and the steps: `resume` on the stopped thread.
    pub(crate) fn debug_resume(&mut self, resume: Resume, cx: &mut Context<Self>) {
        let Some(debugger) = self.live_debugger(cx) else {
            return;
        };
        let resumed = debugger.update(cx, |debugger, _| {
            (debugger.session.as_ref()).is_some_and(|running| running.session.resume(resume, None))
        });
        if resumed {
            cx.notify();
        }
    }

    /// `debug.pause`.
    pub(crate) fn debug_pause(&mut self, cx: &mut Context<Self>) {
        let Some(debugger) = self.live_debugger(cx) else {
            return;
        };
        debugger.update(cx, |debugger, _| {
            if let Some(running) = debugger.session.as_ref() {
                running.session.pause(None);
            }
        });
        cx.notify();
    }

    /// `debug.restart`: restarts the debuggee, while Restart is enabled.
    pub(crate) fn debug_restart(&mut self, cx: &mut Context<Self>) {
        let Some(debugger) = self.live_debugger(cx) else {
            return;
        };
        debugger.update(cx, |debugger, _| {
            if let Some(running) = debugger.session.as_ref()
                && running.session.controls().restart.is_enabled()
            {
                running.session.restart();
            }
        });
        cx.notify();
    }

    /// The active editor's file (canonical), buffer and caret's row, if it
    /// has a file.
    fn caret_in_file(&self, cx: &App) -> Option<(PathBuf, BufferId, u32)> {
        let editor = self.active_editor(cx)?.read(cx);
        let model = editor.model();
        let path = canonical(model.path()?);
        let row = (model.text())
            .offset_to_point(model.primary_selection().head())
            .row;
        Some((path, model.buffer().id(), row))
    }

    /// `debug.run_to_cursor`: continues to the caret's line through a
    /// hidden one-shot breakpoint, while Continue is enabled. It sends the
    /// breakpoint's file's set with it, and every file an earlier hidden
    /// breakpoint may still be in without one (`Running::unconfirmed`),
    /// and continues once all are answered, from the stop it was asked in
    /// only, if the breakpoint is still this run's.
    pub(crate) fn debug_run_to_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(debugger) = self.live_debugger(cx) else {
            return;
        };
        let Some((path, buffer, row)) = self.caret_in_file(cx) else {
            return toast::show(cx, Toast::info(tr!("workbench-debug-no-file")));
        };
        let Some(store) = self.breakpoint_store(cx) else {
            return;
        };
        let line = store.read(cx).lines_on_disk_in(&path, buffer, &[row])[0];
        let armed = debugger.update(cx, |debugger, _| {
            let running = debugger.session.as_mut().filter(|running| running.live())?;
            if !running.session.controls().continue_.is_enabled() {
                return None;
            }
            // An earlier run's breakpoint elsewhere goes, unconfirmed till
            // its file's next send is answered.
            if let Some(before) = running.one_shot.take()
                && before.path != path
            {
                running.unconfirmed.insert(before.path);
            }
            let cleanup: Vec<PathBuf> = (running.unconfirmed.iter())
                .filter(|unconfirmed| **unconfirmed != path)
                .cloned()
                .collect();
            let token = running.next_send;
            running.one_shot = Some(OneShot {
                path: path.clone(),
                line,
                epoch: running.session.epoch(),
                token,
                pending: cleanup.len() + 1,
                failed: false,
            });
            Some((running.id, token, cleanup))
        });
        let Some((id, token, cleanup)) = armed else {
            return;
        };
        for file in cleanup.iter().chain([&path]) {
            let then: SentThen = Box::new(move |workbench, answered, cx| {
                workbench.run_to_cursor_sent(id, token, answered, cx);
            });
            if !self.send_breakpoints_then(id, file, Some(then), cx) {
                self.run_to_cursor_sent(id, token, false, cx);
            }
        }
        cx.notify();
    }

    /// One of run to cursor `token`'s sends was answered, or failed. Once
    /// all are answered, and none failed, it continues from the stop it
    /// was asked in, if that's still the session's (checked as the resume
    /// goes); else its breakpoint goes.
    fn run_to_cursor_sent(&mut self, id: u64, token: u64, answered: bool, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let resumed = debugger.update(cx, |debugger, _| {
            let running = debugger.current(id).filter(|running| running.live())?;
            let shot = running
                .one_shot
                .as_mut()
                .filter(|shot| shot.token == token)?;
            shot.failed |= !answered;
            shot.pending = shot.pending.saturating_sub(1);
            if shot.pending > 0 {
                return None;
            }
            let (failed, epoch) = (shot.failed, shot.epoch);
            Some(!failed && running.session.resume_at(Resume::Continue, None, epoch))
        });
        if resumed == Some(false) {
            self.drop_one_shot(id, token, cx);
        }
        cx.notify();
    }

    /// Takes run to cursor `token`'s breakpoint out, if it's still set, and
    /// sends its file's set without it.
    fn drop_one_shot(&mut self, id: u64, token: u64, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let taken = debugger.update(cx, |debugger, _| {
            let running = debugger.current(id)?;
            running
                .one_shot
                .as_ref()
                .filter(|shot| shot.token == token)?;
            let path = running.one_shot.take()?.path;
            running.unconfirmed.insert(path.clone());
            Some(path)
        });
        if let Some(path) = taken {
            self.send_breakpoints(id, &path, cx);
        }
    }

    /// Session `id` stopped: run to cursor's breakpoint, if it was asked in
    /// an earlier stop, is taken out, and its file's set sent without it,
    /// unconfirmed till that's answered.
    pub(super) fn retire_one_shot(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let retired = debugger.update(cx, |debugger, _| {
            let running = debugger.current(id)?;
            let epoch = running.session.epoch();
            running
                .one_shot
                .as_ref()
                .filter(|shot| shot.epoch != epoch)?;
            let path = running.one_shot.take()?.path;
            running.unconfirmed.insert(path.clone());
            Some(path)
        });
        if let Some(path) = retired {
            self.send_breakpoints(id, &path, cx);
        }
    }

    /// `debug.toggle_breakpoint`: adds a breakpoint at the caret's row, or
    /// removes the one there.
    pub(crate) fn debug_toggle_breakpoint(&mut self, cx: &mut Context<Self>) {
        let Some((path, buffer, row)) = self.caret_in_file(cx) else {
            return toast::show(cx, Toast::info(tr!("workbench-debug-no-file")));
        };
        if let Some(store) = self.breakpoint_store(cx) {
            store.update(cx, |store, cx| store.toggle_in(&path, buffer, row, cx));
        }
    }

    /// `debug.toggle_breakpoint_enabled`: disables the breakpoint at the
    /// caret's row, or enables it.
    pub(crate) fn debug_toggle_breakpoint_enabled(&mut self, cx: &mut Context<Self>) {
        let Some((path, buffer, row)) = self.caret_in_file(cx) else {
            return toast::show(cx, Toast::info(tr!("workbench-debug-no-file")));
        };
        let Some(store) = self.breakpoint_store(cx) else {
            return;
        };
        let enabled = (store.read(cx).placements_in(&path, buffer).into_iter())
            .find(|placement| placement.row == row)
            .map(|placement| placement.enabled);
        match enabled {
            Some(enabled) => {
                store.update(cx, |store, cx| {
                    store.set_enabled_in(&path, buffer, row, !enabled, cx)
                });
            }
            None => toast::show(cx, Toast::info(tr!("workbench-debug-no-breakpoint-here"))),
        }
    }

    /// `debug.remove_all_breakpoints`: every breakpoint of the root's.
    pub(crate) fn debug_remove_all_breakpoints(&mut self, cx: &mut Context<Self>) {
        if let Some(store) = self.breakpoint_store(cx) {
            store.update(cx, |store, cx| store.remove_all(cx));
        }
    }

    /// `debug.start_with_args`: asks for the extra arguments, then shows
    /// the configuration picker.
    pub(crate) fn debug_start_with_args(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let modals = self.shell().read(cx).modals().clone();
        modals.update(cx, |layer, cx| layer.dismiss_modal(window, cx));
        let delegate = ArgsPrompt {
            workbench: cx.entity().downgrade(),
            previous_focus: window.focused(cx),
            args: Ok(Vec::new()),
            query: String::new(),
        };
        let placeholder = text!("workbench-debug-args-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        (self.shell().clone()).update(cx, |shell, cx| {
            shell.show_modal(ARGS_PICKER, picker, window, cx);
        });
    }

    /// The prompt's arguments were given: the picker of configurations to
    /// start with them.
    fn start_with(&mut self, args: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        debugger.update(cx, |debugger, _| debugger.pending_args = Some(args));
        let reading = self.read_launch_configs(cx);
        self.show_debug_picker(reading, window, cx);
    }

    /// `debug.show_session`: the picker of the window's sessions, the one
    /// there is.
    pub(crate) fn debug_show_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let row = {
            let debugger = debugger.read(cx);
            let name = (debugger.starting()).or_else(|| {
                debugger
                    .is_active()
                    .then(|| debugger.configuration())
                    .flatten()
            });
            name.map(|name| SessionRow {
                name: name.to_owned(),
                status: session_status(debugger),
            })
        };
        let Some(row) = row else {
            return toast::show(cx, Toast::info(tr!("workbench-debug-not-running")));
        };
        let modals = self.shell().read(cx).modals().clone();
        modals.update(cx, |layer, cx| layer.dismiss_modal(window, cx));
        let delegate = SessionPicker {
            workbench: cx.entity().downgrade(),
            previous_focus: window.focused(cx),
            row,
            shown: true,
        };
        let placeholder = text!("workbench-debug-session-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        (self.shell().clone()).update(cx, |shell, cx| {
            shell.show_modal(SESSION_PICKER, picker, window, cx);
        });
    }

    /// Opens the file the session is stopped in at its line, if it is.
    fn show_stopped_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(debugger) = self.debugger(cx) else {
            return;
        };
        let place = (debugger.read(cx).stop_location())
            .and_then(|stop| Some((stop.path.clone()?, stop.row + 1)));
        if let Some((path, line)) = place {
            (self.open_document_at(path, Some(line), None, window, cx)).detach();
        }
    }
}

#[cfg(test)]
impl Workbench {
    /// The modal showing, if it's one of this module's: its name and its
    /// rows' labels.
    pub(crate) fn debug_command_picker(&self, cx: &App) -> Option<(ModalId, Vec<SharedString>)> {
        let layer = self.shell().read(cx).modals().read(cx);
        let id = layer.modal()?;
        let view = layer.modal_view()?.clone();
        let rows = |count: usize, label: &dyn Fn(usize) -> SharedString| -> Vec<SharedString> {
            (0..count).map(label).collect()
        };
        if id == ARGS_PICKER {
            let picker: Entity<Picker<ArgsPrompt>> = view.downcast().ok()?;
            let delegate = &picker.read(cx).delegate;
            return Some((
                id,
                rows(delegate.match_count(), &|ix| delegate.match_label(ix, cx)),
            ));
        }
        if id == SESSION_PICKER {
            let picker: Entity<Picker<SessionPicker>> = view.downcast().ok()?;
            let delegate = &picker.read(cx).delegate;
            return Some((
                id,
                rows(delegate.match_count(), &|ix| delegate.match_label(ix, cx)),
            ));
        }
        None
    }

    /// Picks the first row of `workbench`'s modal showing, if it's one of
    /// this module's, as Enter would: from the picker, not the workbench.
    pub(crate) fn confirm_debug_command_picker(
        workbench: &Entity<Workbench>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layer = workbench.read(cx).shell().read(cx).modals().read(cx);
        let (Some(id), Some(view)) = (layer.modal(), layer.modal_view().cloned()) else {
            return;
        };
        if id == ARGS_PICKER
            && let Ok(picker) = view.clone().downcast::<Picker<ArgsPrompt>>()
        {
            picker.update(cx, |picker, cx| picker.confirm(0, window, cx));
        } else if id == SESSION_PICKER
            && let Ok(picker) = view.downcast::<Picker<SessionPicker>>()
        {
            picker.update(cx, |picker, cx| picker.confirm(0, window, cx));
        }
    }
}

/// A session's state, as its row in the session picker and its toolbar
/// label say it.
fn session_status(debugger: &Debugger) -> SharedString {
    match debugger.state() {
        _ if debugger.starting().is_some() => tr!("workbench-debug-run-starting"),
        Some(SessionState::Launching | SessionState::Initializing | SessionState::Configuring) => {
            tr!("workbench-debug-run-starting")
        }
        Some(SessionState::Stopped) => tr!("workbench-debug-run-stopped"),
        _ => tr!("workbench-debug-run-running"),
    }
}

/// Why typed arguments can't be split yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Incomplete {
    /// A quote isn't closed.
    Quote,
    /// A backslash outside quotes ends the text, escaping nothing.
    Escape,
}

/// `text` as a shell splits it into arguments: on unquoted whitespace,
/// `'…'` taken as it is, `"…"` with `\"` and `\\` escaped, and a
/// backslash outside quotes escaping the next character. Incomplete if a
/// quote isn't closed or a backslash ends it, as a shell would ask for
/// more.
pub(crate) fn split_args(text: &str) -> Result<Vec<String>, Incomplete> {
    let mut args = Vec::new();
    let mut arg: Option<String> = None;
    let mut chars = text.chars();
    let next_quoted = |chars: &mut std::str::Chars| chars.next().ok_or(Incomplete::Quote);
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => args.extend(arg.take()),
            '\'' => {
                let arg = arg.get_or_insert_with(String::new);
                loop {
                    match next_quoted(&mut chars)? {
                        '\'' => break,
                        c => arg.push(c),
                    }
                }
            }
            '"' => {
                let arg = arg.get_or_insert_with(String::new);
                loop {
                    match next_quoted(&mut chars)? {
                        '"' => break,
                        '\\' => match next_quoted(&mut chars)? {
                            c @ ('"' | '\\') => arg.push(c),
                            c => {
                                arg.push('\\');
                                arg.push(c);
                            }
                        },
                        c => arg.push(c),
                    }
                }
            }
            '\\' => {
                let escaped = chars.next().ok_or(Incomplete::Escape)?;
                arg.get_or_insert_with(String::new).push(escaped);
            }
            c => arg.get_or_insert_with(String::new).push(c),
        }
    }
    args.extend(arg);
    Ok(args)
}

/// `debug.start_with_args`' prompt: its one row starts with the arguments
/// typed, or says what they lack.
struct ArgsPrompt {
    workbench: WeakEntity<Workbench>,
    previous_focus: Option<FocusHandle>,
    /// The arguments typed, or what they lack.
    args: Result<Vec<String>, Incomplete>,
    query: String,
}

impl ArgsPrompt {
    fn label(&self) -> SharedString {
        match &self.args {
            Err(Incomplete::Quote) => tr!("workbench-debug-args-unclosed"),
            Err(Incomplete::Escape) => tr!("workbench-debug-args-trailing-escape"),
            Ok(args) if args.is_empty() => tr!("workbench-debug-args-none"),
            Ok(_) => tr!("workbench-debug-args-start", args = self.query.trim()),
        }
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

impl PickerDelegate for ArgsPrompt {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.args = split_args(&query);
        self.query = query;
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        1
    }

    fn match_id(&self, _: usize) -> SharedString {
        "debug-args".into()
    }

    fn render_match(&self, _: usize, _: bool, cx: &App) -> AnyElement {
        let line = div().truncate().child(self.label());
        match &self.args {
            Ok(_) => line,
            Err(_) => line.text_color(fe_ui::ui_theme(cx).muted_text),
        }
        .into_any_element()
    }

    fn match_label(&self, _: usize, _: &App) -> SharedString {
        self.label()
    }

    /// Takes the arguments, unless a quote is open, and shows the
    /// configurations to start with them.
    fn confirm(&mut self, _: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Ok(args) = self.args.clone() else {
            return false;
        };
        self.restore_focus(window, cx);
        if let Some(workbench) = self.workbench.upgrade() {
            // The configuration picker replaces this one in the modal
            // layer, so it shows once this one has gone.
            window.defer(cx, move |window, cx| {
                workbench.update(cx, |workbench, cx| workbench.start_with(args, window, cx));
            });
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }

    fn rows_fill_width(&self) -> bool {
        true
    }
}

/// The session picker's row.
struct SessionRow {
    name: String,
    status: SharedString,
}

/// `debug.show_session`'s picker: one row in v1, the window's session.
struct SessionPicker {
    workbench: WeakEntity<Workbench>,
    previous_focus: Option<FocusHandle>,
    row: SessionRow,
    /// Whether the query matches it.
    shown: bool,
}

impl SessionPicker {
    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }
}

impl PickerDelegate for SessionPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.shown = self.row.name.to_lowercase().contains(query.trim());
        cx.notify();
        let picker = cx.entity();
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        usize::from(self.shown)
    }

    fn match_id(&self, _: usize) -> SharedString {
        "debug-session-0".into()
    }

    fn render_match(&self, _: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .whitespace_nowrap()
            .child(div().flex_none().child(self.row.name.clone()))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(ui.muted_text)
                    .child(self.row.status.clone()),
            )
            .into_any_element()
    }

    fn match_label(&self, _: usize, _: &App) -> SharedString {
        joined_label([self.row.name.as_str(), self.row.status.as_ref()])
    }

    /// Shows where the session is stopped, if it is.
    fn confirm(&mut self, _: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        self.restore_focus(window, cx);
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| workbench.show_stopped_place(window, cx));
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.restore_focus(window, cx);
    }

    fn rows_fill_width(&self) -> bool {
        true
    }
}
