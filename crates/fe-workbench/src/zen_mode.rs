//! Zen Mode (`workbench.toggle_zen_mode`, VS Code's
//! `workbench.action.toggleZenMode`): the editors alone. Turning it on
//! notes what of the window's chrome shows, then hides the sidebar (its
//! References and Problems too), the terminal's dock and the activity
//! rail, and, as `[workbench.zen_mode]` says, the status bar and the panes'
//! tab strips with their toolbars, and centers the panes with margins of a
//! fifth of the window's width on each side, VS Code's centered layout.
//! While it's on the panes sit in a `zen_mode` key context frame. Turning
//! it off puts back what it hid, but a dock or the status bar shown or
//! hidden meanwhile stays as it is.

use fe_config::settings::ZenModeSettings;
use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _, WeakEntity,
    Window, div, prelude::FluentBuilder as _,
};
use kiln_shell::{DockView as _, PaneView as _, Workspace};

use super::Workbench;
use crate::Pane;

/// The key context frame the panes sit in while Zen Mode is on.
pub(crate) const KEY_CONTEXT: &str = "zen_mode";

/// How much of the window's width each side's margin takes in a centered
/// layout: VS Code's default ratio.
pub(crate) const MARGIN: f32 = 0.2;

/// A window's Zen Mode.
pub(crate) struct ZenMode {
    /// While it's on: the chrome as it was, and as it left it.
    on: Option<Noted>,
    /// Whether the panes leave their tab strips out.
    tabs_hidden: bool,
    /// The shell's centre, wrapped the first time it turns on.
    center: Option<Entity<ZenCenter>>,
    /// The window's `[workbench.zen_mode]`, as last given.
    settings: ZenModeSettings,
}

impl ZenMode {
    /// Off, with the settings in use now, until the window's own come
    /// ([`Workbench::follow_zen_settings`]).
    pub(crate) fn new(cx: &App) -> Self {
        let settings = (cx.try_global::<kiln::GlobalSettings<fe_config::Settings>>())
            .map(|global| global.settings().workbench.zen_mode.clone())
            .unwrap_or_default();
        Self {
            on: None,
            tabs_hidden: false,
            center: None,
            settings,
        }
    }
}

/// The chrome turning Zen Mode off puts back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Chrome {
    sidebar: bool,
    terminal: bool,
    status_bar: bool,
}

#[derive(Clone, Copy)]
struct Noted {
    before: Chrome,
    /// The chrome as last seen, Zen Mode's own changes included.
    seen: Chrome,
    /// Which pieces something other than Zen Mode showed or hid since it
    /// turned on, even if back as they were: those stay as they are.
    changed: Chrome,
}

impl Noted {
    /// Notes the chrome `now`, and which pieces changed since last seen.
    fn see(&mut self, now: Chrome) {
        self.changed.sidebar |= now.sidebar != self.seen.sidebar;
        self.changed.terminal |= now.terminal != self.seen.terminal;
        self.changed.status_bar |= now.status_bar != self.seen.status_bar;
        self.seen = now;
    }

    /// What turning it off leaves, given the chrome `now`: each piece as it
    /// was before, unless something else changed it meanwhile.
    fn restored(&self, now: Chrome) -> Chrome {
        let pick = |before, changed, now| if changed { now } else { before };
        Chrome {
            sidebar: pick(self.before.sidebar, self.changed.sidebar, now.sidebar),
            terminal: pick(self.before.terminal, self.changed.terminal, now.terminal),
            status_bar: pick(
                self.before.status_bar,
                self.changed.status_bar,
                now.status_bar,
            ),
        }
    }
}

impl Workbench {
    /// Whether Zen Mode is on.
    pub fn zen_mode(&self) -> bool {
        self.zen.on.is_some()
    }

    /// `workbench.toggle_zen_mode`: turns Zen Mode on, or off.
    pub(crate) fn toggle_zen_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.zen_mode() {
            self.leave_zen_mode(cx);
        } else {
            self.enter_zen_mode(window, cx);
        }
    }

    fn enter_zen_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.zen.settings.clone();
        let before = self.chrome(cx);
        let hid = Chrome {
            sidebar: false,
            terminal: false,
            status_bar: before.status_bar && !settings.hide_status_bar,
        };
        let unchanged = Chrome {
            sidebar: false,
            terminal: false,
            status_bar: false,
        };
        // Seen as hidden before it hides them, so its own changes aren't
        // taken for another's.
        self.zen.on = Some(Noted {
            before,
            seen: hid,
            changed: unchanged,
        });
        self.layout_keys.zen_sidebar.set(Some(before.sidebar));
        // Focus on a dock goes to the active editor: on the bottom panel's
        // tool windows here, the sidebar's below.
        if self.bottom_panel_focused(window, cx) {
            window.focus(&self.focus_handle(cx), cx);
        }
        self.set_chrome(hid, cx);
        self.leave_hidden_panel(window, cx);
        self.set_rail_hidden(true, cx);
        self.zen.tabs_hidden = settings.hide_tabs;
        for pane in self.panes(cx) {
            self.follow_zen_tabs(&pane, cx);
        }
        let center = self.zen_center(cx);
        center.update(cx, |center, cx| {
            center.on = true;
            center.centered = settings.center_layout;
            cx.notify();
        });
    }

    fn leave_zen_mode(&mut self, cx: &mut Context<Self>) {
        let Some(noted) = self.zen.on.take() else {
            return;
        };
        self.layout_keys.zen_sidebar.set(None);
        let mut restored = noted.restored(self.chrome(cx));
        // Not a terminal whose shell exited meanwhile, which closes its
        // tab; Problems or the Git log shown in the bottom panel come back.
        let bottom = self.terminal_dock.read(cx).dock().active();
        restored.terminal &= bottom != Some(crate::terminal_panel::TERMINAL)
            || self.terminal.read(cx).view().is_some();
        self.set_chrome(restored, cx);
        self.set_rail_hidden(false, cx);
        self.zen.tabs_hidden = false;
        for pane in self.panes(cx) {
            self.follow_zen_tabs(&pane, cx);
        }
        if let Some(center) = &self.zen.center {
            center.update(cx, |center, cx| {
                center.on = false;
                cx.notify();
            });
        }
    }

    /// While Zen Mode is on, notes which docks changed, as either dock
    /// notifies: a sidebar changed is saved in the layout as it is.
    pub(super) fn note_zen_chrome(&mut self, cx: &App) {
        if self.zen.on.is_none() {
            return;
        }
        let now = self.chrome(cx);
        let Some(noted) = &mut self.zen.on else {
            return;
        };
        noted.see(now);
        if noted.changed.sidebar {
            self.layout_keys.zen_sidebar.set(None);
        }
    }

    /// While Zen Mode is on, notes the status bar shown or hidden by
    /// something else ([`Workbench::set_status_bar_visible`]: its toggle, a
    /// settings reload), even to what it shows now: it stays so.
    pub(super) fn note_zen_status_bar(&mut self) {
        let visible = self.status_bar_visible;
        if let Some(noted) = &mut self.zen.on {
            noted.changed.status_bar = true;
            noted.seen.status_bar = visible;
        }
    }

    /// Takes the window's `[workbench.zen_mode]`, for the next time Zen
    /// Mode turns on: from [`Workbench::reload_settings`].
    pub(super) fn follow_zen_settings(&mut self, settings: &ZenModeSettings) {
        self.zen.settings = settings.clone();
    }

    fn chrome(&self, cx: &App) -> Chrome {
        Chrome {
            sidebar: self.dock.read(cx).is_visible(),
            terminal: self.terminal_dock.read(cx).is_visible(),
            status_bar: self.status_bar_visible,
        }
    }

    fn set_chrome(&mut self, chrome: Chrome, cx: &mut Context<Self>) {
        self.dock.update(cx, |dock, cx| {
            dock.dock_mut().set_visible(chrome.sidebar);
            cx.notify();
        });
        self.terminal_dock.update(cx, |dock, cx| {
            dock.dock_mut().set_visible(chrome.terminal);
            cx.notify();
        });
        // Not through `set_status_bar_visible`, which notes another's.
        if chrome.status_bar != self.status_bar_visible {
            self.status_bar_visible = chrome.status_bar;
            self.show_status_bar(cx);
            cx.notify();
        }
    }

    fn set_rail_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        self.dock.update(cx, |dock, cx| {
            dock.dock_mut().hide_rail(hidden);
            cx.notify();
        });
    }

    /// Has `pane` show its tab strip, or leave it out in Zen Mode: for
    /// every pane as it turns on or off, and for one that joins meanwhile.
    pub(super) fn follow_zen_tabs(&self, pane: &Entity<Pane>, cx: &mut App) {
        let hidden = self.zen.tabs_hidden;
        pane.update(cx, |pane, cx| {
            if pane.pane().tab_bar_hidden() != hidden {
                pane.pane_mut().set_tab_bar_hidden(hidden);
                cx.notify();
            }
        });
    }

    /// The shell's centre, wrapped in a [`ZenCenter`] the first time.
    fn zen_center(&mut self, cx: &mut Context<Self>) -> Entity<ZenCenter> {
        if let Some(center) = &self.zen.center {
            return center.clone();
        }
        let inner = (self.shell.read(cx).center().cloned()).expect("the workspace's centre");
        let workspace = self.workspace.downgrade();
        let center = cx.new(|_| ZenCenter {
            inner,
            workspace,
            on: false,
            centered: false,
        });
        (self.shell).update(cx, |shell, cx| shell.set_center(center.clone(), cx));
        self.zen.center = Some(center.clone());
        center
    }
}

/// The shell's centre, the workspace's panes, in Zen Mode's key context
/// while it's on, with margins of [`MARGIN`] of the window's width on each
/// side while it centers them.
pub(crate) struct ZenCenter {
    inner: AnyView,
    workspace: WeakEntity<Workspace<Pane>>,
    on: bool,
    centered: bool,
}

impl Render for ZenCenter {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let margin = window.viewport_size().width * MARGIN;
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .when(self.on, |center| center.key_context(KEY_CONTEXT))
            .when(self.on && self.centered, |center| {
                center.pl(margin).pr(margin)
            })
            .child(self.inner.clone())
    }
}

impl Focusable for ZenCenter {
    /// The workspace's, as the centre it wraps has.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.workspace.upgrade() {
            Some(workspace) => workspace.focus_handle(cx),
            None => cx.focus_handle(),
        }
    }
}
