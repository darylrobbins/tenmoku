// SPDX-License-Identifier: LicenseRef-Proprietary

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{ClickHandler, UiTheme};

/// A small button that is on or off, such as a search option: its label
/// accented on a highlighted background when on, muted when off.
#[derive(IntoElement)]
pub struct ToggleButton {
    id: ElementId,
    label: SharedString,
    on: bool,
    theme: UiTheme,
    on_click: Option<ClickHandler>,
}

impl ToggleButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            on: false,
            theme: *theme,
            on_click: None,
        }
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ToggleButton {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(20.))
            .min_w(px(22.))
            .px_1()
            .rounded_sm()
            .map(|button| {
                if self.on {
                    button
                        .bg(theme.row_selected)
                        .text_color(theme.tab_active_text)
                } else {
                    button
                        .text_color(theme.muted_text)
                        .hover(move |style| style.bg(theme.row_hover))
                }
            })
            .child(self.label)
            .when_some(self.on_click, |button, on_click| button.on_click(on_click))
    }
}
