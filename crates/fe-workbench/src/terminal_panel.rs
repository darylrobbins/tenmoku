//! The terminals on the bottom dock: shells in the project root, shown and
//! focused by `terminal.toggle`, as VS Code's ctrl-` does.
//!
//! The panel starts its first session on first show
//! ([`TerminalPanel::start`]) and keeps its sessions while the dock hides.
//! `terminal.new` starts another after the last, `terminal.close` drops the
//! active one, and `terminal.next` and `terminal.previous` cycle through
//! them; with two or more a strip of tabs shows above the active one. A
//! new session starts where the active one's shell last said it was (OSC 7;
//! docs/terminal-shell-integration.md), and `terminal.previous_prompt` and
//! `terminal.next_prompt` scroll the active one between the prompts its
//! shell marked (OSC 133). When
//! a shell exits the panel drops its session; when none is left it tells
//! the workbench ([`TerminalPanelEvent::Exited`]), which hides the dock,
//! and the next show starts a new one. Closing the window drops the
//! sessions, which hangs their shells up. `terminal.find` opens a find row
//! above the active one ([`terminal_find`]).

use std::{path::PathBuf, rc::Rc};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry, Source};
use fe_config::TerminalSettings;
use fe_gpui::{EditorTheme, commands::RunCommand};
use fe_terminal_view::{TerminalOptions, TerminalStyle, TerminalView, TerminalViewEvent};
use fe_ui::{ActiveTheme as _, Button, Icon, IconButton, TAB_HEIGHT};
use gpui::{
    App, AppContext as _, Context, ElementId, Entity, EntityId, EventEmitter, FocusHandle,
    Focusable, Global, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, prelude::FluentBuilder as _, px,
};
use kiln_shell::{DockItem, PanelId};

use crate::find_bar::FindOp;
use terminal_find::TerminalFind;

#[path = "terminal_find.rs"]
mod terminal_find;

/// The panel's id in the bottom dock.
pub(crate) const TERMINAL: PanelId = PanelId("terminal");

/// What the panel tells the workbench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalPanelEvent {
    /// The last session's shell exited, or it was closed, and the panel
    /// dropped it: none is left.
    Exited,
}

/// One terminal in the panel.
struct Session {
    view: Entity<TerminalView>,
    /// Its tab's label until the shell sets a title: `Terminal <number>`.
    number: usize,
    /// To its shell exiting.
    _exited: Subscription,
}

pub struct TerminalPanel {
    /// In start order, from the first show until the last shell exits.
    sessions: Vec<Session>,
    /// The session shown, an index into `sessions` while there are any.
    active: usize,
    /// The number the next session's tab gets, from 1 again once none is
    /// left.
    next_number: usize,
    /// The tab strip's scroll offset and its tabs' bounds.
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
    /// The find row, made by the first `terminal.find`.
    find: Option<TerminalFind>,
    /// The panel's, tracked by its root, so it contains the find row and
    /// the sessions. Focused itself, as when the dock shows the panel, it
    /// hands focus to the active session as the panel draws, starting one
    /// if there's none.
    focus: FocusHandle,
    /// The shell a session runs instead of the user's.
    #[cfg(test)]
    shell: Option<(String, Vec<String>)>,
    /// Sessions started.
    #[cfg(test)]
    pub(crate) started: usize,
    /// The tabs the last frame drew, none without a strip: label and
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
            style: style(theme, &TerminalSettings::default()),
            theme: theme.clone(),
            settings: TerminalSettings::default(),
            find: None,
            focus: cx.focus_handle(),
            #[cfg(test)]
            shell: None,
            #[cfg(test)]
            started: 0,
            #[cfg(test)]
            drawn_tabs: Vec::new(),
        }
    }

    /// Where the next session starts.
    pub(crate) fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
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

    fn restyle(&mut self, cx: &mut Context<Self>) {
        self.style = style(&self.theme, &self.settings);
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
        let exited = cx.subscribe_in(
            &view,
            window,
            |panel, view, TerminalViewEvent::Exited(_), window, cx| {
                if let Some(ix) = panel.sessions.iter().position(|s| &s.view == view) {
                    panel.remove(ix, window, cx);
                }
            },
        );
        self.sessions.push(Session {
            view,
            number: self.next_number,
            _exited: exited,
        });
        self.next_number += 1;
        self.active = self.sessions.len() - 1;
        self.reveal = true;
        self.follow_find(cx);
        cx.notify();
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

    /// Session `ix`'s tab label: the title its shell set, else
    /// `Terminal <n>`.
    fn label(&self, ix: usize, cx: &App) -> SharedString {
        let session = &self.sessions[ix];
        match session.view.read(cx).title() {
            Some(title) => title.to_owned().into(),
            None => format!("Terminal {}", session.number).into(),
        }
    }

    /// One tab per session, the active one in the theme's selection
    /// colour, each with a close button, then a `+` that runs
    /// `terminal.new`. The tabs scroll sideways when they don't fit, the
    /// active one into view as it changes. A tab and its handlers name its
    /// session by its view's id, so a click lands on the session that was
    /// drawn there or, if that's gone, on nothing.
    fn tab_strip(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = fe_ui::ui_theme(cx);
        let colors = &cx.theme().colors;
        let (panel, border, hover) = (
            colors.surface.panel,
            colors.border.default,
            colors.list.row_hover,
        );
        let (inactive, selected) = (colors.tab.inactive, colors.list.row_selected);
        let (text, active_text) = (colors.text.primary, colors.tab.active_text);
        let tabs: Vec<_> = (0..self.sessions.len())
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
                let close = IconButton::new(format!("terminal-tab-close-{id}"), Icon::X, &ui)
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        cx.stop_propagation();
                        if let Some(ix) = panel.position(id) {
                            panel.remove(ix, window, cx);
                        }
                    }));
                div()
                    .id(ElementId::NamedInteger("terminal-tab".into(), id.as_u64()))
                    .flex()
                    .flex_none()
                    .h_full()
                    .items_center()
                    .gap_1()
                    .pl_3()
                    .pr_1()
                    .border_r_1()
                    .border_color(border)
                    .bg(bg)
                    .text_color(color)
                    .when(!active, |tab| tab.hover(move |style| style.bg(hover)))
                    .child(div().whitespace_nowrap().child(label))
                    .child(close)
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        if let Some(ix) = panel.position(id) {
                            panel.activate(ix, window, cx);
                        }
                    }))
            })
            .collect();
        if std::mem::take(&mut self.reveal) {
            self.tabs.scroll_to_item(self.active);
        }
        let new = Button::new("terminal-new", "+")
            .ghost()
            .small()
            .on_click(|_, window, cx| {
                crate::pane_toolbar::dispatch("terminal.new", Source::Mouse, window, cx);
            });
        div()
            .flex()
            .flex_none()
            .w_full()
            .h(TAB_HEIGHT)
            .bg(panel)
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .id("terminal-tabs")
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_x_scroll()
                    .track_scroll(&self.tabs)
                    .children(tabs),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_1()
                    .h_full()
                    .border_l_1()
                    .border_color(border)
                    .child(new),
            )
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

/// `cwd` if it's a directory, else the home directory, else the root.
fn start_dir(cwd: &std::path::Path) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    [Some(cwd.to_owned()), home]
        .into_iter()
        .flatten()
        .find(|dir| dir.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// The terminal's style: the editor's colours and, unless `settings` set
/// them, font and size, and ANSI colours that read on its background.
fn style(theme: &EditorTheme, settings: &TerminalSettings) -> TerminalStyle {
    TerminalStyle {
        font_family: settings.font_family.as_deref().map_or_else(
            || theme.font_family.clone(),
            |family| family.to_owned().into(),
        ),
        font_fallbacks: settings.font_fallbacks.iter().map(Into::into).collect(),
        font_size: settings
            .font_size
            .map_or(theme.font_size, |size| px(size as f32)),
        line_height: TerminalStyle::LINE_HEIGHT,
        background: theme.background,
        foreground: theme.foreground,
        cursor: theme.cursor,
        selection: theme.selection,
        ansi: TerminalStyle::ansi_for(theme.background),
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

    /// The title the active session's shell set, else "Terminal".
    fn title(&self, cx: &App) -> SharedString {
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
        self.start(window, cx);
        if focused {
            self.focus_active(window, cx);
        }
        // A query set before its session started is checked once it has.
        self.update_find(|find, view, cx| find.settle(view, cx), cx);
        #[cfg(test)]
        self.drawn_tabs.clear();
        let strip = (self.sessions.len() > 1).then(|| self.tab_strip(cx));
        let view = self.view().cloned();
        let ui = fe_ui::ui_theme(cx);
        let find = (self.find.as_ref()).and_then(|find| find.render(view.as_ref(), &ui, cx));
        div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(self.style.background)
            .children(strip)
            .children(find)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .children(self.view().cloned()),
            )
    }
}

/// What a terminal command asks of the workbench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalOp {
    /// `terminal.toggle`.
    Toggle,
    /// `terminal.new`.
    New,
    /// `terminal.close`.
    Close,
    /// `terminal.next`.
    Next,
    /// `terminal.previous`.
    Previous,
    /// `terminal.find`.
    Find,
    /// `terminal.previous_prompt`.
    PreviousPrompt,
    /// `terminal.next_prompt`.
    NextPrompt,
}

/// The [`CommandTarget`] of the terminal's commands: the one that ran.
pub(crate) struct TerminalCommands;

impl CommandTarget for TerminalCommands {
    type Tx = ();
    type Target<'a> = Option<TerminalOp>;
}

struct Commands(Rc<Registry<TerminalCommands>>);

impl Global for Commands {}

/// Registers the terminal's commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<TerminalCommands> {
    use TerminalOp::*;
    let mut registry = Registry::new();
    // The prompt commands apply only in a focused terminal.
    let (workbench, terminal) = (crate::KEY_CONTEXT, fe_terminal_view::KEY_CONTEXT);
    for (id, title, op, context) in [
        ("terminal.toggle", "Toggle Terminal", Toggle, workbench),
        ("terminal.new", "New Terminal", New, workbench),
        ("terminal.close", "Close Terminal", Close, workbench),
        ("terminal.next", "Next Terminal", Next, workbench),
        (
            "terminal.previous",
            "Previous Terminal",
            Previous,
            workbench,
        ),
        ("terminal.find", "Terminal: Find", Find, workbench),
        (
            "terminal.previous_prompt",
            "Terminal: Previous Prompt",
            PreviousPrompt,
            terminal,
        ),
        (
            "terminal.next_prompt",
            "Terminal: Next Prompt",
            NextPrompt,
            terminal,
        ),
    ] {
        registry
            .register(
                id,
                title,
                move |target: &mut Option<TerminalOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("terminal command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

/// The terminal command `run` is, in a context where it applies.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<TerminalOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}

#[cfg(test)]
impl TerminalPanel {
    /// Runs `shell` in place of the user's in the sessions started from
    /// now.
    pub(crate) fn set_shell(&mut self, shell: &str, args: &[&str]) {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self.shell = Some((shell.to_owned(), args));
    }

    /// The tabs the last frame drew, none without a strip: label and
    /// background.
    pub(crate) fn drawn_tabs(&self) -> &[(SharedString, gpui::Hsla)] {
        &self.drawn_tabs
    }

    /// Where tab `ix` was drawn in the last frame, as the strip scrolled.
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

#[cfg(test)]
#[path = "terminal_tabs_tests.rs"]
mod terminal_tabs_tests;

#[cfg(test)]
#[path = "terminal_find_tests.rs"]
mod terminal_find_tests;

#[cfg(test)]
#[path = "terminal_shell_integration_tests.rs"]
mod terminal_shell_integration_tests;
