// SPDX-License-Identifier: Apache-2.0

//! The unified title bar an app draws itself on macOS, where the window's
//! content runs under a transparent title bar (see
//! [`kiln_platform::window_options`]): the traffic lights inline, then the
//! app's left items, or else the title centred clear of the lights; the
//! app's right items at the far end; and the window draggable from the
//! bar's empty space.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Div, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _,
    RenderOnce, SharedString, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use kiln_platform::window_options::{TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET};
use kiln_ui::ActiveTheme as _;

use crate::window_frame::{WindowRequest, request};

/// What the bar draws in one of its slots, built afresh each time it
/// renders.
pub type TitleBarSlot = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

/// The app's items in the bar: `left` right after the traffic lights, in
/// place of the centred title, and `right` at the bar's far end. A press
/// on either slot is the items' own: it neither moves nor zooms the window.
#[derive(Clone, Default)]
pub struct TitleBarItems {
    pub left: Option<TitleBarSlot>,
    pub right: Option<TitleBarSlot>,
}

impl TitleBarItems {
    pub fn is_empty(&self) -> bool {
        self.left.is_none() && self.right.is_none()
    }
}

/// Right of the right items, the same gap as the lights keep from the
/// window's left edge, less the items' own padding.
const RIGHT_PAD: gpui::Pixels = px(8.);

/// The bar, titled `title`, on `surface.chrome`, with no line under it: the
/// row below (a tab strip) carries on the same band. A press on it moves
/// the window, and a double click does what the platform's title bar does.
#[derive(IntoElement)]
pub struct TitleBar {
    title: SharedString,
    items: TitleBarItems,
}

impl TitleBar {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            items: TitleBarItems::default(),
        }
    }

    /// The app's items: see [`TitleBarItems`].
    pub fn items(mut self, items: TitleBarItems) -> Self {
        self.items = items;
        self
    }
}

/// `slot`'s items, in a box a press stays in.
fn slot(name: &'static str, slot: &TitleBarSlot, window: &mut Window, cx: &mut App) -> Div {
    div()
        .debug_selector(move || name.into())
        .flex_none()
        .flex()
        .items_center()
        .h_full()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(slot(window, cx))
}

impl RenderOnce for TitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let bar = div()
            .id("title-bar")
            .debug_selector(|| "title-bar".into())
            .flex_none()
            .w_full()
            .h(TITLE_BAR_HEIGHT)
            .flex()
            .items_center()
            .bg(colors.surface.chrome)
            .text_color(colors.text.primary)
            .on_mouse_down(MouseButton::Left, |event, window, cx| {
                let ask = match event.click_count {
                    1 => WindowRequest::Move,
                    2 => WindowRequest::TitlebarDoubleClick,
                    _ => return,
                };
                request(ask, window, cx);
            });
        let title = div()
            .debug_selector(|| "title-bar-label".into())
            .min_w_0()
            .truncate()
            .child(self.title);
        let TitleBarItems { left, right } = self.items;
        let Some(left) = left else {
            // The title, centred clear of the lights on both sides; any
            // right items sit over its right margin.
            return bar
                .relative()
                .px(TRAFFIC_LIGHT_INSET)
                .justify_center()
                .child(title)
                .when_some(right, |bar, right| {
                    bar.child(
                        div()
                            .absolute()
                            .top_0()
                            .right(RIGHT_PAD)
                            .h_full()
                            .child(slot("title-bar-right", &right, window, cx)),
                    )
                });
        };
        bar.pl(TRAFFIC_LIGHT_INSET)
            .pr(RIGHT_PAD)
            // A long left item gives way first, clipped at the gap.
            .child(
                slot("title-bar-left", &left, window, cx)
                    .flex_shrink(1.)
                    .min_w_0()
                    .overflow_hidden(),
            )
            .child(div().flex_1().min_w_0())
            .when_some(right, |bar, right| {
                bar.child(slot("title-bar-right", &right, window, cx))
            })
    }
}
