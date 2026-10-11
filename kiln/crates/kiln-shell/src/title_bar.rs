// SPDX-License-Identifier: LicenseRef-Proprietary

//! The unified title bar an app draws itself on macOS, where the window's
//! content runs under a transparent title bar (see
//! [`kiln_platform::window_options`]): the traffic lights inline, the
//! title centred clear of them, and the window draggable from the bar's
//! empty space.

use gpui::{
    App, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div,
};
use kiln_platform::window_options::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET};
use kiln_ui::ActiveTheme as _;

use crate::window_frame::{WindowRequest, request};

/// The bar, titled `title`: a press on it moves the window, and a double
/// click does what the platform's title bar does.
#[derive(IntoElement)]
pub struct TitleBar {
    title: SharedString,
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

impl RenderOnce for TitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
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
            .bg(colors.surface.panel)
            .border_b_1()
            .border_color(colors.border.default)
            .text_color(colors.text.primary)
            .on_mouse_down(MouseButton::Left, |event, window, cx| {
                let ask = match event.click_count {
                    1 => WindowRequest::Move,
                    2 => WindowRequest::TitlebarDoubleClick,
                    _ => return,
                };
                request(ask, window, cx);
            })
            .child(
                div()
                    .debug_selector(|| "title-bar-label".into())
                    .min_w_0()
                    .truncate()
                    .child(self.title),
            )
    }
}
