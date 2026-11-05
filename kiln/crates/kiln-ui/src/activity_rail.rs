// SPDX-License-Identifier: Apache-2.0

use gpui::{
    AnyView, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, RenderOnce, ScrollHandle, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, svg,
};

use crate::{ActiveTheme as _, ClickHandler, Icon, UiTheme};

/// The activity rail's width, and each of its buttons' width and height.
pub const RAIL_WIDTH: Pixels = px(36.);

/// A button's icon, square.
const ICON_SIZE: Pixels = px(18.);
/// The highlighted button's bar, along the rail's left edge.
const BAR_WIDTH: Pixels = px(2.);

type TooltipBuilder = Box<dyn Fn(&mut Window, &mut App) -> AnyView>;

/// A full-height vertical strip of [`RailButton`]s, top to bottom, with a
/// 1 px line on its left.
#[derive(IntoElement)]
pub struct ActivityRail {
    id: ElementId,
    buttons: Vec<RailButton>,
    footer: Vec<RailButton>,
    track: Option<ScrollHandle>,
}

impl ActivityRail {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            buttons: Vec::new(),
            footer: Vec::new(),
            track: None,
        }
    }

    pub fn children(mut self, buttons: impl IntoIterator<Item = RailButton>) -> Self {
        self.buttons.extend(buttons);
        self
    }

    /// Buttons pinned to the rail's bottom, below `children`.
    pub fn footer(mut self, buttons: impl IntoIterator<Item = RailButton>) -> Self {
        self.footer.extend(buttons);
        self
    }

    /// Tracks each button's laid-out bounds
    /// ([`ScrollHandle::bounds_for_item`]) in `handle`. With a footer, the
    /// item after the children is the gap that pushes it down, then the
    /// footer's buttons.
    pub fn track(mut self, handle: &ScrollHandle) -> Self {
        self.track = Some(handle.clone());
        self
    }
}

impl RenderOnce for ActivityRail {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        // Drawn under the buttons, so the highlight's bar covers it.
        let line = div()
            .absolute()
            .top_0()
            .left_0()
            .h_full()
            .w(px(1.))
            .bg(colors.border.default);
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .w(RAIL_WIDTH)
            .h_full()
            .bg(colors.surface.panel)
            .child(line)
            .child(
                div()
                    .id("buttons")
                    .flex()
                    .flex_col()
                    .size_full()
                    .when_some(self.track, |buttons, track| buttons.track_scroll(&track))
                    .children(self.buttons)
                    .when(!self.footer.is_empty(), |buttons| {
                        buttons
                            .child(div().flex_1().min_h_0())
                            .children(self.footer)
                    }),
            )
    }
}

/// One of the rail's buttons: an icon centred in a square. Highlighted, its
/// icon is in `tab.active_text` with an `accent.base` bar on its left; else
/// in `list.icon`.
#[derive(IntoElement)]
pub struct RailButton {
    id: ElementId,
    icon: Icon,
    highlighted: bool,
    on_click: Option<ClickHandler>,
    tooltip: Option<TooltipBuilder>,
}

impl RailButton {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, icon: Icon, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            icon,
            highlighted: false,
            on_click: None,
            tooltip: None,
        }
    }

    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Shown on hover, built when it shows: see [`crate::Tooltip`].
    pub fn tooltip(mut self, build: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.tooltip = Some(Box::new(build));
        self
    }
}

impl RenderOnce for RailButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let color = if self.highlighted {
            colors.tab.active_text
        } else {
            colors.list.icon
        };
        let hover = colors.list.row_hover;
        let bar = self.highlighted.then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .h_full()
                .w(BAR_WIDTH)
                .bg(colors.accent.base)
        });
        div()
            .id(self.id)
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(RAIL_WIDTH)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .child(
                svg()
                    .path(self.icon.path())
                    .size(ICON_SIZE)
                    .flex_none()
                    .text_color(color),
            )
            .children(bar)
            .when_some(self.on_click, |button, on_click| button.on_click(on_click))
            .when_some(self.tooltip, |button, tooltip| {
                button.tooltip(move |window, cx| tooltip(window, cx))
            })
    }
}
