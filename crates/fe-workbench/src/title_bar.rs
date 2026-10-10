//! The unified macOS title bar the standalone app draws itself: the
//! window's traffic lights inline, the root folder's name centred, and the
//! window draggable from the bar's empty space.

use std::path::Path;

use fe_ui::UiTheme;
use gpui::{
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _,
    SharedString, Styled as _, Window, div,
};

// The unified title bar's metrics and window options are kiln's.
pub use kiln_platform::window_options::{
    TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, TRAFFIC_LIGHTS, titlebar_options,
};

/// What the bar reads with no root folder.
const NO_ROOT: &str = "fastedit";

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
