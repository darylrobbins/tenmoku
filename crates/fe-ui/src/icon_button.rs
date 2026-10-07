use gpui::{
    AnyView, App, ClickEvent, CursorStyle, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px, svg,
};

use crate::{ClickHandler, Icon, UiTheme};

type TooltipBuilder = Box<dyn Fn(&mut Window, &mut App) -> AnyView>;

/// A 16 px icon in a 22 px square that runs a handler when clicked. A
/// disabled one draws its icon muted, has no hover and runs nothing.
#[derive(IntoElement)]
pub struct IconButton {
    id: SharedString,
    icon: Icon,
    disabled: bool,
    theme: UiTheme,
    on_click: Option<ClickHandler>,
    tooltip: Option<TooltipBuilder>,
}

impl IconButton {
    /// `id` names the button's element and its `icon-button:<id>` debug
    /// selector.
    pub fn new(id: impl Into<SharedString>, icon: Icon, theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            icon,
            disabled: false,
            theme: *theme,
            on_click: None,
            tooltip: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

impl RenderOnce for IconButton {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let color = if self.disabled {
            theme.muted_text
        } else {
            theme.icon
        };
        let id = self.id.clone();
        div()
            .id(ElementId::Name(self.id))
            .debug_selector(|| format!("icon-button:{id}"))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(22.))
            .rounded_sm()
            .child(
                svg()
                    .path(self.icon.path())
                    .size(px(16.))
                    .flex_none()
                    .text_color(color),
            )
            .when(!self.disabled, |button| {
                button
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(theme.row_hover))
                    .when_some(self.on_click, |button, on_click| button.on_click(on_click))
            })
            .when_some(self.tooltip, |button, tooltip| {
                button.tooltip(move |window, cx| tooltip(window, cx))
            })
    }
}
