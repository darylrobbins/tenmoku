//! The unified macOS title bar the standalone app draws itself: the
//! window's traffic lights inline, the root folder's name centred, and the
//! window draggable from the bar's empty space.

use std::path::Path;

use fe_ui::UiTheme;
use gpui::{
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels,
    Point, SharedString, Styled as _, TitlebarOptions, Window, div, point, px,
};

/// The bar's height.
pub const TITLE_BAR_HEIGHT: Pixels = px(32.);

/// Where the traffic lights sit, from the window's top-left corner: 9 px
/// down centres the 14 px buttons in the 32 px bar.
pub const TRAFFIC_LIGHTS: Point<Pixels> = point(px(12.), px(9.));

/// The width the traffic lights take, plus a gap: the label stays out of it
/// on both sides.
pub const TRAFFIC_LIGHT_INSET: Pixels = px(78.);

/// What the bar reads with no root folder.
const NO_ROOT: &str = "fastedit";

/// The window options for a window whose content draws under its title bar.
/// A host opting in must also set `WindowOptions::app_owns_titlebar_drag`
/// to `true`, and call `Workbench::set_title_bar(true)`.
pub fn titlebar_options(title: impl Into<SharedString>) -> TitlebarOptions {
    TitlebarOptions {
        title: Some(title.into()),
        appears_transparent: true,
        traffic_light_position: Some(TRAFFIC_LIGHTS),
    }
}

/// What a mouse down on the bar asked the window to do, under test, where
/// the platform's window move is unimplemented.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Move,
    DoubleClick,
}

#[cfg(test)]
thread_local! {
    pub(crate) static REQUESTS: std::cell::RefCell<Vec<Request>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn on_mouse_down(event: &MouseDownEvent, window: &mut Window) {
    match event.click_count {
        1 => {
            #[cfg(test)]
            REQUESTS.with_borrow_mut(|requests| requests.push(Request::Move));
            #[cfg(not(test))]
            window.start_window_move();
        }
        2 => {
            #[cfg(test)]
            REQUESTS.with_borrow_mut(|requests| requests.push(Request::DoubleClick));
            #[cfg(not(test))]
            window.titlebar_double_click();
        }
        _ => {}
    }
    #[cfg(test)]
    let _ = window;
}

/// The root folder's name, or `fastedit` with no root.
pub(crate) fn name(root: Option<&Path>) -> SharedString {
    root.and_then(Path::file_name).map_or_else(
        || SharedString::from(NO_ROOT),
        |name| SharedString::from(name.to_string_lossy().into_owned()),
    )
}

/// The bar, titled with `root`'s folder name.
pub(crate) fn title_bar(root: Option<&Path>, ui: &UiTheme) -> impl IntoElement {
    let name = name(root);
    div()
        .id("title-bar")
        .debug_selector(|| "title-bar".into())
        .flex_none()
        .w_full()
        .h(TITLE_BAR_HEIGHT)
        .px(TRAFFIC_LIGHT_INSET)
        .flex()
        .items_center()
        .justify_center()
        .bg(ui.panel_background)
        .border_b_1()
        .border_color(ui.border)
        .text_color(ui.text)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            on_mouse_down(event, window)
        })
        .child(
            div()
                .debug_selector(|| "title-bar-label".into())
                .min_w_0()
                .truncate()
                .child(name),
        )
}
