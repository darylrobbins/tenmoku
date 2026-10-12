//! The terminal on the bottom dock: a shell in the project root, shown and
//! focused by `terminal.toggle`, as VS Code's ctrl-` does.
//!
//! The panel starts its session on first show ([`TerminalPanel::start`]) and
//! keeps it while the dock hides. When the shell exits the panel drops it
//! and tells the workbench ([`TerminalPanelEvent::Exited`]), which hides
//! the dock; the next show starts a new one. Closing the window drops the
//! session, which hangs the shell up.

use std::{path::PathBuf, rc::Rc};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::{EditorTheme, commands::RunCommand};
use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView, TerminalViewEvent};
use fe_ui::Icon;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window, div,
};
use kiln_shell::{DockItem, PanelId};

/// The panel's id in the bottom dock.
pub(crate) const TERMINAL: PanelId = PanelId("terminal");

/// What the panel tells the workbench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalPanelEvent {
    /// The shell exited, or couldn't start, and the panel dropped it.
    Exited,
}

pub struct TerminalPanel {
    /// The session, from its first show until its shell exits.
    view: Option<Entity<TerminalView>>,
    /// Where a session starts.
    cwd: PathBuf,
    style: TerminalStyle,
    /// Focused while there's no session, which starts one as the panel
    /// draws.
    focus: FocusHandle,
    /// The shell a session runs instead of the user's.
    #[cfg(test)]
    shell: Option<(String, Vec<String>)>,
    /// Sessions started.
    #[cfg(test)]
    pub(crate) started: usize,
    _exited: Option<Subscription>,
}

impl EventEmitter<TerminalPanelEvent> for TerminalPanel {}

impl TerminalPanel {
    /// A panel whose sessions start in `cwd`, drawn in `theme`'s font and
    /// colours.
    pub(crate) fn new(cwd: PathBuf, theme: &EditorTheme, cx: &mut Context<Self>) -> Self {
        Self {
            view: None,
            cwd,
            style: style(theme),
            focus: cx.focus_handle(),
            #[cfg(test)]
            shell: None,
            #[cfg(test)]
            started: 0,
            _exited: None,
        }
    }

    /// Where the next session starts.
    pub(crate) fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    /// Restyles the session, and the ones to come, from `theme`.
    pub(crate) fn set_theme(&mut self, theme: &EditorTheme, cx: &mut Context<Self>) {
        self.style = style(theme);
        if let Some(view) = &self.view {
            let style = self.style.clone();
            view.update(cx, |view, cx| view.set_style(style, cx));
        }
    }

    /// The session, if one is running.
    pub fn view(&self) -> Option<&Entity<TerminalView>> {
        self.view.as_ref()
    }

    /// Whether the terminal has focus.
    pub(crate) fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle(cx).contains_focused(window, cx)
    }

    /// Starts a session unless one is running. Its shell starts once the
    /// view is laid out and its size known, in the panel's directory or,
    /// if that's gone, the home directory.
    pub(crate) fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view.is_some() {
            return;
        }
        let options = TerminalOptions {
            shell: self.shell(),
            ..TerminalOptions::new(start_dir(&self.cwd))
        };
        #[cfg(test)]
        {
            self.started += 1;
        }
        let style = self.style.clone();
        let view = cx.new(|cx| {
            let fallback = (options.clone(), style.clone());
            TerminalView::new(options, style, window, cx).unwrap_or_else(|err| {
                // Removed since `start_dir` looked: the root always is one.
                log::warn!("starting the terminal in /: {err}");
                let (mut options, style) = fallback;
                options.cwd = PathBuf::from("/");
                TerminalView::new(options, style, window, cx).expect("/ is a directory")
            })
        });
        self._exited = Some(cx.subscribe_in(
            &view,
            window,
            |panel, _, TerminalViewEvent::Exited(_), _, cx| {
                panel.view = None;
                panel._exited = None;
                cx.emit(TerminalPanelEvent::Exited);
                cx.notify();
            },
        ));
        self.view = Some(view);
        cx.notify();
    }

    /// The shell sessions run: the user's, outside tests.
    fn shell(&self) -> Option<(String, Vec<String>)> {
        #[cfg(test)]
        return self.shell.clone();
        #[cfg(not(test))]
        None
    }
}

/// `cwd` if it's a directory, else the home directory, else the root.
fn start_dir(cwd: &std::path::Path) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    [Some(cwd.to_owned()), home]
        .into_iter()
        .flatten()
        .find(|dir| dir.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// The terminal's style: the editor's font and colours, and ANSI colours
/// that read on its background.
fn style(theme: &EditorTheme) -> TerminalStyle {
    TerminalStyle {
        font_family: theme.font_family.clone(),
        font_size: theme.font_size,
        line_height: TerminalStyle::LINE_HEIGHT,
        background: theme.background,
        foreground: theme.foreground,
        cursor: theme.cursor,
        selection: theme.selection,
        ansi: TerminalStyle::ansi_for(theme.background),
    }
}

impl Focusable for TerminalPanel {
    /// The session's, or the panel's own while there's none.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match &self.view {
            Some(view) => view.focus_handle(cx),
            None => self.focus.clone(),
        }
    }
}

impl DockItem for TerminalPanel {
    fn id(&self) -> PanelId {
        TERMINAL
    }

    /// The title the shell set, else "Terminal".
    fn title(&self, cx: &App) -> SharedString {
        let title = self.view.as_ref().and_then(|view| view.read(cx).title());
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
        // Shown with no session, as by a rail click after the shell exited:
        // start one, focused if the panel is.
        if self.view.is_none() {
            let focused = self.focus.is_focused(window);
            self.start(window, cx);
            if focused && let Some(view) = &self.view {
                window.focus(&view.focus_handle(cx), cx);
            }
        }
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .bg(self.style.background)
            .children(self.view.clone())
    }
}

/// The [`CommandTarget`] of the terminal's command: whether it ran.
pub(crate) struct TerminalCommands;

impl CommandTarget for TerminalCommands {
    type Tx = ();
    type Target<'a> = bool;
}

struct Commands(Rc<Registry<TerminalCommands>>);

impl Global for Commands {}

/// Registers `terminal.toggle`.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<TerminalCommands> {
    let mut registry = Registry::new();
    registry
        .register(
            "terminal.toggle",
            "Toggle Terminal",
            |toggled: &mut bool, _: &Actor| {
                *toggled = true;
                Ok(Dispatched::new(Effect::Notify))
            },
        )
        .expect("the terminal command ID is valid")
        .context(ContextPredicate::identifier(crate::KEY_CONTEXT));
    registry
}

/// Whether `run` is `terminal.toggle`, in a context where it applies.
pub(crate) fn toggles(run: &RunCommand, cx: &App) -> bool {
    let Some(commands) = cx.try_global::<Commands>() else {
        return false;
    };
    let registry = commands.0.clone();
    if registry.get(&run.0.command).is_none() {
        return false;
    }
    let mut toggled = false;
    if let Err(error) = registry.dispatch(&mut toggled, &run.0) {
        log::warn!("{error}");
    }
    toggled
}

#[cfg(test)]
impl TerminalPanel {
    /// Runs `shell` in place of the user's in the sessions started from
    /// now.
    pub(crate) fn set_shell(&mut self, shell: &str, args: &[&str]) {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self.shell = Some((shell.to_owned(), args));
    }
}

#[cfg(test)]
#[path = "terminal_panel_tests.rs"]
mod terminal_panel_tests;
