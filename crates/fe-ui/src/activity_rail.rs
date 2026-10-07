use gpui::{
    AnyView, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, RenderOnce, ScrollHandle, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, svg,
};

use crate::{ClickHandler, Icon, UiTheme};

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
    theme: UiTheme,
    track: Option<ScrollHandle>,
}

impl ActivityRail {
    pub fn new(id: impl Into<ElementId>, theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            buttons: Vec::new(),
            theme: *theme,
            track: None,
        }
    }

    pub fn children(mut self, buttons: impl IntoIterator<Item = RailButton>) -> Self {
        self.buttons.extend(buttons);
        self
    }

    /// Tracks each button's laid-out bounds
    /// ([`ScrollHandle::bounds_for_item`]) in `handle`.
    pub fn track(mut self, handle: &ScrollHandle) -> Self {
        self.track = Some(handle.clone());
        self
    }
}

impl RenderOnce for ActivityRail {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // Drawn under the buttons, so the highlight's bar covers it.
        let line = div()
            .absolute()
            .top_0()
            .left_0()
            .h_full()
            .w(px(1.))
            .bg(self.theme.border);
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .w(RAIL_WIDTH)
            .h_full()
            .bg(self.theme.panel_background)
            .child(line)
            .child(
                div()
                    .id("buttons")
                    .flex()
                    .flex_col()
                    .size_full()
                    .when_some(self.track, |buttons, track| buttons.track_scroll(&track))
                    .children(self.buttons),
            )
    }
}

/// One of the rail's buttons: an icon centred in a square. Highlighted, its
/// icon is in `tab_active_text` with an `accent` bar on its left; else in
/// `icon`.
#[derive(IntoElement)]
pub struct RailButton {
    id: ElementId,
    icon: Icon,
    highlighted: bool,
    theme: UiTheme,
    on_click: Option<ClickHandler>,
    tooltip: Option<TooltipBuilder>,
}

impl RailButton {
    pub fn new(id: impl Into<ElementId>, icon: Icon, theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            icon,
            highlighted: false,
            theme: *theme,
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let color = if self.highlighted {
            theme.tab_active_text
        } else {
            theme.icon
        };
        let bar = self.highlighted.then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .h_full()
                .w(BAR_WIDTH)
                .bg(theme.accent)
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
            .hover(move |style| style.bg(theme.row_hover))
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
