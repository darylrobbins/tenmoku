// SPDX-License-Identifier: Apache-2.0

//! [`Shell`]: a window's shell. Around the host's centre view it draws the
//! chrome ([`Root`](crate::Root)'s pieces: the unified title bar, the
//! in-window menu bar and the frame drawn under client-side decorations),
//! the docks on the centre's sides and below it, the status bar, and the
//! modal layer over everything, all under the app's root key context.

use std::rc::Rc;

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    StyleRefinement, Styled as _, Subscription, Tiling, Window, div, prelude::FluentBuilder as _,
};
use kiln_platform::{Capabilities, focus};
use kiln_ui::{ROW_HEIGHT, ui_theme};

use crate::{
    Closing, DockPosition, MenuBarView, ModalId, ModalLayer, ModalView, TitleBar, TitleBarItems,
    close, command_palette, dock,
    menu_bar::Open,
    pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH},
    window_frame::{self, TopRow},
};

/// The centre's focus, read when focus needs somewhere to go.
type CenterFocus = Rc<dyn Fn(&App) -> FocusHandle>;

/// A window's shell, one per window:
///
/// ```ignore
/// cx.open_window(options, |window, cx| {
///     let notes = cx.new(|cx| Notes::new(window, cx));
///     cx.new(|cx| {
///         let mut shell = Shell::new("notes", window, cx)
///             .platform_chrome(Capabilities::current(), cx);
///         shell.set_center(notes, cx);
///         shell.set_title("Notes", cx);
///         shell
///     })
/// })
/// ```
///
/// Its root element sets the app's root key context, which the frame's
/// buttons run their commands in as well: kiln names none of an app's
/// contexts itself. It handles `menu_bar.open`, opening the in-window menu
/// bar if it shows, and `command_palette.toggle`, showing the command
/// palette ([`command_palette`]) in its modal layer.
///
/// Its modal layer gives focus to the centre when a modal that has it
/// hides. The status bar is the host's own view, drawn in a row of
/// kiln-ui's `ROW_HEIGHT` at the bottom, from its cache unless
/// [`Shell::redraw_status_bar`] asks otherwise.
pub struct Shell {
    context: SharedString,
    title: SharedString,
    text_size: Option<Pixels>,
    title_bar: bool,
    title_bar_items: TitleBarItems,
    menu_bar: Option<Entity<MenuBarView>>,
    window_frame: bool,
    client_tiling: Option<Tiling>,
    center: Option<(AnyView, CenterFocus)>,
    docks: Vec<(DockPosition, AnyView)>,
    status_bar: Option<AnyView>,
    status_bar_visible: bool,
    redraw_status_bar: bool,
    modals: Entity<ModalLayer>,
    _center_released: Option<Subscription>,
}

impl Shell {
    /// A shell under the app's root key `context`, with no chrome, centre,
    /// docks or status bar yet. Making it has the window keep keys working
    /// when the focused element stops being drawn ([`focus::watch`]), and
    /// the platform's close button ask the window's items first
    /// ([`close::close_window`], over the items the host gave
    /// [`close::set_window_items`]). It draws in the active theme's colours
    /// ([`ui_theme`]), so it follows a theme change.
    pub fn new(
        context: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        focus::watch(window, cx);
        let id = window.window_handle().window_id();
        cx.on_release(move |_, cx| dock::forget_centre(id, cx))
            .detach();
        // The platform's close button asks the window's items first.
        window.on_window_should_close(cx, |window, cx| {
            let handle = window.window_handle();
            match close::close_window(handle, Some(window), cx) {
                Closing::Now => true,
                Closing::Refused => false,
                Closing::Later(closing) => {
                    closing.detach();
                    false
                }
            }
        });
        let shell = cx.weak_entity();
        let modals = cx.new(|_| {
            let mut layer = ModalLayer::new();
            layer.set_focus_fallback(Rc::new(move |_, cx| {
                shell.upgrade()?.read(cx).center_focus(cx)
            }));
            layer
        });
        Self {
            context: context.into(),
            title: SharedString::default(),
            text_size: None,
            title_bar: false,
            title_bar_items: TitleBarItems::default(),
            menu_bar: None,
            window_frame: false,
            client_tiling: None,
            center: None,
            docks: Vec::new(),
            status_bar: None,
            status_bar_visible: true,
            redraw_status_bar: false,
            modals,
            _center_released: None,
        }
    }

    /// Draws the chrome as on a platform with `capabilities`: the unified
    /// title bar where there is one (macOS), the menus in the window where
    /// there are no native ones (Linux), and the frame where the window
    /// manager leaves it to the app.
    pub fn platform_chrome(mut self, capabilities: Capabilities, cx: &mut Context<Self>) -> Self {
        self.title_bar = capabilities.unified_title_bar;
        self.menu_bar = (!capabilities.native_menus).then(|| cx.new(MenuBarView::new));
        self.window_frame = !capabilities.unified_title_bar;
        self
    }

    /// The text size the shell's content inherits, unless the host sets its
    /// own around it.
    pub fn text_size(mut self, size: Pixels) -> Self {
        self.text_size = Some(size);
        self
    }

    /// The app's root key context.
    pub fn key_context(&self) -> &SharedString {
        &self.context
    }

    /// The title the chrome shows, in the unified title bar or the frame.
    /// The window's own title stays the app's to set with
    /// `window.set_window_title`.
    pub fn set_title(&mut self, title: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.title = title.into();
        cx.notify();
    }

    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// Shows or hides the unified title bar, for a window whose title bar
    /// is transparent (macOS).
    pub fn set_title_bar(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.title_bar = shown;
        cx.notify();
    }

    /// The app's items in the unified title bar, in place of its centred
    /// title on the left: see [`TitleBarItems`].
    pub fn set_title_bar_items(&mut self, items: TitleBarItems, cx: &mut Context<Self>) {
        self.title_bar_items = items;
        cx.notify();
    }

    /// The menu bar drawn in the window, from the installed menus, or none.
    pub fn set_menu_bar(&mut self, bar: Option<Entity<MenuBarView>>, cx: &mut Context<Self>) {
        self.menu_bar = bar;
        cx.notify();
    }

    pub fn menu_bar(&self) -> Option<&Entity<MenuBarView>> {
        self.menu_bar.as_ref()
    }

    /// Whether the shell draws the window's frame (title, buttons, resize
    /// edges) whenever the platform asks the app to.
    pub fn set_window_frame(&mut self, drawn: bool, cx: &mut Context<Self>) {
        self.window_frame = drawn;
        cx.notify();
    }

    /// Draws as if the platform asked for client-side decorations with
    /// `tiling`, or as it asks with `None`: for tests, whose windows only
    /// have server-side ones. It takes effect as the shell next draws.
    pub fn set_client_tiling(&mut self, tiling: Option<Tiling>) {
        self.client_tiling = tiling;
    }

    /// The host's view in the middle, which gets focus when a modal or a
    /// hidden panel that had it goes.
    pub fn set_center<V: Render + Focusable>(&mut self, center: Entity<V>, cx: &mut Context<Self>) {
        let weak = center.downgrade();
        let focus: CenterFocus = Rc::new(move |cx| match weak.upgrade() {
            Some(center) => center.focus_handle(cx),
            None => cx.focus_handle(),
        });
        self._center_released = Some(cx.observe_release(&center, |shell, _, cx| {
            shell.center = None;
            cx.notify();
        }));
        self.center = Some((center.into(), focus));
        cx.notify();
    }

    /// The centre view, if set.
    pub fn center(&self) -> Option<&AnyView> {
        self.center.as_ref().map(|(view, _)| view)
    }

    /// The centre's focus, if there's a centre.
    pub fn center_focus(&self, cx: &App) -> Option<FocusHandle> {
        self.center.as_ref().map(|(_, focus)| focus(cx))
    }

    /// Puts `dock` at `position`, in place of any there, or removes the
    /// one there with `None`. A dock is any view, such as a [`Dock`](crate::Dock).
    pub fn set_dock(
        &mut self,
        position: DockPosition,
        dock: Option<AnyView>,
        cx: &mut Context<Self>,
    ) {
        self.docks.retain(|(at, _)| *at != position);
        if let Some(dock) = dock {
            self.docks.push((position, dock));
        }
        cx.notify();
    }

    /// The dock at `position`, if any.
    pub fn dock(&self, position: DockPosition) -> Option<&AnyView> {
        self.docks
            .iter()
            .find(|(at, _)| *at == position)
            .map(|(_, dock)| dock)
    }

    /// The host's status bar view, or none.
    pub fn set_status_bar(&mut self, bar: Option<AnyView>, cx: &mut Context<Self>) {
        self.status_bar = bar;
        cx.notify();
    }

    /// Shows or hides the status bar.
    pub fn set_status_bar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if visible != self.status_bar_visible {
            self.status_bar_visible = visible;
            cx.notify();
        }
    }

    pub fn status_bar_visible(&self) -> bool {
        self.status_bar_visible
    }

    /// Draws the status bar afresh in the frame being drawn, rather than
    /// from its cache: for a host that gave the bar new content while
    /// drawing, which a notification can't bring into the frame. Call it as
    /// the frame draws, before the shell does.
    pub fn redraw_status_bar(&mut self) {
        self.redraw_status_bar = true;
    }

    /// The modal layer.
    pub fn modals(&self) -> &Entity<ModalLayer> {
        &self.modals
    }

    /// Shows `modal` as `id` over the window, dismissing the one showing,
    /// if any: see [`ModalLayer::show_modal`].
    pub fn show_modal<M: ModalView>(
        &mut self,
        id: ModalId,
        modal: Entity<M>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        (self.modals.clone()).update(cx, |layer, cx| layer.show_modal(id, modal, window, cx));
    }

    /// The modal showing, if any.
    pub fn modal(&self, cx: &App) -> Option<ModalId> {
        self.modals.read(cx).modal()
    }

    /// Dismisses the modal showing, as Escape would.
    pub fn dismiss_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.modals.clone()).update(cx, |layer, cx| layer.dismiss_modal(window, cx));
    }

    fn toggle_command_palette(
        &mut self,
        _: &command_palette::Toggle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        command_palette::toggle(self, window, cx);
    }

    fn open_menu_bar(&mut self, _: &Open, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(bar) = &self.menu_bar {
            bar.update(cx, |bar, cx| bar.open(0, true, window, cx));
        }
    }

    fn docks_at(&self, position: DockPosition) -> impl Iterator<Item = AnyView> + '_ {
        self.docks
            .iter()
            .filter(move |(at, _)| *at == position)
            .map(|(_, dock)| dock.clone())
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = ui_theme(cx);
        let tiling = self
            .client_tiling
            .or_else(|| window_frame::client_tiling(window, cx))
            .filter(|_| self.window_frame);
        let top_row = (self.menu_bar.is_some() || tiling.is_some()).then(|| {
            let row =
                TopRow::new(self.title.clone(), tiling.is_some()).key_context(self.context.clone());
            match &self.menu_bar {
                Some(bar) => row.menu_bar(bar.clone()),
                None => row,
            }
        });
        // The centre keeps its least: the docks shrink first.
        let main = div()
            .flex_1()
            .min_h(MIN_PANE_HEIGHT)
            .w_full()
            .flex()
            .children(self.docks_at(DockPosition::Left))
            // Kept its least beside the docks, and measured for their
            // limits.
            // The bottom docks under the centre only: the side docks run
            // the main area's full height.
            .child(
                div()
                    .flex_1()
                    .min_w(MIN_PANE_WIDTH)
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .children(self.center().cloned().map(|center| {
                        div()
                            .relative()
                            .flex_1()
                            .min_h(MIN_PANE_HEIGHT)
                            .flex()
                            .child(center)
                            .child(dock::track_centre())
                    }))
                    .children(self.docks_at(DockPosition::Bottom)),
            )
            .children(self.docks_at(DockPosition::Right));
        // Cached: frames the shell redraws for other reasons reuse the
        // bar's last render, unless the host asked for it afresh.
        let mut style = StyleRefinement::default().w_full().h(ROW_HEIGHT);
        style.flex_shrink = Some(0.);
        let fresh = std::mem::take(&mut self.redraw_status_bar);
        let status_bar = self
            .status_bar
            .clone()
            .filter(|_| self.status_bar_visible)
            .map(|bar| {
                if fresh {
                    bar.into_any_element()
                } else {
                    bar.cached(style).into_any_element()
                }
            });
        div()
            .key_context(self.context.as_ref())
            .on_action(cx.listener(Self::open_menu_bar))
            .on_action(cx.listener(Self::toggle_command_palette))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(ui.background)
            .text_color(ui.text)
            .when_some(self.text_size, |shell, size| shell.text_size(size))
            .when(self.title_bar, |shell| {
                shell.child(TitleBar::new(self.title.clone()).items(self.title_bar_items.clone()))
            })
            .children(top_row)
            .child(main)
            .children(status_bar)
            .child(self.modals.clone())
            // The frame's border and resize edges, over everything, unless
            // the window fills the screen.
            .when_some(tiling.filter(|tiling| !tiling.is_tiled()), |shell, _| {
                shell.border_1().border_color(ui.border)
            })
            .children(tiling.into_iter().flat_map(window_frame::resize_handles))
    }
}
