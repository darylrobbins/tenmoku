// SPDX-License-Identifier: LicenseRef-Proprietary

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, ClickHandler, UiTheme};

/// A small button that is on or off, such as a search option: its label
/// accented on a highlighted background when on, muted when off.
#[derive(IntoElement)]
pub struct ToggleButton {
    id: ElementId,
    label: SharedString,
    on: bool,
    on_click: Option<ClickHandler>,
}

impl ToggleButton {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            on: false,
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
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let hover = colors.list.row_hover;
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
                        .bg(colors.list.row_selected)
                        .text_color(colors.tab.active_text)
                } else {
                    button
                        .text_color(colors.text.muted)
                        .hover(move |style| style.bg(hover))
                }
            })
            .child(self.label)
            .when_some(self.on_click, |button, on_click| button.on_click(on_click))
    }
}
