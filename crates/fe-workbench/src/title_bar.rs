//! The unified macOS title bar the standalone app draws itself, which is
//! kiln-shell's [`kiln_shell::TitleBar`]: here, what it reads, the root
//! folder's name.

use std::path::Path;

use gpui::SharedString;

// The unified title bar's metrics and window options are kiln's.
pub use kiln_platform::window_options::{
    TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, TRAFFIC_LIGHTS, titlebar_options,
};

/// What the bar reads with no root folder.
const NO_ROOT: &str = fe_config::APP_NAME;

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

/// Records the title bar's requests of the window in [`REQUESTS`].
#[cfg(test)]
pub(crate) fn record_requests(cx: &mut gpui::App) {
    use kiln_shell::window_frame::{WindowRequest, intercept_window_requests};
    intercept_window_requests(cx, |request| {
        let request = match request {
            WindowRequest::Move => Request::Move,
            WindowRequest::TitlebarDoubleClick => Request::DoubleClick,
            _ => return,
        };
        REQUESTS.with_borrow_mut(|requests| requests.push(request));
    });
}

/// The root folder's name, or Tenmoku with no root.
pub(crate) fn name(root: Option<&Path>) -> SharedString {
    root.and_then(Path::file_name).map_or_else(
        || SharedString::from(NO_ROOT),
        |name| SharedString::from(name.to_string_lossy().into_owned()),
    )
}
