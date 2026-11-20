//! The terminals on the bottom dock: shells in the project root, shown and
//! focused by `terminal.toggle`, as VS Code's ctrl-` does.
//!
//! The panel starts its first session on first show
//! ([`TerminalPanel::start`]) and keeps its sessions while the dock hides.
//! `terminal.new` starts another after the last, `terminal.close` drops the
//! active one, and `terminal.next` and `terminal.previous` cycle through
//! them. From two, each is a tab in the panel's own strip
//! (`docs/design/design-system.md#terminal`), which marks one whose command
//! is running or failed ([`TabMark`]); one alone draws no strip, and the
//! dock's carries its `+` and kill. A
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
//! named for it. Unlike a shell's, its
//! session stays when it exits, with its output: its name gains a check mark
//! if it passed, and a toast says so if it didn't. Running its label again
//! replaces a finished tab's session in place, or adds `label (2)` beside
//! one that's still running. One that can't start leaves no tab.
//!
//! A debug session's program runs in a debugger-owned tab
//! ([`TerminalPanel::add_debuggee`], docs/decisions/r1b9dw3g0g-debugger.md
//! §5): a task tab's look, named for its configuration, kept after exit
//! with its output, its check mark or failure toast. It isn't a task: it is
//! never the last task, `tasks.rerun_last` never replays its argv (the
//! adapter's one-shot launcher), and its rerun ([`DebuggeeRerun`]) asks the
//! debugger for a fresh session of its configuration.

use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_command::{CommandId, Source, Value};
use fe_config::TerminalSettings;
use fe_editor::lane::LaneId;
use fe_gpui::EditorTheme;
use fe_terminal_view::{
    CommandStatus, TerminalOptions, TerminalStyle, TerminalView, TerminalViewEvent,
    profiles::{self, Profile, ProfileKind},
};
use fe_ui::{ActiveTheme as _, Icon, IconButton, IconRef, TAB_HEIGHT, Tab, TabBar, Tooltip};
use gpui::{
    App, AppContext as _, ClickEvent, Context, ElementId, Entity, EntityId, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyContext, ParentElement as _,
    Render, ScrollHandle, SharedString, Styled as _, Subscription, Window, div, px,
};
use kiln_l10n::{tr, verbatim};
use kiln_shell::{DockItem, PaneAction, PanelId};

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

/// The `+` caret's id among the strip's buttons: `icon-button:<id>`.
pub(crate) const PROFILES_BUTTON: &str = "terminal-profiles-button";

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
    /// The user double-clicked the tab of the session with this view id:
    /// the workbench asks for its new name.
    RenameRequested(EntityId),
    /// A finished task's toast asked for its tab, now active: the workbench
    /// shows the dock.
    ShowRequested,
    /// The profiles a terminal can start changed, as detection found more
    /// or the settings changed: an open picker of them lists them again.
    ProfilesChanged,
}

/// A debuggee tab's rerun: the debugger starts a fresh session of the
/// configuration `name` of `root` (`debug.rerun_last`'s path), never
/// replaying the tab's argv.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebuggeeRerun {
    pub name: String,
    pub root: PathBuf,
}

/// What a debug session's `runInTerminal` asks to run
/// ([`TerminalPanel::add_debuggee`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Debuggee {
    /// The configuration's name, the tab's.
    pub name: String,
    /// Run directly, never through a shell.
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    /// What it sets in the environment it inherits; a `None` removes the
    /// variable.
    pub env: BTreeMap<String, Option<String>>,
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
    /// Its shell's foreground command, as its view last told.
    status: CommandStatus,
    /// The lane it belongs to, if it's one's terminal: an agent's, or the
    /// terminal lane's own CLI agent. Its tab is silver-blue.
    lane: Option<LaneId>,
    /// Whether its view takes no typing: an agent's terminal, which the
    /// user only watches. A terminal lane's own session is writable.
    read_only: bool,
    /// The command a lane's tab names: `lane 2 · <command>`.
    command: Option<SharedString>,
    /// The profile it was started from, whose name its tab shows until its
    /// program sets a title.
    profile: Option<SharedString>,
    /// To its view's events: its shell's exit and its command's status.
    _events: Subscription,
}

/// The theme token of the agent's colour: silver-blue, only ever an agent's
/// signal, never the accent.
const AGENT_TOKEN: &str = "agent.base";

impl Session {
    /// What its tab shows before its label: a task's run, or its shell's
    /// command as OSC 133 last marked it.
    fn mark(&self) -> TabMark {
        let (running, failed) = match &self.task {
            Some(tab) => (tab.run == TaskRun::Running, tab.run == TaskRun::Failed),
            None => (self.status == CommandStatus::Running, self.status.failed()),
        };
        if running {
            TabMark::Running
        } else if failed {
            TabMark::Failed
        } else {
            TabMark::Idle
        }
    }
}

/// What a session's tab shows before its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabMark {
    /// The terminal icon.
    Idle,
    /// A spinner: its task, or its shell's foreground command (an OSC 133;C
    /// with no D after it), is still running.
    Running,
    /// The error mark, in the theme's error colour: its task, or its shell's
    /// last command (an OSC 133;D), exited with a code other than 0.
    Failed,
}

impl TabMark {
    /// The icon, in the agent's colour on a lane's tab: a failure keeps its
    /// own hue there too.
    fn icon(self, lane: bool) -> IconRef {
        match (self, lane) {
            (Self::Idle, false) => IconRef::Bundled(Icon::FileTerminal),
            (Self::Running, false) => IconRef::Bundled(Icon::LoaderCircle),
            (Self::Idle, true) => IconRef::Tinted(Icon::FileTerminal, AGENT_TOKEN),
            (Self::Running, true) => IconRef::Tinted(Icon::LoaderCircle, AGENT_TOKEN),
            (Self::Failed, _) => IconRef::Tinted(Icon::CircleX, "status.error"),
        }
    }
}

/// What makes a session a task's tab.
struct TaskTab {
    /// Unique among the tabs this panel ever had, and kept by a rerun: what a
    /// toast's Show finds it by.
    id: u64,
    /// The task's label, which a rerun finds its tab by.
    label: String,
    /// The root it was run in: with the label, which task it is. Where it
    /// runs is the task's own, and an edit to it is still this task.
    root: PathBuf,
    /// When it last started, among this panel's runs: the latest has the
    /// last word on how the task went.
    started: u64,
    /// Which of the label's tabs this is: 1 goes by the label alone, the
    /// rest as `label (2)` and on.
    ordinal: usize,
    run: TaskRun,
    /// A debug session's program ([`TerminalPanel::add_debuggee`]), its
    /// label the configuration's name: not a task to find, rerun or stop.
    debuggee: bool,
}

impl TaskTab {
    /// The tab's name, as its label and its toasts have it.
    fn title(&self) -> SharedString {
        match self.ordinal {
            1 => self.label.clone().into(),
            ordinal => tr!(
                "workbench-terminal-task-numbered",
                label = &self.label,
                ordinal = ordinal
            ),
        }
    }
}

/// Where a task's session has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskRun {
    Running,
    /// It exited with 0.
    Passed,
    /// It exited with another code, or was killed.
    Failed,
}

/// A session asked for while detection ran ([`TerminalPanel::waiting`]).
struct Waiting {
    dir: Option<PathBuf>,
    /// The profile asked for; `None` for the default one.
    name: Option<String>,
}

/// The `+` caret's open menu: a row per profile.
/// Its rows are the profiles there are as it draws, so profiles detection
/// finds while it's open show in it.
struct ProfileMenu {
    /// The profile, by id, whose row Up and Down moved to, or the pointer
    /// is over, which Enter chooses: none until either does.
    highlighted: Option<String>,
    /// The caret's bounds in the window, which it hangs under.
    anchor: gpui::Bounds<gpui::Pixels>,
    /// What had focus as it opened, which closing it gives focus back to.
    back: Option<FocusHandle>,
    /// Closes it when focus leaves the panel.
    _focus_out: Subscription,
}

/// The profile a user's `[[terminal.profiles]]` entry is: `user:<name>`.
fn user_profile(user: &fe_config::TerminalProfileSettings) -> Profile {
    let name = user.name.trim();
    Profile {
        cwd: user.cwd.as_ref().map(PathBuf::from),
        env: user.env.clone(),
        icon: user.icon.clone(),
        ..Profile::shell(
            format!("{}{name}", profiles::ids::USER_PREFIX),
            name,
            user.command.trim(),
            user.args.clone(),
        )
    }
}

/// What the menu, the picker and a session's tab call `profile`: Default and
/// Command Prompt in the active locale, the others by their names.
pub(crate) fn profile_label(profile: &Profile) -> SharedString {
    match (&profile.kind, profile.id.as_str()) {
        (ProfileKind::Login, _) => tr!("workbench-terminal-profile-default"),
        (_, profiles::ids::COMMAND_PROMPT) => tr!("workbench-terminal-profile-cmd"),
        _ => profile.name.clone().into(),
    }
}

/// `profile`'s icon: its own, if the icon set has one of that name, else a
/// terminal's.
fn profile_icon(profile: &Profile) -> Icon {
    (profile.icon.as_deref())
        .and_then(|name| Icon::ALL.iter().copied().find(|icon| icon.name() == name))
        .unwrap_or(Icon::SquareTerminal)
}

pub struct TerminalPanel {
    /// In start order, from the first show until the last shell exits.
    sessions: Vec<Session>,
    /// The session shown, an index into `sessions` while there are any.
    active: usize,
    /// The number the next session's tab gets, from 1 again once none is
    /// left.
    next_number: usize,
    /// The strip's scroll offset and its tabs' bounds.
    tabs: ScrollHandle,
    /// Whether the strip scrolls the active tab into view as it next draws.
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
    /// The task last run here, for a rerun. A new root forgets it.
    last_task: Option<Task>,
    /// The id the last task tab got.
    last_task_id: u64,
    /// The runs started: see [`TaskTab::started`].
    runs_started: u64,
    /// Itself, for the dock strip's actions ([`DockItem::actions`]).
    this: gpui::WeakEntity<Self>,
    /// The profiles detection ([`profiles::detected`]) has found on this
    /// machine so far: on Windows, the shells come before the WSL distros.
    detected: Vec<Profile>,
    /// Whether detection may find more.
    detecting: bool,
    /// Whether detection found more since the panel last drew, for
    /// [`Self::start_waiting`].
    landed: bool,
    /// The sessions asked for while detection runs whose profile it may
    /// yet find: each started as it lands ([`Self::start_waiting`]).
    waiting: Vec<Waiting>,
    /// The `+` caret's menu of profiles, while it's open.
    profile_menu: Option<ProfileMenu>,
    /// Where the panel's own strip last drew the caret, in the window.
    caret_bounds: Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
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
    /// Whether a fake debuggee tab's program never starts, as one whose tab
    /// isn't laid out: [`TerminalPanel::started_pid`] says so.
    #[cfg(test)]
    pub(crate) debuggees_unstarted: bool,
    /// Each session's tab as the last frame had it, drawn or not: label
    /// and background.
    #[cfg(test)]
    drawn_tabs: Vec<(SharedString, gpui::Hsla)>,
    /// Each session's tab as the last frame had it, drawn or not: the text
    /// colour a lane's tab was given and its icon.
    #[cfg(test)]
    drawn_tab_marks: Vec<(Option<gpui::Hsla>, IconRef)>,
    /// Whether the last frame drew the strip.
    #[cfg(test)]
    strip_drawn: bool,
}

impl EventEmitter<TerminalPanelEvent> for TerminalPanel {}
impl EventEmitter<DebuggeeRerun> for TerminalPanel {}

impl TerminalPanel {
    /// A panel whose sessions start in `cwd`, drawn in `theme`'s font and
    /// colours.
    pub(crate) fn new(cwd: PathBuf, theme: &EditorTheme, cx: &mut Context<Self>) -> Self {
        // Under the test platform nothing detects: the detecting thread
        // landing would wake the panel's task from off the test scheduler,
        // which fails the test as nondeterministic. Tests give the panel
        // what detection finds themselves ([`Self::detection_landed`]).
        // `cx.is_test()` covers other crates' tests too, where `cfg!(test)`
        // is off.
        if !cx.is_test() {
            Self::detect_profiles(cx);
        }
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
            last_task: None,
            last_task_id: 0,
            runs_started: 0,
            this: cx.weak_entity(),
            detected: Vec::new(),
            detecting: true,
            landed: false,
            waiting: Vec::new(),
            profile_menu: None,
            caret_bounds: Rc::default(),
            #[cfg(test)]
            shell: None,
            #[cfg(test)]
            started: 0,
            #[cfg(test)]
            started_options: Vec::new(),
            #[cfg(test)]
            fake_sessions: false,
            #[cfg(test)]
            debuggees_unstarted: false,
            #[cfg(test)]
            drawn_tabs: Vec::new(),
            #[cfg(test)]
            drawn_tab_marks: Vec::new(),
            #[cfg(test)]
            strip_drawn: false,
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
    /// New profiles show in the caret's menu, if it's open, and the picker.
    pub(crate) fn set_settings(&mut self, settings: &TerminalSettings, cx: &mut Context<Self>) {
        self.settings = settings.clone();
        self.restyle(cx);
        cx.emit(TerminalPanelEvent::ProfilesChanged);
        cx.notify();
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
        if self.sessions.is_empty() && self.waiting.is_empty() {
            self.add(window, cx);
        }
    }

    /// Starts a session after the last and makes it active. Its shell
    /// starts once the view is laid out and its size known, in the
    /// directory the active session's shell last reported if it's a
    /// directory here, else in the panel's directory or, if that's gone,
    /// the home directory.
    pub(crate) fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_in(None, window, cx);
    }

    /// Like [`Self::add`], but in `dir` if it's given and a directory.
    /// It runs the default profile (`terminal.default_profile`); with none
    /// set, `terminal.shell` or the login shell, as before profiles.
    pub(crate) fn add_in(
        &mut self,
        dir: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.start_profile_in(dir, None, window, cx);
    }

    /// Like [`Self::add`], but running the profile `name` picks (by its name
    /// or id, ignoring case): whether it's starting, or will once
    /// detection has finished. If no profile has that name, a toast says so
    /// and nothing starts.
    pub(crate) fn add_profile(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.start_profile_in(None, Some(name), window, cx)
    }

    /// Starts a session of profile `name`, else of the default profile, in
    /// `dir` if it's a directory, else where [`Self::add`] says. One named
    /// that neither the user's profiles nor what detection has found so far
    /// has waits, while detection runs, for what it finds next: on Windows
    /// a shell is found before `wsl.exe` runs, so only a WSL distro's (or an
    /// unknown name) waits for its answer, at most
    /// [`profiles::WSL_TIMEOUT`]. Nothing blocks meanwhile. An unknown name
    /// is a toast; for the default profile, the session still starts, as
    /// with no default profile set. False if nothing starts or waits.
    fn start_profile_in(
        &mut self,
        dir: Option<&Path>,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let asked = name.unwrap_or(&self.settings.default_profile).trim();
        let profile = if asked.is_empty() {
            None
        } else {
            match self.profile_named(asked) {
                Some(profile) => Some(profile),
                None if self.detecting => {
                    self.waiting.push(Waiting {
                        dir: dir.map(Path::to_owned),
                        name: name.map(str::to_owned),
                    });
                    cx.notify();
                    return true;
                }
                None => {
                    let message = tr!("workbench-terminal-profile-unknown", name = asked);
                    toast::show(cx, Toast::error(message));
                    if name.is_some() {
                        return false;
                    }
                    None
                }
            }
        };
        let dir = match dir.filter(|dir| dir.is_dir()) {
            Some(dir) => dir.to_owned(),
            None => (self.view())
                .and_then(|view| view.read(cx).terminal()?.cwd())
                .filter(|dir| dir.is_dir())
                .unwrap_or_else(|| start_dir(&self.cwd)),
        };
        let scrollback = self.settings.scrollback;
        let options = match &profile {
            Some(profile) => profile.options(dir, scrollback),
            None => TerminalOptions {
                shell: self.shell(),
                scrollback,
                ..TerminalOptions::new(dir)
            },
        };
        let label = profile.as_ref().map(profile_label);
        self.add_session(options, label, window, cx);
        true
    }

    /// Once detection has found more: starts the sessions that waited for
    /// it whose profile it found, and, once it's finished, says which
    /// weren't found (the default profile's then runs the shell). True if
    /// one asked for by name wasn't found and so the panel, with no
    /// session, has told the workbench to hide it: it starts none itself.
    fn start_waiting(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !std::mem::take(&mut self.landed) {
            return false;
        }
        let mut refused = false;
        for waiting in std::mem::take(&mut self.waiting) {
            let (dir, name) = (waiting.dir.as_deref(), waiting.name.as_deref());
            refused |= !self.start_profile_in(dir, name, window, cx);
        }
        if refused && self.sessions.is_empty() && self.waiting.is_empty() {
            cx.emit(TerminalPanelEvent::Exited);
            return true;
        }
        false
    }

    /// Starts detecting the machine's profiles, once per process, on a
    /// thread of its own; the panel has each stage as it lands. Never under
    /// the test platform: see [`Self::new`].
    fn detect_profiles(cx: &mut Context<Self>) {
        let (found, landed) = async_channel::unbounded();
        profiles::detected(move |detected| {
            found.try_send(detected.clone()).ok();
        });
        cx.spawn(async move |panel, cx| {
            while let Ok(found) = landed.recv().await {
                let done = found.done;
                if panel
                    .update(cx, |panel, cx| panel.detection_landed(found, cx))
                    .is_err()
                    || done
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// What detection has found so far: the sessions waiting for it start
    /// as the panel next draws, and the menu and picker of profiles list it.
    pub(crate) fn detection_landed(&mut self, found: profiles::Detected, cx: &mut Context<Self>) {
        self.detected = found.profiles;
        self.detecting = !found.done;
        self.landed = true;
        cx.emit(TerminalPanelEvent::ProfilesChanged);
        cx.notify();
    }

    /// Detection has finished, having found `found`.
    #[cfg(test)]
    pub(crate) fn set_detected(&mut self, found: Vec<Profile>, cx: &mut Context<Self>) {
        let done = profiles::Detected {
            profiles: found,
            done: true,
        };
        self.detection_landed(done, cx);
    }

    /// Every profile a terminal can start, in the menu's order: those found
    /// on this machine so far, each replaced by the user's of its name, then
    /// the user's others.
    pub(crate) fn profiles(&self) -> Vec<Profile> {
        let detected = self.detected.clone();
        (self.settings).profiles_over(detected, |profile| &profile.name, user_profile)
    }

    /// The profile `name` picks, by its name or id.
    fn profile_named(&self, name: &str) -> Option<Profile> {
        (self.profiles().into_iter()).find(|profile| profile.is_named(name))
    }

    /// Starts a session running `/bin/sh`, which writes `bytes` to the
    /// terminal and then waits on its input, after the last, and makes it
    /// active: a fixed screen for the UI audit's captures, prompts, commands
    /// and their output (OSC 133 marks them as a shell does), with no user
    /// shell or settings in it. The process runs until the session closes.
    #[doc(hidden)]
    pub fn open_with_session(&mut self, bytes: &str, window: &mut Window, cx: &mut Context<Self>) {
        let script = verbatim!("printf '%s' \"$1\"; exec cat >/dev/null");
        let args = ["-c", script, "sh", bytes].map(str::to_owned).to_vec();
        let options = TerminalOptions {
            shell: Some(("/bin/sh".to_owned(), args)),
            scrollback: self.settings.scrollback,
            ..TerminalOptions::new(start_dir(&self.cwd))
        };
        self.add_session(options, None, window, cx);
    }

    /// Starts a shell's session running `options` after the last, and makes
    /// it active; `profile` names its tab until its program sets a title.
    fn add_session(
        &mut self,
        options: TerminalOptions,
        profile: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.new_view(options, false, window, cx);
        let events = self.follow(&view, window, cx);
        self.sessions.push(Session {
            view,
            number: self.next_number,
            name: None,
            task: None,
            status: CommandStatus::Idle,
            lane: None,
            read_only: false,
            command: None,
            profile,
            _events: events,
        });
        self.next_number += 1;
        self.session_added(self.sessions.len() - 1, cx);
    }

    /// Starts a session of `lane` running `options`, after the last, and
    /// makes it active. Its tab is silver-blue; read-only, it's an agent's
    /// terminal, named `lane 2 · <command>`, and takes no typing. Writable,
    /// it's a terminal lane's own session.
    #[allow(dead_code)] // Card 19 (`agent.new_terminal`) opens them.
    pub(crate) fn add_lane_session(
        &mut self,
        lane: LaneId,
        read_only: bool,
        command: impl Into<SharedString>,
        options: TerminalOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = self.new_view(options, false, window, cx);
        view.update(cx, |view, cx| view.set_read_only(read_only, cx));
        let events = self.follow(&view, window, cx);
        self.sessions.push(Session {
            view,
            number: self.next_number,
            name: None,
            task: None,
            status: CommandStatus::Idle,
            lane: Some(lane),
            read_only,
            command: Some(command.into()),
            profile: None,
            _events: events,
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
                let toast =
                    Toast::error(tr!("workbench-terminal-task-cant-run", label = &task.label));
                toast::show(cx, toast.detail(error.to_string()));
                return false;
            }
        };
        let finished = self.sessions.iter().position(|session| {
            (session.task.as_ref()).is_some_and(|tab| {
                !tab.debuggee
                    && tab.label == task.label
                    && tab.root == self.cwd
                    && tab.run != TaskRun::Running
            })
        });
        let kept = finished
            .and_then(|ix| self.sessions[ix].task.as_ref())
            .map(|tab| (tab.id, tab.ordinal));
        let (id, ordinal) = kept.unwrap_or_else(|| {
            self.last_task_id += 1;
            (self.last_task_id, self.free_ordinal(&task.label))
        });
        let view = self.new_view(options, true, window, cx);
        let events = self.follow(&view, window, cx);
        self.runs_started += 1;
        let tab = TaskTab {
            id,
            label: task.label.clone(),
            root: self.cwd.clone(),
            started: self.runs_started,
            ordinal,
            run: TaskRun::Running,
            debuggee: false,
        };
        self.put_task_tab(finished, view, tab, events, cx);
        true
    }

    /// Runs a debug session's program, as its adapter's `runInTerminal`
    /// asks, in the tab of its configuration, which it makes active: in
    /// place of that tab's session if there is one (a running one is hung
    /// up), else in a new tab. It is not a task: the last task stays as it
    /// was, and nothing reruns its argv.
    ///
    /// # Errors
    ///
    /// When its directory isn't one or the argv is empty: no tab changed.
    pub(crate) fn add_debuggee(
        &mut self,
        debuggee: &Debuggee,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> io::Result<Entity<TerminalView>> {
        let options = debuggee_options(debuggee, self.settings.scrollback)?;
        let reused = self.sessions.iter().position(|session| {
            (session.task.as_ref()).is_some_and(|tab| {
                tab.debuggee && tab.label == debuggee.name && tab.root == self.cwd
            })
        });
        let kept = reused
            .and_then(|ix| self.sessions[ix].task.as_ref())
            .map(|tab| (tab.id, tab.ordinal));
        let (id, ordinal) = kept.unwrap_or_else(|| {
            self.last_task_id += 1;
            (self.last_task_id, self.free_ordinal(&debuggee.name))
        });
        let view = self.new_view(options, true, window, cx);
        let events = self.follow(&view, window, cx);
        self.runs_started += 1;
        let tab = TaskTab {
            id,
            label: debuggee.name.clone(),
            root: self.cwd.clone(),
            started: self.runs_started,
            ordinal,
            run: TaskRun::Running,
            debuggee: true,
        };
        self.put_task_tab(reused, view.clone(), tab, events, cx);
        Ok(view)
    }

    /// The process id a debuggee tab's `view` runs, once its session has
    /// started: `None` until then, `Some(None)` for a test's fake session.
    pub(crate) fn started_pid(&self, view: &Entity<TerminalView>, cx: &App) -> Option<Option<u32>> {
        #[cfg(test)]
        if self.fake_sessions {
            return (!self.debuggees_unstarted).then_some(None);
        }
        let terminal = view.read(cx).terminal()?;
        Some(Some(terminal.pid()))
    }

    /// The rerun of the debuggee tab showing `view`'s session: asks the
    /// debugger for a fresh session of its configuration. Nothing for any
    /// other tab.
    pub(crate) fn rerun_debuggee(&mut self, view: EntityId, cx: &mut Context<Self>) {
        let tab = (self.sessions.iter())
            .find(|session| session.view.entity_id() == view)
            .and_then(|session| session.task.as_ref())
            .filter(|tab| tab.debuggee);
        if let Some(tab) = tab {
            let rerun = DebuggeeRerun {
                name: tab.label.clone(),
                root: tab.root.clone(),
            };
            cx.emit(rerun);
        }
    }

    /// Closes the tab showing `view`'s session if its process hasn't
    /// started (it starts on the tab's first layout), so it never will: a
    /// debuggee tab whose session gave up on it. One that has started keeps
    /// its tab, its output and its rerun.
    pub(crate) fn close_unstarted(
        &mut self,
        view: EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.position(view) else {
            return;
        };
        if self.started_pid(&self.sessions[ix].view, cx).is_none() {
            self.remove(ix, window, cx);
        }
    }

    /// Shows `tab`'s session `view` in session `at`'s place, or in a new
    /// session after the last, and makes it active.
    fn put_task_tab(
        &mut self,
        at: Option<usize>,
        view: Entity<TerminalView>,
        tab: TaskTab,
        events: Subscription,
        cx: &mut Context<Self>,
    ) {
        let ix = match at {
            // Dropping the old view hangs up what's left of its session; its
            // exit has been told already.
            Some(ix) => {
                let session = &mut self.sessions[ix];
                session.view = view;
                session.task = Some(tab);
                session.status = CommandStatus::Idle;
                session._events = events;
                ix
            }
            // Tasks don't count towards `Terminal <n>`.
            None => {
                self.sessions.push(Session {
                    view,
                    number: 0,
                    name: None,
                    task: Some(tab),
                    status: CommandStatus::Idle,
                    lane: None,
                    read_only: false,
                    command: None,
                    profile: None,
                    _events: events,
                });
                self.sessions.len() - 1
            }
        };
        self.session_added(ix, cx);
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
                    options.shell = Some(sh_c(verbatim!("exit 127")));
                    options.env.clear();
                }
                TerminalView::new(options, style, window, cx).expect("/ is a directory")
            })
        })
    }

    /// Hears `view`'s session exit, where a shell's tab goes and a task's
    /// stays; and its shell's foreground command start or end, which its
    /// tab shows.
    fn follow(
        &self,
        view: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(view, window, |panel, view, event, window, cx| {
            let Some(ix) = panel.sessions.iter().position(|s| &s.view == view) else {
                return;
            };
            match *event {
                TerminalViewEvent::CommandStatus(status) => {
                    panel.sessions[ix].status = status;
                    cx.notify();
                }
                TerminalViewEvent::Exited(code) if panel.sessions[ix].task.is_some() => {
                    panel.task_exited(ix, code, window, cx);
                }
                TerminalViewEvent::Exited(_) => panel.remove(ix, window, cx),
            }
        })
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
            let message = tr!("workbench-terminal-task-couldnt-start", title = &title);
            toast::show(cx, Toast::error(message));
            self.remove(ix, window, cx);
            return;
        }
        tab.run = if code == Some(0) {
            TaskRun::Passed
        } else {
            TaskRun::Failed
        };
        let (id, debuggee) = (tab.id, tab.debuggee);
        cx.notify();
        let message = match code {
            Some(0) => return,
            Some(code) => tr!(
                "workbench-terminal-task-failed",
                title = &title,
                code = code
            ),
            None => tr!("workbench-terminal-task-killed", title = &title),
        };
        let (answer, answered) = futures::channel::oneshot::channel();
        let mut choices = vec![tr!("workbench-terminal-task-show").to_string()];
        if debuggee {
            choices.push(tr!("workbench-debug-debuggee-rerun").to_string());
        }
        let toast = Toast::error(message).choices(choices, move |choice| {
            answer.send(choice).ok();
        });
        toast::show(cx, toast);
        cx.spawn_in(window, async move |panel, cx| {
            let choice = answered.await;
            panel
                .update_in(cx, |panel, window, cx| match choice {
                    Ok(Some(0)) => panel.show_task(id, window, cx),
                    Ok(Some(1)) => panel.rerun_task_tab(id, cx),
                    _ => {}
                })
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

    /// [`Self::rerun_debuggee`] for the tab `id`, if it's still open.
    fn rerun_task_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let view = (self.sessions.iter())
            .find(|session| session.task.as_ref().is_some_and(|tab| tab.id == id))
            .map(|session| session.view.entity_id());
        if let Some(view) = view {
            self.rerun_debuggee(view, cx);
        }
    }

    /// How the task `label` run from `root`, last went,
    /// whoever started it: running if any of its tabs is, else how its
    /// latest run ended; none if it has no tab.
    pub(crate) fn task_run(&self, root: &Path, label: &str) -> Option<TaskRun> {
        let tabs = (self.sessions.iter())
            .filter_map(|session| session.task.as_ref())
            .filter(|tab| !tab.debuggee && tab.label == label && tab.root == root);
        let mut latest: Option<&TaskTab> = None;
        for tab in tabs {
            if tab.run == TaskRun::Running {
                return Some(TaskRun::Running);
            }
            if latest.is_none_or(|latest| tab.started > latest.started) {
                latest = Some(tab);
            }
        }
        latest.map(|tab| tab.run)
    }

    /// Stops the task `label` run from `root`: drops its
    /// running sessions, which hangs them up.
    pub(crate) fn stop_task(
        &mut self,
        root: &Path,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        while let Some(ix) = self.sessions.iter().position(|session| {
            (session.task.as_ref()).is_some_and(|tab| {
                !tab.debuggee
                    && tab.label == label
                    && tab.root == root
                    && tab.run == TaskRun::Running
            })
        }) {
            self.remove(ix, window, cx);
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
            tr!("workbench-terminal-task-passed", label = &label)
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
            return tab.title();
        }
        if let (Some(lane), true, Some(command)) =
            (session.lane, session.read_only, &session.command)
        {
            return tr!(
                "workbench-terminal-lane",
                lane = lane.get(),
                command = command
            );
        }
        match (session.view.read(cx).title(), &session.profile) {
            (Some(title), _) => title.to_owned().into(),
            (None, Some(profile)) => profile.clone(),
            (None, None) => tr!("workbench-terminal-numbered", number = session.number),
        }
    }

    /// The panel's own strip, 34 px as the dock's: a tab per session in
    /// start order, each with its [`TabMark`] before its label, the active
    /// one with the accent line along its top while the panel has focus;
    /// then, at the right, a `+` running `terminal.new` and a kill button
    /// running `terminal.close`, whose tooltips carry the commands' keys. A
    /// click on a tab activates and focuses its session, a double click
    /// asks for its new name, and its close button or a middle click kills
    /// it. The tabs scroll sideways when they don't fit, the active one into
    /// view as it changes. A tab and its handlers name its session by its
    /// view's id, so a click lands on the session that was drawn there or,
    /// if that's gone, on nothing.
    ///
    /// Drawn from two sessions ([`TerminalPanel::has_strip`]); with one the
    /// dock's strip is the panel's only header, and carries the `+`, the
    /// kill and a debuggee's rerun ([`DockItem::actions`]).
    fn tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = fe_ui::ui_theme(cx);
        let colors = &cx.theme().colors;
        let (chrome, border) = (colors.surface.chrome, colors.border.default);
        let agent = colors.agent.base;
        #[cfg(test)]
        {
            self.strip_drawn = true;
        }
        let tabs: Vec<_> = (0..self.sessions.len())
            .map(|ix| {
                let id = self.sessions[ix].view.entity_id();
                let label = self.label(ix, cx);
                let active = ix == self.active;
                let lane = self.sessions[ix].lane.is_some();
                let agent = lane.then_some(agent);
                let kill = move |panel: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                    if let Some(ix) = panel.position(id) {
                        panel.remove(ix, window, cx);
                    }
                };
                Tab::new(
                    ElementId::NamedInteger("terminal-tab".into(), id.as_u64()),
                    label,
                    &ui,
                )
                .icon(Some(self.sessions[ix].mark().icon(lane)))
                .label_color(agent)
                .active(active)
                .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                    if event.click_count() >= 2 {
                        cx.emit(TerminalPanelEvent::RenameRequested(id));
                    } else if let Some(ix) = panel.position(id) {
                        panel.activate(ix, window, cx);
                    }
                }))
                .on_close(cx.listener(move |panel, _, window, cx| kill(panel, window, cx)))
                .on_middle_click(cx.listener(move |panel, _, window, cx| kill(panel, window, cx)))
            })
            .collect();
        if std::mem::take(&mut self.reveal) {
            self.tabs.scroll_to_item(self.active);
        }
        let bar = TabBar::new("terminal-tabs", &ui)
            .track_scroll(&self.tabs)
            .focus_within(&self.focus)
            .children(tabs);
        let tip = move |title: SharedString, command: &'static str| {
            move |_: &mut Window, cx: &mut App| {
                let key = header_shortcut(command, cx);
                let tooltip = Tooltip::new(title.clone(), key, &ui);
                #[cfg(test)]
                LAST_TOOLTIP.replace(Some(tooltip.text()));
                cx.new(|_| tooltip).into()
            }
        };
        let (new, kill, profiles) = (
            tr!("workbench-terminal-new"),
            tr!("workbench-terminal-kill"),
            tr!("workbench-terminal-profiles"),
        );
        let rerun = self.debuggee().map(|view| {
            let title = tr!("workbench-debug-debuggee-rerun-session");
            div()
                .id("terminal-debuggee-rerun")
                .debug_selector(|| "terminal-debuggee-rerun".to_owned())
                .child(
                    IconButton::new(
                        "terminal-debuggee-rerun-button",
                        Icon::RefreshCw,
                        title.clone(),
                        &ui,
                    )
                    .tooltip(tip(title, "debug.rerun_last"))
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        panel.rerun_debuggee(view, cx);
                    })),
                )
        });
        let actions = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .h(TAB_HEIGHT)
            .bg(chrome)
            .border_b_1()
            .border_color(border)
            .children(rerun)
            .child(
                div()
                    .id("terminal-new")
                    .debug_selector(|| "terminal-new".to_owned())
                    .child(
                        IconButton::new("terminal-new-button", Icon::Plus, new.clone(), &ui)
                            .tooltip(tip(new, "terminal.new"))
                            .on_click(|_, window, cx| {
                                pane_toolbar::dispatch("terminal.new", Source::Mouse, window, cx);
                            }),
                    ),
            )
            .child({
                let (drawn, caret) = (self.caret_bounds.clone(), self.caret_bounds.clone());
                div()
                    .on_children_prepainted(move |bounds, _, _| drawn.set(bounds.first().copied()))
                    .child(
                        IconButton::new(PROFILES_BUTTON, Icon::ChevronDown, profiles.clone(), &ui)
                            .tooltip(tip(profiles, "terminal.new_with_profile"))
                            .on_click(cx.listener(move |panel, event: &ClickEvent, window, cx| {
                                let at = event.position();
                                let anchor = caret.get().unwrap_or_else(|| {
                                    gpui::Bounds::new(at, gpui::size(px(0.), px(0.)))
                                });
                                panel.toggle_profile_menu(anchor, window, cx);
                            })),
                    )
            })
            .child(
                IconButton::new("terminal-kill", Icon::X, kill.clone(), &ui)
                    .tooltip(tip(kill, "terminal.close"))
                    .on_click(|_, window, cx| {
                        pane_toolbar::dispatch("terminal.close", Source::Mouse, window, cx);
                    }),
            );
        div()
            .flex_none()
            .flex()
            .w_full()
            .h(TAB_HEIGHT)
            .child(div().flex_1().min_w_0().child(bar))
            .child(actions)
    }

    /// Whether the panel draws its own strip of session tabs: from two
    /// sessions, a lane's among them. With one, or none, the dock's strip
    /// is its only header row.
    fn has_strip(&self) -> bool {
        self.sessions.len() >= 2
    }

    /// The active session's view if it is a debuggee tab's, whose rerun the
    /// header offers: a fresh debug session of its configuration, never
    /// its argv again.
    fn debuggee(&self) -> Option<EntityId> {
        (self.sessions.get(self.active))
            .filter(|session| session.task.as_ref().is_some_and(|tab| tab.debuggee))
            .map(|session| session.view.entity_id())
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

impl TerminalPanel {
    /// The caret's click: opens the menu of profiles hanging from
    /// `anchor`, the caret's bounds, or closes it if it's open. The panel takes focus while it's
    /// open, for its keys: Up and Down move its highlight, wrapping, Enter
    /// starts the highlighted profile and Escape closes it. A click outside
    /// it, or focus leaving the panel, closes it too.
    pub(crate) fn toggle_profile_menu(
        &mut self,
        anchor: gpui::Bounds<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.profile_menu.is_some() {
            return self.close_profile_menu(window, cx);
        }
        let back = window.focused(cx);
        window.focus(&self.focus, cx);
        let focus_out = cx.on_focus_out(&self.focus, window, |panel, _, _, cx| {
            if panel.profile_menu.take().is_some() {
                cx.notify();
            }
        });
        self.profile_menu = Some(ProfileMenu {
            highlighted: None,
            anchor,
            back,
            _focus_out: focus_out,
        });
        cx.notify();
    }

    /// Closes the menu of profiles, starting nothing, and gives focus back
    /// to what had it.
    fn close_profile_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.profile_menu.take() else {
            return;
        };
        match menu.back {
            Some(back) => window.focus(&back, cx),
            None => self.focus_active(window, cx),
        }
        cx.notify();
    }

    /// Starts the menu's profile `id`, closing it.
    fn choose_profile(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.profile_menu.take().is_none() {
            return;
        }
        cx.notify();
        self.add_profile(id, window, cx);
        window.focus(&self.focus, cx);
    }

    /// Escape, Up, Down and Enter while the menu of profiles is open; other
    /// keys do nothing then.
    fn on_profile_menu_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.profile_menu.is_none() {
            return;
        }
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        cx.stop_propagation();
        let profiles = self.profiles();
        let Some(menu) = &mut self.profile_menu else {
            return;
        };
        let at = (menu.highlighted.as_ref())
            .and_then(|id| profiles.iter().position(|profile| &profile.id == id));
        match keystroke.key.as_str() {
            "escape" => self.close_profile_menu(window, cx),
            "down" | "up" if !profiles.is_empty() => {
                let last = profiles.len() - 1;
                let to = match (at, keystroke.key == "down") {
                    (None, true) => 0,
                    (None, false) => last,
                    (Some(at), true) if at >= last => 0,
                    (Some(at), true) => at + 1,
                    (Some(at), false) => at.checked_sub(1).unwrap_or(last),
                };
                menu.highlighted = Some(profiles[to].id.clone());
                cx.notify();
            }
            "enter" => {
                if let Some(at) = at {
                    self.choose_profile(&profiles[at].id, window, cx);
                }
            }
            _ => {}
        }
    }

    /// The open menu of profiles, over the window under the caret: a row
    /// per profile, or one saying there's none.
    fn render_profile_menu(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        use fe_ui::{MenuEntry, MenuPanel};
        use gpui::{anchored, deferred};
        let menu = self.profile_menu.as_ref()?;
        let profiles = self.profiles();
        let none = profiles.is_empty();
        let entries = if none {
            vec![MenuEntry::Item {
                title: tr!("workbench-terminal-profiles-none"),
                shortcut: None,
            }]
        } else {
            (profiles.iter())
                .map(|profile| MenuEntry::Item {
                    title: profile_label(profile),
                    shortcut: None,
                })
                .collect()
        };
        let highlighted = (menu.highlighted.as_ref())
            .and_then(|id| profiles.iter().position(|profile| &profile.id == id));
        // Rows by the id they drew with, should the list change under them.
        let ids: Rc<[String]> = profiles.iter().map(|profile| profile.id.clone()).collect();
        let (panel, hovered) = (cx.entity().downgrade(), cx.entity().downgrade());
        let clicked = Rc::clone(&ids);
        let ui = fe_ui::ui_theme(cx);
        let mut menu_panel = MenuPanel::new(entries, &ui)
            .item_nodes()
            .highlighted(highlighted)
            .on_click(move |ix, window, cx| {
                let Some(id) = clicked.get(ix) else {
                    return;
                };
                (panel.update(cx, |panel, cx| panel.choose_profile(id, window, cx))).ok();
            })
            .on_hover(move |ix, _, cx| {
                let Some(id) = ids.get(ix) else {
                    return;
                };
                (hovered.update(cx, |panel, cx| {
                    if let Some(menu) = &mut panel.profile_menu
                        && menu.highlighted.as_ref() != Some(id)
                    {
                        menu.highlighted = Some(id.clone());
                        cx.notify();
                    }
                }))
                .ok();
            });
        if none {
            menu_panel = menu_panel.disabled(vec![0]);
        }
        // A press on the caret is its click's, which closes the menu.
        let anchor = menu.anchor;
        let wrapper = div()
            .id("terminal-profile-menu")
            .debug_selector(|| "terminal-profile-menu".into())
            .on_mouse_down_out(cx.listener(
                move |panel, event: &gpui::MouseDownEvent, window, cx| {
                    if !anchor.contains(&event.position) {
                        panel.close_profile_menu(window, cx);
                    }
                },
            ))
            .child(menu_panel);
        let below = anchor.bottom_left() + gpui::point(px(0.), px(2.));
        Some(
            deferred(
                anchored()
                    .position(below)
                    .snap_to_window_with_margin(px(4.))
                    .child(wrapper),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// The open menu's profiles, by name, if it's open.
    #[cfg(test)]
    pub(crate) fn profile_menu_names(&self) -> Option<Vec<String>> {
        self.profile_menu.as_ref()?;
        Some(self.profiles().into_iter().map(|p| p.name).collect())
    }
}

/// The key a header button's tooltip shows for `command`, as the dock
/// strip's buttons show it (kiln-shell's `DockState`, whose contexts the
/// workbench sets to these): the first sequence that runs it there, a chord
/// too. So `+`, kill and a debuggee's rerun tell the same key in the
/// panel's strip as in the dock's, where they are with one session.
fn header_shortcut(command: &str, cx: &App) -> Option<String> {
    let command = CommandId::parse(command).ok()?;
    let keymap = fe_gpui::current_keymap(cx);
    let stack = pane_toolbar::editor_context();
    let found = fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack);
    found.first().map(fe_keymap::shortcut_label)
}

/// What runs `task`, which only ever runs where it says: a shell line in
/// `/bin/sh` (Windows PowerShell on Windows), not the user's login shell, so
/// that it means the same for everyone sharing the project, and a program
/// directly.
///
/// # Errors
///
/// When its directory isn't one, or it names no program.
pub(crate) fn task_options(task: &Task, scrollback: usize) -> io::Result<TerminalOptions> {
    if !std::fs::metadata(&task.cwd)?.is_dir() {
        let path = task.cwd.display().to_string();
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            tr!("workbench-terminal-task-not-a-directory", path = path).to_string(),
        ));
    }
    let shell = match &task.command {
        TaskCommand::Shell(line) => sh_c(line),
        TaskCommand::Argv(argv) => {
            let Some((program, args)) = argv.split_first() else {
                let error = tr!("workbench-terminal-task-no-command").to_string();
                return Err(io::Error::new(io::ErrorKind::InvalidInput, error));
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

/// What runs `debuggee`: its argv directly, never through a shell, with its
/// environment.
///
/// # Errors
///
/// When its directory isn't one, or its argv is empty.
fn debuggee_options(debuggee: &Debuggee, scrollback: usize) -> io::Result<TerminalOptions> {
    if !std::fs::metadata(&debuggee.cwd)?.is_dir() {
        let path = debuggee.cwd.display().to_string();
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            tr!("workbench-terminal-task-not-a-directory", path = path).to_string(),
        ));
    }
    let Some((program, args)) = debuggee.argv.split_first() else {
        let error = tr!("workbench-terminal-task-no-command").to_string();
        return Err(io::Error::new(io::ErrorKind::InvalidInput, error));
    };
    let env = (debuggee.env.iter())
        .filter_map(|(name, value)| Some((name.clone(), value.clone()?)))
        .collect();
    // A name with an `=` in it (or none) can't be a variable's.
    let removed: Vec<&String> = (debuggee.env.iter())
        .filter(|(name, value)| value.is_none() && !name.is_empty() && !name.contains('='))
        .map(|(name, _)| name)
        .collect();
    Ok(TerminalOptions {
        shell: Some(without_env(&removed, program, args)?),
        scrollback,
        env,
        ..TerminalOptions::new(&debuggee.cwd)
    })
}

/// What runs `program` with `args`, with the variables `removed` taken out
/// of the environment it inherits: through `env -u`, which execs it, on
/// Unix. A program whose path holds an `=`, which `env` would read as a
/// variable to set, is exec'd from `/bin/sh -c 'exec "$0" "$@"'` after it:
/// its path and arguments are passed as they are, never read by the shell.
///
/// # Errors
///
/// On Windows, which has no `env`, when there's anything to remove: the
/// program mustn't run with what the adapter asked to be kept from it.
fn without_env(
    removed: &[&String],
    program: &str,
    args: &[String],
) -> io::Result<(String, Vec<String>)> {
    if removed.is_empty() {
        return Ok((program.to_owned(), args.to_vec()));
    }
    if cfg!(windows) {
        let names: Vec<&str> = removed.iter().map(|name| name.as_str()).collect();
        let error = tr!("workbench-debug-env-unremovable", names = names.join(", "));
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            error.to_string(),
        ));
    }
    let mut argv = Vec::new();
    for name in removed {
        argv.push(verbatim!("-u").to_owned());
        argv.push((*name).clone());
    }
    argv.push(verbatim!("--").to_owned());
    if program.contains('=') {
        argv.extend(
            [verbatim!("/bin/sh"), "-c", verbatim!("exec \"$0\" \"$@\"")].map(str::to_owned),
        );
    }
    argv.push(program.to_owned());
    argv.extend(args.iter().cloned());
    Ok((verbatim!("/usr/bin/env").to_owned(), argv))
}

/// `/bin/sh -c line`.
#[cfg(not(windows))]
fn sh_c(line: &str) -> (String, Vec<String>) {
    let args = vec!["-c".to_owned(), line.to_owned()];
    ("/bin/sh".to_owned(), args)
}

/// On Windows, Windows PowerShell's `-Command line`: the shell every
/// Windows has, at the same place, and one that takes the line as one
/// argument, where `cmd.exe /C` has no quoting that keeps any line whole. It
/// exits with `exit`'s code, else 1 if the last command failed.
#[cfg(windows)]
fn sh_c(line: &str) -> (String, Vec<String>) {
    let root = std::env::var(verbatim!("SystemRoot"))
        .ok()
        .filter(|root| !root.is_empty())
        .unwrap_or_else(|| r"C:\Windows".to_owned());
    let program = root + verbatim!(r"\System32\WindowsPowerShell\v1.0\powershell.exe");
    let args = ["-NoLogo", "-NoProfile", "-Command", line].map(str::to_owned);
    (program, args.to_vec())
}

/// `cwd` if it's a directory, else the home directory, else the root.
fn start_dir(cwd: &Path) -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| {
            cfg!(windows)
                .then(|| std::env::var_os("USERPROFILE"))
                .flatten()
        })
        .map(PathBuf::from);
    [Some(cwd.to_owned()), home]
        .into_iter()
        .flatten()
        .find(|dir| dir.is_dir())
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { r"C:\" } else { "/" }))
}

/// The terminal's style: the editor's colours and, unless `settings` set
/// them, its font and the design's Code small (12 px on 18 px rows), and
/// ANSI colours from the theme's `[terminal]` table. A size the settings set
/// keeps Code small's proportion: its rows are half as tall again.
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
        line_height: fe_ui::CODE_SMALL.line_height / fe_ui::CODE_SMALL.size,
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
    /// shell set, else its profile's name, else "Terminal".
    fn title(&self, cx: &App) -> SharedString {
        if self
            .sessions
            .get(self.active)
            .is_some_and(|s| s.task.is_some())
        {
            return self.label(self.active, cx);
        }
        let title = self.view().and_then(|view| view.read(cx).title());
        let profile = (self.sessions.get(self.active)).and_then(|s| s.profile.clone());
        match (title, profile) {
            (Some(title), _) => title.to_owned().into(),
            (None, Some(profile)) => profile,
            (None, None) => tr!("workbench-terminal-title"),
        }
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

    /// With one session, which draws no strip of its own, the strip's
    /// buttons, in its order and with its names, tooltips and commands: a
    /// debuggee's rerun, `+`, its caret's menu of profiles and kill. None
    /// from two, whose strip has them.
    fn actions(&self, _: &App) -> Vec<PaneAction> {
        if self.has_strip() {
            return Vec::new();
        }
        let rerun = self.debuggee().map(|view| {
            let this = self.this.clone();
            PaneAction {
                id: "terminal-debuggee-rerun-button",
                icon: Icon::RefreshCw,
                title: tr!("workbench-debug-debuggee-rerun-session"),
                command: Some("debug.rerun_last"),
                run: Rc::new(move |_, cx| {
                    (this.update(cx, |panel, cx| panel.rerun_debuggee(view, cx))).ok();
                }),
                anchored: None,
            }
        });
        let action = |id, icon, title, command: &'static str| PaneAction {
            id,
            icon,
            title,
            command: Some(command),
            run: Rc::new(move |window, cx| {
                pane_toolbar::dispatch(command, Source::Mouse, window, cx);
            }),
            anchored: None,
        };
        let this = self.this.clone();
        let profiles = PaneAction {
            id: PROFILES_BUTTON,
            icon: Icon::ChevronDown,
            title: tr!("workbench-terminal-profiles"),
            command: Some("terminal.new_with_profile"),
            run: Rc::new(move |window, cx| {
                pane_toolbar::dispatch("terminal.new_with_profile", Source::Mouse, window, cx);
            }),
            anchored: Some(Rc::new(move |anchor, window, cx| {
                (this.update(cx, |panel, cx| {
                    panel.toggle_profile_menu(anchor, window, cx)
                }))
                .ok();
            })),
        };
        (rerun.into_iter())
            .chain([
                action(
                    "terminal-new-button",
                    Icon::Plus,
                    tr!("workbench-terminal-new"),
                    "terminal.new",
                ),
                profiles,
                action(
                    "terminal-kill",
                    Icon::X,
                    tr!("workbench-terminal-kill"),
                    "terminal.close",
                ),
            ])
            .collect()
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Focused itself, as when the dock shows it: the active session
        // takes focus. Shown with no session, as by a rail click after the
        // shell exited: start one.
        let focused = self.focus.is_focused(window);
        // One asked for by name that detection didn't find hides the
        // panel rather than starting the default.
        if !self.start_waiting(window, cx) {
            self.start(window, cx);
        }
        // The menu of profiles holds focus for its keys.
        if focused && self.profile_menu.is_none() {
            self.focus_active(window, cx);
        }
        // A query set before its session started is checked once it has.
        self.update_find(|find, view, cx| find.settle(view, cx), cx);
        #[cfg(test)]
        self.record_tabs(cx);
        let strip = self.has_strip().then(|| self.tab_strip(cx));
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
        let profile_menu = self.render_profile_menu(cx);
        div()
            .track_focus(&self.focus)
            .key_context(context)
            .on_key_down(cx.listener(Self::on_profile_menu_key))
            .size_full()
            .flex()
            .flex_col()
            .bg(self.style.background)
            .children(strip)
            .children(find)
            .child(
                div()
                    .debug_selector(|| "terminal-session".to_owned())
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .relative()
                    .children(unstarted)
                    .children(session),
            )
            .children(profile_menu)
    }
}

#[cfg(test)]
impl TerminalPanel {
    /// Runs `shell` in place of the user's in the sessions started from
    /// now.
    pub(crate) fn set_shell(&mut self, shell: &str, args: &[&str]) {
        // `/bin/sh` alone is the tests' interactive shell, whichever: on
        // Windows, cmd.exe.
        #[cfg(windows)]
        let shell = if shell == "/bin/sh" && args.is_empty() {
            "cmd.exe"
        } else {
            shell
        };
        let args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self.shell = Some((shell.to_owned(), args));
    }

    /// Notes each session's tab as this frame has it, whether or not the
    /// strip draws (it does from two sessions): its label and background,
    /// its label's colour and its icon.
    fn record_tabs(&mut self, cx: &App) {
        let colors = &cx.theme().colors;
        let (agent, tab_active, tab_inactive) =
            (colors.agent.base, colors.tab.active, colors.tab.inactive);
        self.strip_drawn = false;
        self.drawn_tabs.clear();
        self.drawn_tab_marks.clear();
        for ix in 0..self.sessions.len() {
            let lane = self.sessions[ix].lane.is_some();
            let icon = self.sessions[ix].mark().icon(lane);
            self.drawn_tab_marks.push((lane.then_some(agent), icon));
            let background = if ix == self.active {
                tab_active
            } else {
                tab_inactive
            };
            self.drawn_tabs.push((self.label(ix, cx), background));
        }
    }

    /// Each session's tab as the last frame had it, drawn or not: the
    /// colour its label was given and its icon.
    pub(crate) fn drawn_tab_marks(&self) -> &[(Option<gpui::Hsla>, IconRef)] {
        &self.drawn_tab_marks
    }

    /// Each session's tab as the last frame had it, drawn or not: label
    /// and background.
    pub(crate) fn drawn_tabs(&self) -> &[(SharedString, gpui::Hsla)] {
        &self.drawn_tabs
    }

    /// Whether the last frame drew the panel's strip of session tabs.
    pub(crate) fn strip_drawn(&self) -> bool {
        self.strip_drawn
    }

    /// Where tab `ix` was drawn in the last frame, as the strip scrolled.
    pub(crate) fn tab_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        // The strip's handle keeps the bounds it was last laid out with.
        if !self.has_strip() {
            return None;
        }
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
            tr!("workbench-terminal-rename-clear").to_string()
        } else {
            tr!("workbench-terminal-rename-to", name = &self.name).to_string()
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

    /// The row's text: the name it gives the shell.
    fn match_label(&self, _: usize, _: &App) -> gpui::SharedString {
        self.label().into()
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
        Some(tr!("workbench-terminal-rename-status"))
    }
}

/// `terminal.new_with_profile`'s picker: every profile, those whose name
/// has the query in it, ignoring case; Enter starts one. It lists them again
/// as they change (detection finding more, the settings changing), keeping
/// the query and the selected profile.
pub(crate) struct PickProfile {
    workbench: gpui::WeakEntity<crate::Workbench>,
    profiles: Vec<Profile>,
    /// The indexes into `profiles` of the rows shown.
    shown: Vec<usize>,
    /// The terminal's [`TerminalPanelEvent::ProfilesChanged`].
    _changed: Subscription,
}

impl PickProfile {
    /// The picker of `terminal`'s profiles.
    pub(crate) fn new(
        workbench: gpui::WeakEntity<crate::Workbench>,
        terminal: &Entity<TerminalPanel>,
        cx: &mut Context<Picker<Self>>,
    ) -> Self {
        let profiles = terminal.read(cx).profiles();
        let changed = cx.subscribe(
            terminal,
            |picker, terminal, event: &TerminalPanelEvent, cx| {
                if *event == TerminalPanelEvent::ProfilesChanged {
                    let profiles = terminal.read(cx).profiles();
                    let selected = picker.selected();
                    let query = picker.input().read(cx).text().to_owned();
                    picker.delegate.relist(profiles, selected, &query, cx);
                }
            },
        );
        let shown = (0..profiles.len()).collect();
        Self {
            workbench,
            profiles,
            shown,
            _changed: changed,
        }
    }

    fn profile(&self, ix: usize) -> Option<&Profile> {
        self.profiles.get(*self.shown.get(ix)?)
    }

    /// The rows of `profiles` matching `query`.
    fn matching(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        (0..self.profiles.len())
            .filter(|&ix| {
                let label = profile_label(&self.profiles[ix]).to_lowercase();
                label.contains(&query) || self.profiles[ix].name.to_lowercase().contains(&query)
            })
            .collect()
    }

    /// Lists `profiles` in place of the profiles it had, matching `query`,
    /// with the profile that was on row `selected` still selected if it's
    /// there.
    fn relist(
        &mut self,
        profiles: Vec<Profile>,
        selected: usize,
        query: &str,
        cx: &mut Context<Picker<Self>>,
    ) {
        if profiles == self.profiles {
            return;
        }
        let picker = cx.entity();
        let was = self.profile(selected).map(|profile| profile.id.clone());
        self.profiles = profiles;
        self.shown = self.matching(query);
        let at = was.and_then(|id| {
            (0..self.shown.len()).find(|&ix| self.profile(ix).is_some_and(|p| p.id == id))
        });
        cx.notify();
        // `Picker::select` borrows the picker, which is being updated now.
        cx.defer(move |cx| {
            picker.update(cx, |picker, cx| match at {
                Some(at) => picker.select(at, cx),
                None => picker.matches_updated(false, cx),
            })
        });
    }
}

impl PickerDelegate for PickProfile {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.shown = self.matching(&query);
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.shown.len()
    }

    fn match_id(&self, ix: usize) -> gpui::SharedString {
        self.profile(ix)
            .map_or_else(SharedString::default, |p| p.id.clone().into())
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> gpui::AnyElement {
        use gpui::{IntoElement as _, ParentElement as _, Styled as _};
        let Some(profile) = self.profile(ix) else {
            return div().into_any_element();
        };
        let ui = fe_ui::ui_theme(cx);
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                gpui::svg()
                    .path(profile_icon(profile).path())
                    .size(px(14.))
                    .flex_none()
                    .text_color(ui.icon),
            )
            .child(profile_label(profile))
            .into_any_element()
    }

    /// The row's text: the profile's name.
    fn match_label(&self, ix: usize, _: &App) -> gpui::SharedString {
        self.profile(ix).map(profile_label).unwrap_or_default()
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(id) = self.profile(ix).map(|profile| profile.id.clone()) else {
            return false;
        };
        let workbench = self.workbench.clone();
        // Once the picker has gone: the workbench moves focus.
        window.defer(cx, move |window, cx| {
            (workbench.update(cx, |workbench, cx| {
                workbench.new_terminal_with_profile(Some(id), window, cx)
            }))
            .ok();
        });
        true
    }

    fn dismissed(&mut self, _: &mut Window, _: &mut Context<Picker<Self>>) {}

    fn status(&self) -> Option<gpui::SharedString> {
        Some(tr!("workbench-terminal-profile-status"))
    }
}

#[cfg(test)]
#[path = "terminal_rename_tests.rs"]
mod terminal_rename_tests;
#[cfg(test)]
#[path = "terminal_tabs_tests.rs"]
pub(crate) mod terminal_tabs_tests;

#[cfg(test)]
#[path = "terminal_lane_tests.rs"]
mod terminal_lane_tests;

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
thread_local! {
    /// The text of the last strip-button tooltip built, for the tests.
    static LAST_TOOLTIP: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The text of the last strip-button tooltip built on this thread.
#[cfg(test)]
pub(crate) fn last_strip_tooltip() -> Option<String> {
    LAST_TOOLTIP.with_borrow(Clone::clone)
}

#[cfg(test)]
#[path = "terminal_actions_tests.rs"]
mod terminal_actions_tests;

#[cfg(test)]
#[path = "terminal_keys_tests.rs"]
mod terminal_keys_tests;
