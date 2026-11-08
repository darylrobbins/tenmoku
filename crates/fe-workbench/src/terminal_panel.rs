//! The terminals on the bottom dock: shells in the project root, shown and
//! focused by `terminal.toggle`, as VS Code's ctrl-` does.
//!
//! The panel starts its first session on first show
//! ([`TerminalPanel::start`]) and keeps its sessions while the dock hides.
//! `terminal.new` starts another after the last, `terminal.close` drops the
//! active one, and `terminal.next` and `terminal.previous` cycle through
//! them; with two or more a list of them shows on its right. A
//! new session starts where the active one's shell last said it was (OSC 7;
//! docs/terminal-shell-integration.md), and `terminal.previous_prompt` and
//! `terminal.next_prompt` scroll the active one between the prompts its
//! shell marked (OSC 133); `terminal.vi_mode` turns the active one's vi copy
//! mode on or off. When
//! a shell exits the panel drops its session; when none is left it tells
//! the workbench ([`TerminalPanelEvent::Exited`]), which hides the dock,
//! and the next show starts a new one. Closing the window drops the
//! sessions, which hangs their shells up. `terminal.find` opens a find row
//! above the active one ([`terminal_find`]).
//!
//! A project task runs in a tab of its own ([`TerminalPanel::add_task`]),
//! named for it, always listed so the name shows. Unlike a shell's, its
//! session stays when it exits, with its output: its name gains a check mark
//! if it passed, and a toast says so if it didn't. Running its label again
//! replaces a finished tab's session in place, or adds `label (2)` beside
//! one that's still running. One that can't start leaves no tab.

use std::{
    io,
    path::{Path, PathBuf},
};

use fe_command::Source;
use fe_config::TerminalSettings;
use fe_gpui::EditorTheme;
use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView, TerminalViewEvent};
use fe_ui::{ActiveTheme as _, Button, Icon, IconButton, TAB_HEIGHT, Tooltip};
use gpui::{
    App, AppContext as _, ClickEvent, Context, ElementId, Entity, EntityId, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyContext, ParentElement as _,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, div, prelude::FluentBuilder as _, px, svg,
};
use kiln_shell::{DockItem, PanelId};

use crate::{
    find_bar::FindOp,
    pane_toolbar,
    picker::{Picker, PickerDelegate},
    task_defs::{Task, TaskCommand},
    workbench::toast::{self, Toast},
};
use terminal_find::TerminalFind;

#[path = "terminal_find.rs"]
mod terminal_find;

/// The session list's width.
const TAB_LIST_WIDTH: gpui::Pixels = px(160.);

/// The panel's id in the bottom dock.
pub(crate) const TERMINAL: PanelId = PanelId("terminal");

/// The key context the panel's element sets, around the sessions' own
/// (`terminal`) and the find row's.
pub(crate) const KEY_CONTEXT: &str = "terminal_panel";

/// The flag the panel's key context has while vim is on, which the window
/// keys leaving the panel need (`dock.focus_up` and the rest).
const VIM_FLAG: &str = "vim";

/// What the panel tells the workbench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalPanelEvent {
    /// The last session's shell exited, or it was closed, and the panel
    /// dropped it: none is left.
    Exited,
    /// The user double-clicked the list row of the session with this view
    /// id: the workbench asks for its new name.
    RenameRequested(EntityId),
    /// A finished task's toast asked for its tab, now active: the workbench
    /// shows the dock.
    ShowRequested,
}

/// One terminal in the panel.
struct Session {
    view: Entity<TerminalView>,
    /// Its tab's label until the shell sets a title: `Terminal <number>`.
    number: usize,
    /// The name the user gave it (`terminal.rename`), which its label shows
    /// in place of the shell's title until it closes.
    name: Option<SharedString>,
    /// The task it runs, if it's a task's tab.
    task: Option<TaskTab>,
    /// To its shell exiting.
    _exited: Subscription,
}

/// What makes a session a task's tab.
struct TaskTab {
    /// Unique among the tabs this panel ever had, and kept by a rerun: what a
    /// toast's Show finds it by.
    id: u64,
    /// The task's label, which a rerun finds its tab by.
    label: String,
    /// Which of the label's tabs this is: 1 goes by the label alone, the
    /// rest as `label (2)` and on.
    ordinal: usize,
    run: TaskRun,
}

impl TaskTab {
    /// The tab's name, as its label and its toasts have it.
    fn title(&self) -> String {
        match self.ordinal {
            1 => self.label.clone(),
            ordinal => format!("{} ({ordinal})", self.label),
        }
    }
}

/// Where a task's session has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskRun {
    Running,
    /// It exited with 0.
    Passed,
    /// It exited with another code, or was killed.
    Failed,
}

pub struct TerminalPanel {
    /// In start order, from the first show until the last shell exits.
    sessions: Vec<Session>,
    /// The session shown, an index into `sessions` while there are any.
    active: usize,
    /// The number the next session's tab gets, from 1 again once none is
    /// left.
    next_number: usize,
    /// The session list's scroll offset and its tabs' bounds.
    tabs: ScrollHandle,
    /// Whether the list scrolls the active tab into view as it next draws.
    reveal: bool,
    /// Where a session starts.
    cwd: PathBuf,
    style: TerminalStyle,
    /// The theme `style` was last made from.
    theme: EditorTheme,
    /// The user's `terminal` settings.
    settings: TerminalSettings,
    /// To [`fe_gpui::ActivePalette`], which a theme file's reload or the
    /// system's appearance changes without a settings reload: restyles the
    /// sessions and the ones to come.
    _palette: Subscription,
    /// The find row, made by the first `terminal.find`.
    find: Option<TerminalFind>,
    /// The panel's, tracked by its root, so it contains the find row and
    /// the sessions. Focused itself, as when the dock shows the panel, it
    /// hands focus to the active session as the panel draws, starting one
    /// if there's none.
    focus: FocusHandle,
    /// Whether a window key moved focus here from the sidebar, and focus
    /// has stayed in the panel since: `<C-w>` back out goes there.
    pub(crate) from_sidebar: bool,
    /// To focus leaving the panel, which forgets `from_sidebar`.
    focus_out: Option<Subscription>,
    /// The task last run here, for a rerun. A new root forgets it.
    last_task: Option<Task>,
    /// The id the last task tab got.
    last_task_id: u64,
    /// The shell a session runs instead of the user's.
    #[cfg(test)]
    shell: Option<(String, Vec<String>)>,
    /// Sessions started.
    #[cfg(test)]
    pub(crate) started: usize,
    /// What each session started was asked to run, in start order.
    #[cfg(test)]
    pub(crate) started_options: Vec<TerminalOptions>,
    /// Whether sessions stay out of the frame, so no process ever starts:
    /// a test emits their exits itself.
    #[cfg(test)]
    pub(crate) fake_sessions: bool,
    /// The tabs the last frame drew, none without a list: label and
    /// background.
    #[cfg(test)]
    drawn_tabs: Vec<(SharedString, gpui::Hsla)>,
}

impl EventEmitter<TerminalPanelEvent> for TerminalPanel {}

impl TerminalPanel {
    /// A panel whose sessions start in `cwd`, drawn in `theme`'s font and
    /// colours.
    pub(crate) fn new(cwd: PathBuf, theme: &EditorTheme, cx: &mut Context<Self>) -> Self {
        Self {
            sessions: Vec::new(),
            active: 0,
            next_number: 1,
            tabs: ScrollHandle::new(),
            reveal: false,
            cwd,
            style: style(
                theme,
                &fe_gpui::editor_palette(cx).terminal,
                &TerminalSettings::default(),
            ),
            theme: theme.clone(),
            settings: TerminalSettings::default(),
            _palette: cx.observe_global::<fe_gpui::ActivePalette>(Self::follow_palette),
            find: None,
            focus: cx.focus_handle(),
            from_sidebar: false,
            focus_out: None,
            last_task: None,
            last_task_id: 0,
            #[cfg(test)]
            shell: None,
            #[cfg(test)]
            started: 0,
            #[cfg(test)]
            started_options: Vec::new(),
            #[cfg(test)]
            fake_sessions: false,
            #[cfg(test)]
            drawn_tabs: Vec::new(),
        }
    }

    /// Where the next session starts: a new root, whose tasks these are, so
    /// the last one run isn't its to rerun.
    pub(crate) fn set_cwd(&mut self, cwd: PathBuf) {
        if self.cwd != cwd {
            self.last_task = None;
        }
        self.cwd = cwd;
    }

    /// The task last run here, unless the root has changed since.
    pub(crate) fn last_task(&self) -> Option<&Task> {
        self.last_task.as_ref()
    }

    /// Restyles the sessions, and the ones to come, from `theme`.
    pub(crate) fn set_theme(&mut self, theme: &EditorTheme, cx: &mut Context<Self>) {
        self.theme = theme.clone();
        self.restyle(cx);
    }

    /// Applies the `terminal` settings: a new font restyles the running
    /// sessions; a new shell, arguments or scrollback apply to the next one.
    pub(crate) fn set_settings(&mut self, settings: &TerminalSettings, cx: &mut Context<Self>) {
        self.settings = settings.clone();
        self.restyle(cx);
    }

    /// Recolours the theme the terminal's colours fall back to from the
    /// palette now active, and restyles with its `[terminal]` table.
    fn follow_palette(&mut self, cx: &mut Context<Self>) {
        self.theme.set_colors(&fe_gpui::editor_palette(cx));
        self.restyle(cx);
    }

    fn restyle(&mut self, cx: &mut Context<Self>) {
        self.style = style(
            &self.theme,
            &fe_gpui::editor_palette(cx).terminal,
            &self.settings,
        );
        for session in &self.sessions {
            let style = self.style.clone();
            session
                .view
                .update(cx, |view, cx| view.set_style(style, cx));
        }
    }

    /// The active session, if one is running.
    pub fn view(&self) -> Option<&Entity<TerminalView>> {
        self.sessions.get(self.active).map(|session| &session.view)
    }

    /// The sessions, in start order.
    pub fn views(&self) -> impl Iterator<Item = &Entity<TerminalView>> {
        self.sessions.iter().map(|session| &session.view)
    }

    /// The active session's index in [`TerminalPanel::views`].
    pub fn active(&self) -> usize {
        self.active
    }

    /// Whether the terminal, or its find row, has focus.
    pub(crate) fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle(cx).contains_focused(window, cx)
    }

    /// Starts a session unless one is running.
    pub(crate) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sessions.is_empty() {
            self.add(window, cx);
        }
    }

    /// Starts a session after the last and makes it active. Its shell
    /// starts once the view is laid out and its size known, in the
    /// directory the active session's shell last reported if it's a
    /// directory here, else in the panel's directory or, if that's gone,
    /// the home directory.
    pub(crate) fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reported = (self.view())
            .and_then(|view| view.read(cx).terminal()?.cwd())
            .filter(|dir| dir.is_dir());
        let options = TerminalOptions {
            shell: self.shell(),
            scrollback: self.settings.scrollback,
            ..TerminalOptions::new(reported.unwrap_or_else(|| start_dir(&self.cwd)))
        };
        self.add_session(options, window, cx);
    }

    /// Starts a session running `/bin/sh`, which writes `bytes` to the
    /// terminal and then waits on its input, after the last, and makes it
    /// active: a fixed screen for the UI audit's captures, prompts, commands
    /// and their output (OSC 133 marks them as a shell does), with no user
    /// shell or settings in it. The process runs until the session closes.
    #[doc(hidden)]
    pub fn open_with_session(&mut self, bytes: &str, window: &mut Window, cx: &mut Context<Self>) {
        let script = "printf '%s' \"$1\"; exec cat >/dev/null";
        let args = ["-c", script, "sh", bytes].map(str::to_owned).to_vec();
        let options = TerminalOptions {
            shell: Some(("/bin/sh".to_owned(), args)),
            scrollback: self.settings.scrollback,
            ..TerminalOptions::new(start_dir(&self.cwd))
        };
        self.add_session(options, window, cx);
    }

    /// Starts a shell's session running `options` after the last, and makes
    /// it active.
    fn add_session(
        &mut self,
        options: TerminalOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.new_view(options, false, window, cx);
        let exited = self.follow_exit(&view, window, cx);
        self.sessions.push(Session {
            view,
            number: self.next_number,
            name: None,
            task: None,
            _exited: exited,
        });
        self.next_number += 1;
        self.session_added(self.sessions.len() - 1, cx);
    }

    /// Runs `task` in a tab named for it and makes that active: in place of
    /// the session of a finished tab with its label, else in a new one, named
    /// `label (2)` and on while others with its label are open. Remembers it
    /// as the last task. Whether it's starting: if not, a toast has said why
    /// and no tab changed.
    ///
    /// It runs in its own directory or not at all, never in the home or root
    /// directory a shell falls back to.
    pub(crate) fn add_task(
        &mut self,
        task: &Task,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.last_task = Some(task.clone());
        let options = match task_options(task, self.settings.scrollback) {
            Ok(options) => options,
            Err(error) => {
                let toast = Toast::error(format!("Can't run {}", task.label));
                toast::show(cx, toast.detail(error.to_string()));
                return false;
            }
        };
        let finished = self.sessions.iter().position(|session| {
            (session.task.as_ref())
                .is_some_and(|tab| tab.label == task.label && tab.run != TaskRun::Running)
        });
        let kept = finished
            .and_then(|ix| self.sessions[ix].task.as_ref())
            .map(|tab| (tab.id, tab.ordinal));
        let (id, ordinal) = kept.unwrap_or_else(|| {
            self.last_task_id += 1;
            (self.last_task_id, self.free_ordinal(&task.label))
        });
        let view = self.new_view(options, true, window, cx);
        let exited = self.follow_exit(&view, window, cx);
        let tab = TaskTab {
            id,
            label: task.label.clone(),
            ordinal,
            run: TaskRun::Running,
        };
        let ix = match finished {
            // Dropping the old view hangs up what's left of its session; its
            // exit has been told already.
            Some(ix) => {
                let session = &mut self.sessions[ix];
                session.view = view;
                session.task = Some(tab);
                session._exited = exited;
                ix
            }
            // Tasks don't count towards `Terminal <n>`.
            None => {
                self.sessions.push(Session {
                    view,
                    number: 0,
                    name: None,
                    task: Some(tab),
                    _exited: exited,
                });
                self.sessions.len() - 1
            }
        };
        self.session_added(ix, cx);
        true
    }

    /// The lowest ordinal no open tab with `label` has.
    fn free_ordinal(&self, label: &str) -> usize {
        let taken = |ordinal: usize| {
            (self.sessions.iter())
                .filter_map(|session| session.task.as_ref())
                .any(|tab| tab.label == label && tab.ordinal == ordinal)
        };
        (1..).find(|&ordinal| !taken(ordinal)).unwrap_or(1)
    }

    /// A view that will run `options`. If its directory has gone since it was
    /// looked at, a shell starts in the root; a `task` mustn't, so there a
    /// failing command stands in for its own, which never runs.
    fn new_view(
        &mut self,
        options: TerminalOptions,
        task: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TerminalView> {
        #[cfg(test)]
        {
            self.started += 1;
            self.started_options.push(options.clone());
        }
        let style = self.style.clone();
        cx.new(|cx| {
            let fallback = (options.clone(), style.clone());
            TerminalView::new(options, style, window, cx).unwrap_or_else(|err| {
                // Removed since `start_dir` looked: the root always is one.
                log::warn!("starting the terminal in /: {err}");
                let (mut options, style) = fallback;
                options.cwd = PathBuf::from("/");
                if task {
                    options.shell = Some(sh_c("exit 127"));
                    options.env.clear();
                }
                TerminalView::new(options, style, window, cx).expect("/ is a directory")
            })
        })
    }

    /// Hears `view`'s session exit: a shell's tab goes, a task's stays.
    fn follow_exit(
        &self,
        view: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            view,
            window,
            |panel, view, TerminalViewEvent::Exited(code), window, cx| {
                let Some(ix) = panel.sessions.iter().position(|s| &s.view == view) else {
                    return;
                };
                if panel.sessions[ix].task.is_some() {
                    panel.task_exited(ix, *code, window, cx);
                } else {
                    panel.remove(ix, window, cx);
                }
            },
        )
    }

    /// Makes session `ix`, just started, active.
    fn session_added(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = ix;
        self.reveal = true;
        self.follow_find(cx);
        cx.notify();
    }

    /// Records how task session `ix` ended, and says so unless it passed. One
    /// whose process never started has nothing to show, so its tab goes.
    fn task_exited(
        &mut self,
        ix: usize,
        code: Option<i32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let started = self.session_started(ix, cx);
        let Some(tab) = self.sessions[ix].task.as_mut() else {
            return;
        };
        if tab.run != TaskRun::Running {
            return;
        }
        let title = tab.title();
        if !started {
            toast::show(cx, Toast::error(format!("Couldn't start {title}")));
            self.remove(ix, window, cx);
            return;
        }
        tab.run = if code == Some(0) {
            TaskRun::Passed
        } else {
            TaskRun::Failed
        };
        let id = tab.id;
        cx.notify();
        let message = match code {
            Some(0) => return,
            Some(code) => format!("{title} failed (exit {code})"),
            None => format!("{title} was killed"),
        };
        let (answer, answered) = futures::channel::oneshot::channel();
        let toast = Toast::error(message).choices(vec!["Show".to_owned()], move |choice| {
            answer.send(choice).ok();
        });
        toast::show(cx, toast);
        cx.spawn_in(window, async move |panel, cx| {
            if answered.await != Ok(Some(0)) {
                return;
            }
            panel
                .update_in(cx, |panel, window, cx| panel.show_task(id, window, cx))
                .ok();
        })
        .detach();
    }

    /// Whether session `ix`'s process started.
    fn session_started(&self, ix: usize, cx: &App) -> bool {
        #[cfg(test)]
        if self.fake_sessions {
            return true;
        }
        self.sessions[ix].view.read(cx).terminal().is_some()
    }

    /// Brings this window forward with task tab `id` active and focused, and
    /// asks for the dock: the toast asking may be in another window. Nothing
    /// if the tab has closed.
    fn show_task(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let ix = (self.sessions.iter())
            .position(|session| session.task.as_ref().is_some_and(|tab| tab.id == id));
        if let Some(ix) = ix {
            window.activate_window();
            self.activate(ix, window, cx);
            cx.emit(TerminalPanelEvent::ShowRequested);
        }
    }

    /// Drops the active session, as `terminal.close` does.
    pub(crate) fn close_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.sessions.is_empty() {
            self.remove(self.active, window, cx);
        }
    }

    /// Drops session `ix`, which hangs its shell up. If it was active its
    /// right neighbour, else its left, becomes active, focused if `ix` had
    /// focus. With none left the panel emits [`TerminalPanelEvent::Exited`].
    fn remove(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let focused = (self.sessions[ix].view)
            .focus_handle(cx)
            .contains_focused(window, cx);
        self.sessions.remove(ix);
        if self.sessions.is_empty() {
            self.active = 0;
            self.next_number = 1;
            self.update_find(|find, _, cx| find.run(FindOp::Close, None, window, cx), cx);
            cx.emit(TerminalPanelEvent::Exited);
            cx.notify();
            return;
        }
        if ix < self.active || self.active == self.sessions.len() {
            self.active -= 1;
        }
        self.reveal = true;
        self.follow_find(cx);
        if focused {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Where the session whose view is `id` is, if it's still running.
    fn position(&self, id: EntityId) -> Option<usize> {
        (self.sessions.iter()).position(|session| session.view.entity_id() == id)
    }

    /// Makes session `ix` active and focuses it.
    pub(crate) fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.sessions.len() {
            self.active = ix;
            self.reveal = true;
            self.follow_find(cx);
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    /// Scrolls the active session to its previous or next prompt
    /// (`forward`), as `terminal.previous_prompt` and `terminal.next_prompt`
    /// do: nothing in a session whose shell marked none.
    pub(crate) fn scroll_to_prompt(&self, forward: bool, cx: &mut App) {
        if let Some(view) = self.view() {
            view.update(cx, |view, cx| {
                if let Some(terminal) = view.terminal() {
                    terminal.scroll_to_prompt(forward);
                }
                cx.notify();
            });
        }
    }

    /// Turns vi copy mode on or off in the active session and focuses it,
    /// as `terminal.vi_mode` does.
    pub(crate) fn toggle_vi_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.view() {
            view.update(cx, |view, cx| view.toggle_vi_mode(cx));
        }
        self.focus_active(window, cx);
    }

    /// Activates the session `by` places on from the active one, wrapping,
    /// and focuses it; whether there was another to activate.
    pub(crate) fn cycle(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let len = self.sessions.len();
        if len < 2 {
            return false;
        }
        let ix = (self.active as isize + by).rem_euclid(len as isize) as usize;
        self.activate(ix, window, cx);
        true
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.view() {
            window.focus(&view.focus_handle(cx), cx);
        }
    }

    /// Does `op` in the find row, made by the first `terminal.find`
    /// ([`FindOp::Show`]): before that, nothing else does anything. Closing
    /// it focuses the active terminal.
    pub(crate) fn run_find(&mut self, op: FindOp, window: &mut Window, cx: &mut Context<Self>) {
        if op == FindOp::Show && self.find.is_none() {
            self.find = Some(TerminalFind::new(&fe_ui::ui_theme(cx), cx));
        }
        self.update_find(|find, view, cx| find.run(op, view, window, cx), cx);
        if op == FindOp::Close {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Changes the find row, if there is one, given the active terminal.
    pub(crate) fn update_find(
        &mut self,
        update: impl FnOnce(&mut TerminalFind, Option<&Entity<TerminalView>>, &mut App),
        cx: &mut App,
    ) {
        let view = self.view().cloned();
        if let Some(find) = &mut self.find {
            update(find, view.as_ref(), cx);
        }
    }

    /// Moves the open find row's query to the active terminal.
    fn follow_find(&mut self, cx: &mut App) {
        self.update_find(|find, view, cx| find.follow(view, cx), cx);
    }

    #[cfg(test)]
    pub(crate) fn find(&self) -> Option<&TerminalFind> {
        self.find.as_ref()
    }

    /// The active session's view id and label, if there is one.
    pub(crate) fn active_label(&self, cx: &App) -> Option<(EntityId, SharedString)> {
        let session = self.sessions.get(self.active)?;
        Some((session.view.entity_id(), self.base_label(self.active, cx)))
    }

    /// The label of the session with view id `id`, if it's still open, as a
    /// name to edit: without a task's check mark.
    pub(crate) fn label_of(&self, id: EntityId, cx: &App) -> Option<SharedString> {
        Some(self.base_label(self.position(id)?, cx))
    }

    /// Names session `id` `name`, trimmed; an empty name clears it, so the
    /// label is the shell's title again. Nothing if it has closed.
    pub(crate) fn set_name(&mut self, id: EntityId, name: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.position(id) {
            let name = name.trim();
            self.sessions[ix].name = (!name.is_empty()).then(|| name.to_owned().into());
            cx.notify();
        }
    }

    /// Focuses session `id`, if it's still open.
    pub(crate) fn focus_session(&self, id: EntityId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.position(id) {
            window.focus(&self.sessions[ix].view.focus_handle(cx), cx);
        }
    }

    /// Session `ix`'s tab label: its name, with a check mark if it's a task
    /// that passed.
    fn label(&self, ix: usize, cx: &App) -> SharedString {
        let label = self.base_label(ix, cx);
        let passed =
            (self.sessions[ix].task.as_ref()).is_some_and(|tab| tab.run == TaskRun::Passed);
        if passed {
            format!("{label} \u{2713}").into()
        } else {
            label
        }
    }

    /// Session `ix`'s name: the one the user gave it, else its task's, else
    /// the title its shell set, else `Terminal <n>`.
    fn base_label(&self, ix: usize, cx: &App) -> SharedString {
        let session = &self.sessions[ix];
        if let Some(name) = &session.name {
            return name.clone();
        }
        if let Some(tab) = &session.task {
            return tab.title().into();
        }
        match session.view.read(cx).title() {
            Some(title) => title.to_owned().into(),
            None => format!("Terminal {}", session.number).into(),
        }
    }

    /// One row per session in a list on the panel's right, the active one
    /// in the theme's selection colour: a terminal icon, the label and a
    /// kill button, shown on the active row and under the pointer. The rows
    /// scroll when they don't fit, the active one into view as it changes.
    /// A row and its handlers name its session by its view's id, so a click
    /// lands on the session that was drawn there or, if that's gone, on
    /// nothing.
    fn tab_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = fe_ui::ui_theme(cx);
        let colors = &cx.theme().colors;
        let (panel, border, hover) = (
            colors.surface.panel,
            colors.border.default,
            colors.list.row_hover,
        );
        let (inactive, selected) = (colors.tab.inactive, colors.list.row_selected);
        let (text, active_text, icon) = (
            colors.text.primary,
            colors.tab.active_text,
            colors.list.icon,
        );
        let rows: Vec<_> = (0..self.sessions.len())
            .map(|ix| {
                let id = self.sessions[ix].view.entity_id();
                let label = self.label(ix, cx);
                let active = ix == self.active;
                let (bg, color) = if active {
                    (selected, active_text)
                } else {
                    (inactive, text)
                };
                #[cfg(test)]
                self.drawn_tabs.push((label.clone(), bg));
                let group = SharedString::from(format!("terminal-tab-{id}"));
                let kill = IconButton::new(format!("terminal-tab-close-{id}"), Icon::X, &ui)
                    .name("Kill Terminal")
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        cx.stop_propagation();
                        if let Some(ix) = panel.position(id) {
                            panel.remove(ix, window, cx);
                        }
                    }));
                div()
                    .id(ElementId::NamedInteger("terminal-tab".into(), id.as_u64()))
                    .group(group.clone())
                    .flex()
                    .flex_none()
                    .w_full()
                    .h(TAB_HEIGHT)
                    .items_center()
                    .gap_1()
                    .pl_2()
                    .pr_1()
                    .bg(bg)
                    .text_color(color)
                    .when(!active, |row| row.hover(move |style| style.bg(hover)))
                    .child(
                        svg()
                            .path(Icon::FileTerminal.path())
                            .size(px(16.))
                            .flex_none()
                            .text_color(icon)
                            .debug_selector(|| format!("terminal-tab-icon-{id}")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_none()
                            .when(!active, |kill_box| {
                                kill_box
                                    .invisible()
                                    .group_hover(group, |style| style.visible())
                            })
                            .child(kill),
                    )
                    .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                        if event.click_count() >= 2 {
                            cx.emit(TerminalPanelEvent::RenameRequested(id));
                        } else if let Some(ix) = panel.position(id) {
                            panel.activate(ix, window, cx);
                        }
                    }))
            })
            .collect();
        if std::mem::take(&mut self.reveal) {
            self.tabs.scroll_to_item(self.active);
        }
        div()
            .id("terminal-tabs")
            .flex()
            .flex_col()
            .flex_none()
            .w(TAB_LIST_WIDTH)
            .h_full()
            .bg(panel)
            .border_l_1()
            .border_color(border)
            .overflow_y_scroll()
            .track_scroll(&self.tabs)
            .children(rows)
    }

    /// The actions row above the sessions and their list: a `+` running
    /// `terminal.new` and a kill button running `terminal.close`, at the
    /// right, with one session or several. Their tooltips carry the
    /// commands' keys.
    fn actions(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = fe_ui::ui_theme(cx);
        let colors = &cx.theme().colors;
        let (panel, border) = (colors.surface.panel, colors.border.default);
        let tip = move |title: &'static str, command: &'static str| {
            move |_: &mut Window, cx: &mut App| {
                let key = pane_toolbar::shortcut(command, cx);
                cx.new(|_| Tooltip::new(title, key, &ui)).into()
            }
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .gap_1()
            .px_1()
            .w_full()
            .h(TAB_HEIGHT)
            .bg(panel)
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .id("terminal-new")
                    .debug_selector(|| "terminal-new".to_owned())
                    .tooltip(tip("New Terminal", "terminal.new"))
                    .child(
                        Button::new("terminal-new-button", "+")
                            .ghost()
                            .small()
                            .on_click(|_, window, cx| {
                                pane_toolbar::dispatch("terminal.new", Source::Mouse, window, cx);
                            }),
                    ),
            )
            .child(
                IconButton::new("terminal-kill", Icon::X, &ui)
                    .name("Kill Terminal")
                    .tooltip(tip("Kill Terminal", "terminal.close"))
                    .on_click(|_, window, cx| {
                        pane_toolbar::dispatch("terminal.close", Source::Mouse, window, cx);
                    }),
            )
    }

    /// The task tabs behind the active one that haven't started, laid out
    /// beneath it at its size for the one frame that starts them. A session
    /// starts when it's laid out, and a task runs when it's asked to, not
    /// when its tab is next looked at.
    fn unstarted_tasks(&self, cx: &App) -> Vec<gpui::AnyElement> {
        #[cfg(test)]
        if self.fake_sessions {
            return Vec::new();
        }
        (self.sessions.iter().enumerate())
            .filter(|(ix, session)| {
                *ix != self.active
                    && session.task.is_some()
                    && session.view.read(cx).terminal().is_none()
            })
            .map(|(_, session)| {
                div()
                    .absolute()
                    .size_full()
                    .child(session.view.clone())
                    .into_any_element()
            })
            .collect()
    }

    /// The shell sessions run: `terminal.shell`, else the user's login
    /// shell. Tests may override it ([`TerminalPanel::set_shell`]).
    fn shell(&self) -> Option<(String, Vec<String>)> {
        #[cfg(test)]
        if let Some(shell) = &self.shell {
            return Some(shell.clone());
        }
        let settings = &self.settings;
        (!settings.shell.is_empty()).then(|| (settings.shell.clone(), settings.shell_args.clone()))
    }
}

/// What runs `task`, which only ever runs where it says: a shell line in
/// `/bin/sh`, not the user's login shell, so that it means the same for
/// everyone sharing the project, and a program directly.
///
/// # Errors
///
/// When its directory isn't one, or it names no program.
pub(crate) fn task_options(task: &Task, scrollback: usize) -> io::Result<TerminalOptions> {
    if !std::fs::metadata(&task.cwd)?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("{} is not a directory", task.cwd.display()),
        ));
    }
    let shell = match &task.command {
        TaskCommand::Shell(line) => sh_c(line),
        TaskCommand::Argv(argv) => {
            let Some((program, args)) = argv.split_first() else {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "no command"));
            };
            (program.clone(), args.to_vec())
        }
    };
    Ok(TerminalOptions {
        shell: Some(shell),
        scrollback,
        env: task.env.clone(),
        ..TerminalOptions::new(&task.cwd)
    })
}

/// `/bin/sh -c line`.
fn sh_c(line: &str) -> (String, Vec<String>) {
    let args = vec!["-c".to_owned(), line.to_owned()];
    ("/bin/sh".to_owned(), args)
}

/// `cwd` if it's a directory, else the home directory, else the root.
fn start_dir(cwd: &Path) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    [Some(cwd.to_owned()), home]
        .into_iter()
        .flatten()
        .find(|dir| dir.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// The terminal's style: the editor's colours and, unless `settings` set
/// them, its font and the design's small code size (12 px, at
/// [`TerminalStyle::LINE_HEIGHT`] so box drawing still meets: 15 px rows),
/// and ANSI colours from the theme's `[terminal]` table.
fn style(
    theme: &EditorTheme,
    terminal: &fe_config::TerminalColors,
    settings: &TerminalSettings,
) -> TerminalStyle {
    let or_editor = |color: Option<fe_config::Color>, editor: gpui::Hsla| {
        color.map_or(editor, |color| gpui::rgba(color.to_rgba_u32()).into())
    };
    TerminalStyle {
        font_family: settings.font_family.as_deref().map_or_else(
            || theme.font_family.clone(),
            |family| family.to_owned().into(),
        ),
        font_fallbacks: settings.font_fallbacks.iter().map(Into::into).collect(),
        font_size: settings
            .font_size
            .map_or(fe_ui::CODE_SMALL.size, |size| px(size as f32)),
        line_height: TerminalStyle::LINE_HEIGHT,
        background: or_editor(terminal.background, theme.background),
        foreground: or_editor(terminal.foreground, theme.foreground),
        cursor: or_editor(terminal.cursor, theme.cursor),
        selection: theme.selection,
        ansi: TerminalStyle::ansi_from_rgba(terminal.ansi.map(|color| color.to_rgba_u32())),
    }
}

impl Focusable for TerminalPanel {
    /// The panel's own, which contains the find row and the sessions:
    /// focused, it hands focus to the active session as the panel draws.
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DockItem for TerminalPanel {
    fn id(&self) -> PanelId {
        TERMINAL
    }

    /// The active task tab's label, else the title the active session's
    /// shell set, else "Terminal".
    fn title(&self, cx: &App) -> SharedString {
        if self
            .sessions
            .get(self.active)
            .is_some_and(|s| s.task.is_some())
        {
            return self.label(self.active, cx);
        }
        let title = self.view().and_then(|view| view.read(cx).title());
        title.map_or_else(|| "Terminal".into(), |title| title.to_owned().into())
    }

    fn icon(&self, _: &App) -> Icon {
        Icon::FileTerminal
    }

    fn show_command(&self) -> Option<&'static str> {
        Some("terminal.toggle")
    }

    /// Joins the rail once shown, so the bottom dock takes no room until
    /// then.
    fn always_on_rail(&self) -> bool {
        false
    }

    fn focus_on_show(&self) -> bool {
        true
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Focused itself, as when the dock shows it: the active session
        // takes focus. Shown with no session, as by a rail click after the
        // shell exited: start one.
        let focused = self.focus.is_focused(window);
        if self.focus_out.is_none() {
            self.focus_out = Some(cx.on_focus_out(&self.focus, window, |panel, _, _, _| {
                panel.from_sidebar = false;
            }));
        }
        self.start(window, cx);
        if focused {
            self.focus_active(window, cx);
        }
        // A query set before its session started is checked once it has.
        self.update_find(|find, view, cx| find.settle(view, cx), cx);
        #[cfg(test)]
        self.drawn_tabs.clear();
        // A task's tab is listed even alone, or nothing would name it.
        let listed = self.sessions.len() > 1 || self.sessions.iter().any(|s| s.task.is_some());
        let list = listed.then(|| self.tab_list(cx));
        let actions = self.actions(cx);
        let view = self.view().cloned();
        let session = view.clone().map(IntoElement::into_any_element);
        #[cfg(test)]
        let session = if self.fake_sessions {
            let held = |view: &Entity<TerminalView>| div().track_focus(&view.focus_handle(cx));
            view.as_ref().map(|view| held(view).into_any_element())
        } else {
            session
        };
        let unstarted = self.unstarted_tasks(cx);
        let ui = fe_ui::ui_theme(cx);
        let find = (self.find.as_ref()).and_then(|find| find.render(view.as_ref(), &ui, cx));
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        if fe_gpui::vim_enabled(cx) {
            context.add(VIM_FLAG);
        }
        div()
            .track_focus(&self.focus)
            .key_context(context)
            .size_full()
            .flex()
            .flex_col()
            .bg(self.style.background)
            .child(actions)
            .children(find)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .relative()
                            .children(unstarted)
                            .children(session),
                    )
                    .children(list),
            )
    }
}

#[cfg(test)]
impl TerminalPanel {
    /// Runs `shell` in place of the user's in the sessions started from
    /// now.
    pub(crate) fn set_shell(&mut self, shell: &str, args: &[&str]) {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self.shell = Some((shell.to_owned(), args));
    }

    /// The tabs the last frame drew, none without a list: label and
    /// background.
    pub(crate) fn drawn_tabs(&self) -> &[(SharedString, gpui::Hsla)] {
        &self.drawn_tabs
    }

    /// Where tab `ix` was drawn in the last frame, as the list scrolled.
    pub(crate) fn tab_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        let bounds = self.tabs.bounds_for_item(ix)?;
        Some(gpui::Bounds::new(
            bounds.origin + self.tabs.offset(),
            bounds.size,
        ))
    }

    /// Where the tabs, scrolled, show.
    pub(crate) fn strip_bounds(&self) -> gpui::Bounds<gpui::Pixels> {
        self.tabs.bounds()
    }
}

#[cfg(test)]
#[path = "terminal_settings_tests.rs"]
mod terminal_settings_tests;

#[cfg(test)]
#[path = "terminal_font_fallbacks_tests.rs"]
mod terminal_font_fallbacks_tests;

#[cfg(test)]
#[path = "terminal_panel_tests.rs"]
mod terminal_panel_tests;

/// `terminal.rename`'s picker: the name as typed, and one row that applies
/// it, or clears the custom name when it's empty.
pub(crate) struct RenameTerminal {
    terminal: gpui::WeakEntity<TerminalPanel>,
    /// The view id of the session it names.
    id: gpui::EntityId,
    /// The query, trimmed.
    name: String,
}

impl RenameTerminal {
    /// Names session `id` of `terminal`, whose label is `label` now.
    pub(crate) fn new(
        terminal: gpui::WeakEntity<TerminalPanel>,
        id: EntityId,
        label: &str,
    ) -> Self {
        Self {
            terminal,
            id,
            name: label.to_owned(),
        }
    }

    /// Holds `label`, trimmed, as the name typed.
    pub(crate) fn typed(&mut self, label: &str) {
        self.name = label.trim().to_owned();
    }

    fn label(&self) -> String {
        if self.name.is_empty() {
            "Use the shell's title".to_owned()
        } else {
            format!("Rename to {}", self.name)
        }
    }
}

impl PickerDelegate for RenameTerminal {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.name = query.trim().to_owned();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        1
    }

    fn match_id(&self, _: usize) -> gpui::SharedString {
        "rename".into()
    }

    fn render_match(&self, _: usize, _: bool, _: &App) -> gpui::AnyElement {
        use gpui::{IntoElement as _, ParentElement as _};
        gpui::div().child(self.label()).into_any_element()
    }

    fn confirm(&mut self, _: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        if let Some(terminal) = self.terminal.upgrade() {
            let (id, name) = (self.id, self.name.clone());
            terminal.update(cx, |terminal, cx| {
                terminal.set_name(id, &name, cx);
                terminal.focus_session(id, window, cx);
            });
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(terminal) = self.terminal.upgrade() {
            let id = self.id;
            terminal.update(cx, |terminal, cx| terminal.focus_session(id, window, cx));
        }
    }

    fn status(&self) -> Option<gpui::SharedString> {
        Some("Enter to rename, Escape to cancel".into())
    }
}

#[cfg(test)]
#[path = "terminal_rename_tests.rs"]
mod terminal_rename_tests;
#[cfg(test)]
#[path = "terminal_tabs_tests.rs"]
mod terminal_tabs_tests;

#[cfg(test)]
#[path = "terminal_tab_list_tests.rs"]
mod terminal_tab_list_tests;

#[cfg(test)]
#[path = "terminal_find_tests.rs"]
mod terminal_find_tests;

#[cfg(test)]
#[path = "terminal_shell_integration_tests.rs"]
mod terminal_shell_integration_tests;

#[cfg(test)]
#[path = "terminal_vi_mode_tests.rs"]
mod terminal_vi_mode_tests;

#[cfg(test)]
#[path = "terminal_actions_tests.rs"]
mod terminal_actions_tests;

#[cfg(test)]
#[path = "terminal_keys_tests.rs"]
mod terminal_keys_tests;
