// SPDX-License-Identifier: LicenseRef-Proprietary

//! The window's frame where the platform asks the app to draw it: under
//! client-side decorations (GNOME on Wayland, which has no server-side
//! ones), the row at the top also serves as the title bar, with the
//! window's title, its buttons, a drag to move and a double click to
//! maximize, and the window's edges resize it. Under server-side
//! decorations the window manager draws all of that, and the row holds only
//! the in-window menu bar, if any.
//!
//! The moves, zooms and resizes go through [`request`], which a test can
//! [`intercept_window_requests`] (GPUI's test windows don't implement
//! them), and [`simulate_client_decorations`] stands in for a platform
//! that asks for client-side decorations.

use std::rc::Rc;

use gpui::{
    AnyElement, App, CursorStyle, Decorations, Global, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, Pixels, Point, RenderOnce, ResizeEdge, SharedString,
    StatefulInteractiveElement as _, Styled as _, Tiling, Window, div, prelude::FluentBuilder as _,
    px, svg,
};
use kiln_command::{ContextFrame, Invocation, Source};
use kiln_platform::commands::RunCommand;
use kiln_ui::{ActiveTheme as _, Icon, MENU_BAR_HEIGHT};

/// How far in from the window's edges a press resizes it.
const RESIZE_EDGE: Pixels = px(5.);

/// The window's buttons' width.
const BUTTON_WIDTH: Pixels = px(40.);

/// What the chrome asks of the window: what a press on a title bar, a
/// frame's drag area or a resize edge does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowRequest {
    /// Move the window with the mouse.
    Move,
    /// The platform's title bar double click (macOS: zoom or minimize, as
    /// the user's settings say).
    TitlebarDoubleClick,
    /// Maximize, or restore.
    Zoom,
    /// The window manager's menu, at a point in the window.
    ShowMenu(Point<Pixels>),
    /// Resize the window from an edge with the mouse.
    Resize(ResizeEdge),
}

type Interceptor = Rc<dyn Fn(WindowRequest)>;

/// What [`intercept_window_requests`] and [`simulate_client_decorations`]
/// set.
#[derive(Default)]
struct Overrides {
    interceptor: Option<Interceptor>,
    client_tiling: Option<Tiling>,
}

impl Global for Overrides {}

/// Sends every [`WindowRequest`] the chrome makes to `f` instead of the
/// window, from now on: for tests, whose platform's windows don't move or
/// resize.
pub fn intercept_window_requests(cx: &mut App, f: impl Fn(WindowRequest) + 'static) {
    cx.default_global::<Overrides>().interceptor = Some(Rc::new(f));
}

/// Has every window's chrome act as if the platform asked for client-side
/// decorations with `tiling`, or, with `None`, as the platform says: for
/// tests, whose platform's windows have only server-side ones.
pub fn simulate_client_decorations(cx: &mut App, tiling: Option<Tiling>) {
    cx.default_global::<Overrides>().client_tiling = tiling;
    cx.refresh_windows();
}

/// Asks `window` to do `request`, or tells the interceptor, if one is set.
pub fn request(request: WindowRequest, window: &mut Window, cx: &mut App) {
    if let Some(interceptor) = cx
        .try_global::<Overrides>()
        .and_then(|overrides| overrides.interceptor.clone())
    {
        interceptor(request);
        return;
    }
    match request {
        WindowRequest::Move => window.start_window_move(),
        WindowRequest::TitlebarDoubleClick => window.titlebar_double_click(),
        WindowRequest::Zoom => window.zoom_window(),
        WindowRequest::ShowMenu(position) => window.show_window_menu(position),
        WindowRequest::Resize(edge) => window.start_window_resize(edge),
    }
}

/// The tiling of the window's edges if the app draws its frame, or `None`
/// if the window manager does.
pub fn client_tiling(window: &Window, cx: &App) -> Option<Tiling> {
    if let Some(tiling) = cx
        .try_global::<Overrides>()
        .and_then(|overrides| overrides.client_tiling)
    {
        return Some(tiling);
    }
    match window.window_decorations() {
        Decorations::Client { tiling } => Some(tiling),
        Decorations::Server => None,
    }
}

/// The row above the app's content: the menu bar, if any, then the space
/// that, under client-side decorations, moves the window and shows its
/// title and buttons. The buttons run `window.minimize`, `window.zoom` and
/// `window.close` from the mouse, in the app's root key context.
#[derive(IntoElement)]
pub struct TopRow {
    menu_bar: Option<AnyElement>,
    title: SharedString,
    client: bool,
    context: Option<SharedString>,
}

impl TopRow {
    /// A row titled `title`, drawn as a frame if `client` (client-side
    /// decorations), else holding only the menu bar.
    pub fn new(title: impl Into<SharedString>, client: bool) -> Self {
        Self {
            menu_bar: None,
            title: title.into(),
            client,
            context: None,
        }
    }

    /// Puts `menu_bar` at the row's start.
    pub fn menu_bar(mut self, menu_bar: impl IntoElement) -> Self {
        self.menu_bar = Some(menu_bar.into_any_element());
        self
    }

    /// The app's root key context, which the buttons' commands run in.
    pub fn key_context(mut self, context: impl Into<SharedString>) -> Self {
        self.context = Some(context.into());
        self
    }
}

impl RenderOnce for TopRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let (muted, text, hover, error, on_error) = (
            colors.text.muted,
            colors.text.primary,
            colors.list.row_hover,
            colors.status.error,
            colors.tab.active_text,
        );
        let client = self.client;
        let drag = div()
            .id("window-drag")
            .debug_selector(|| "window-drag".into())
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(muted)
            .when(client, |drag| {
                drag.child(div().min_w_0().truncate().child(self.title))
                    .on_mouse_down(MouseButton::Left, |event, window, cx| {
                        let ask = if event.click_count == 2 {
                            WindowRequest::Zoom
                        } else {
                            WindowRequest::Move
                        };
                        request(ask, window, cx);
                    })
                    .on_mouse_down(MouseButton::Right, |event, window, cx| {
                        request(WindowRequest::ShowMenu(event.position), window, cx);
                    })
            });
        let controls = window.window_controls();
        let maximized = window.is_maximized();
        let context = self.context;
        let button = |id: &'static str, icon: Icon, close: bool, command: &'static str| {
            let context = context.clone();
            div()
                .id(id)
                .debug_selector(move || id.into())
                .flex()
                .items_center()
                .justify_center()
                .w(BUTTON_WIDTH)
                .h_full()
                .text_color(muted)
                .hover(move |style| {
                    if close {
                        style.bg(error).text_color(on_error)
                    } else {
                        style.bg(hover).text_color(text)
                    }
                })
                // Not a drag of the window.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| run(command, context.as_ref(), window, cx))
                .child(svg().path(icon.path()).size(px(14.)).text_color(muted))
        };
        let buttons = client.then(|| {
            div()
                .flex()
                .flex_row()
                .h_full()
                .when(controls.minimize, |row| {
                    row.child(button(
                        "window-minimize",
                        Icon::Minus,
                        false,
                        "window.minimize",
                    ))
                })
                .when(controls.maximize, |row| {
                    let icon = if maximized { Icon::Copy } else { Icon::Square };
                    row.child(button("window-maximize", icon, false, "window.zoom"))
                })
                .child(button("window-close", Icon::X, true, "window.close"))
        });
        div()
            .flex_none()
            .w_full()
            .h(MENU_BAR_HEIGHT)
            .flex()
            .flex_row()
            .items_center()
            .bg(colors.surface.panel)
            .border_b_1()
            .border_color(colors.border.default)
            .children(self.menu_bar)
            .child(drag)
            .children(buttons)
    }
}

/// Runs a window command as its menu item would, from the mouse, in
/// `context`.
fn run(command: &str, context: Option<&SharedString>, window: &mut Window, cx: &mut App) {
    let Ok(invocation) = Invocation::of(command) else {
        return;
    };
    let invocation = invocation.source(Source::Mouse).context(
        context
            .map(|context| ContextFrame::new(context.as_ref()))
            .into_iter()
            .collect(),
    );
    window.dispatch_action(Box::new(RunCommand(invocation)), cx);
}

/// The handles along the window's untiled edges and corners that resize
/// it, to lay over the whole window.
pub fn resize_handles(tiling: Tiling) -> impl Iterator<Item = AnyElement> {
    use ResizeEdge::*;
    let corner = RESIZE_EDGE * 2.;
    let edges = [
        (Top, !tiling.top),
        (Bottom, !tiling.bottom),
        (Left, !tiling.left),
        (Right, !tiling.right),
        (TopLeft, !tiling.top && !tiling.left),
        (TopRight, !tiling.top && !tiling.right),
        (BottomLeft, !tiling.bottom && !tiling.left),
        (BottomRight, !tiling.bottom && !tiling.right),
    ];
    edges
        .into_iter()
        .filter(|(_, shown)| *shown)
        .map(move |(edge, _)| {
            let handle = div().absolute().occlude();
            let (handle, cursor) = match edge {
                Top => (
                    handle.top_0().left_0().right_0().h(RESIZE_EDGE),
                    CursorStyle::ResizeUpDown,
                ),
                Bottom => (
                    handle.bottom_0().left_0().right_0().h(RESIZE_EDGE),
                    CursorStyle::ResizeUpDown,
                ),
                Left => (
                    handle.left_0().top_0().bottom_0().w(RESIZE_EDGE),
                    CursorStyle::ResizeLeftRight,
                ),
                Right => (
                    handle.right_0().top_0().bottom_0().w(RESIZE_EDGE),
                    CursorStyle::ResizeLeftRight,
                ),
                TopLeft => (
                    handle.top_0().left_0().size(corner),
                    CursorStyle::ResizeUpLeftDownRight,
                ),
                BottomRight => (
                    handle.bottom_0().right_0().size(corner),
                    CursorStyle::ResizeUpLeftDownRight,
                ),
                TopRight => (
                    handle.top_0().right_0().size(corner),
                    CursorStyle::ResizeUpRightDownLeft,
                ),
                BottomLeft => (
                    handle.bottom_0().left_0().size(corner),
                    CursorStyle::ResizeUpRightDownLeft,
                ),
            };
            let selector = format!("window-resize-{edge:?}");
            handle
                .debug_selector(move || selector)
                .cursor(cursor)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    request(WindowRequest::Resize(edge), window, cx);
                })
                .into_any_element()
        })
}
