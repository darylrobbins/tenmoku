//! The window's frame where the platform asks the app to draw it: under
//! client-side decorations (GNOME on Wayland, which has no server-side
//! ones), the top row also serves as the title bar, with the root folder's
//! name, the window's buttons, a drag to move and a double click to
//! maximize, and the window's edges resize it. Under server-side
//! decorations the window manager draws all of that, and the row holds only
//! the menu bar.

use fe_command::Source;
use fe_ui::{Icon, MENU_BAR_HEIGHT, UiTheme};
use gpui::{
    AnyElement, App, CursorStyle, Decorations, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, ResizeEdge, SharedString, StatefulInteractiveElement as _, Styled as _,
    Tiling, Window, div, prelude::FluentBuilder as _, px, svg,
};

/// How far in from the window's edges a press resizes it.
const RESIZE_EDGE: gpui::Pixels = px(5.);

/// The window's buttons' width.
const BUTTON_WIDTH: gpui::Pixels = px(40.);

/// The tiling of the window's edges if the app draws its frame, or `None`
/// if the window manager does.
pub(crate) fn client_tiling(window: &Window) -> Option<Tiling> {
    match window.window_decorations() {
        Decorations::Client { tiling } => Some(tiling),
        Decorations::Server => None,
    }
}

/// The row above the panes: `menu_bar`, if any, then the space that moves
/// the window and, under client-side decorations, `title` and the buttons.
pub(crate) fn top_row(
    menu_bar: Option<AnyElement>,
    title: SharedString,
    client: bool,
    ui: &UiTheme,
    window: &Window,
) -> impl IntoElement {
    let ui = *ui;
    let drag = div()
        .id("window-drag")
        .debug_selector(|| "window-drag".into())
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(ui.muted_text)
        .when(client, |drag| {
            drag.child(div().min_w_0().truncate().child(title))
                .on_mouse_down(MouseButton::Left, |event, window, _| {
                    if event.click_count == 2 {
                        window.zoom_window();
                    } else {
                        window.start_window_move();
                    }
                })
                .on_mouse_down(MouseButton::Right, |event, window, _| {
                    window.show_window_menu(event.position);
                })
        });
    let controls = window.window_controls();
    let maximized = window.is_maximized();
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
                    &ui,
                    |window, cx| run("window.minimize", window, cx),
                ))
            })
            .when(controls.maximize, |row| {
                let icon = if maximized { Icon::Copy } else { Icon::Square };
                row.child(button("window-maximize", icon, false, &ui, |window, cx| {
                    run("window.zoom", window, cx)
                }))
            })
            .child(button("window-close", Icon::X, true, &ui, |window, cx| {
                run("window.close", window, cx)
            }))
    });
    div()
        .flex_none()
        .w_full()
        .h(MENU_BAR_HEIGHT)
        .flex()
        .flex_row()
        .items_center()
        .bg(ui.panel_background)
        .border_b_1()
        .border_color(ui.border)
        .children(menu_bar)
        .child(drag)
        .children(buttons)
}

fn button(
    id: &'static str,
    icon: Icon,
    close: bool,
    ui: &UiTheme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let ui = *ui;
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .items_center()
        .justify_center()
        .w(BUTTON_WIDTH)
        .h_full()
        .text_color(ui.muted_text)
        .hover(move |style| {
            if close {
                style.bg(ui.error_text).text_color(ui.tab_active_text)
            } else {
                style.bg(ui.row_hover).text_color(ui.text)
            }
        })
        // Not a drag of the window.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(
            svg()
                .path(icon.path())
                .size(px(14.))
                .text_color(ui.muted_text),
        )
}

/// Runs a window command as its menu item would, from the mouse.
fn run(command: &str, window: &mut Window, cx: &mut App) {
    crate::pane_toolbar::dispatch(command, Source::Mouse, window, cx);
}

/// The handles along the window's untiled edges and corners that resize
/// it, to lay over the whole window.
pub(crate) fn resize_handles(tiling: Tiling) -> impl Iterator<Item = AnyElement> {
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
            handle
                .cursor(cursor)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    window.start_window_resize(edge);
                })
                .into_any_element()
        })
}
