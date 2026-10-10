// SPDX-License-Identifier: LicenseRef-Proprietary

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::UiTheme;

/// The palette's width, or the window's less a margin if that's narrower.
pub const PALETTE_WIDTH: gpui::Pixels = px(600.);

/// A palette over the window: a box centred near the top with an input, a
/// status line and a list. It fills its positioned parent, catching mouse
/// events only on the box.
#[derive(IntoElement)]
pub struct PaletteFrame {
    input: Option<AnyElement>,
    status: Option<SharedString>,
    list: Option<AnyElement>,
    theme: UiTheme,
}

impl PaletteFrame {
    pub fn new(theme: &UiTheme) -> Self {
        Self {
            input: None,
            status: None,
            list: None,
            theme: *theme,
        }
    }

    pub fn input(mut self, input: impl IntoElement) -> Self {
        self.input = Some(input.into_any_element());
        self
    }

    pub fn status(mut self, status: impl Into<SharedString>) -> Self {
        self.status = Some(status.into());
        self
    }

    pub fn list(mut self, list: impl IntoElement) -> Self {
        self.list = Some(list.into_any_element());
        self
    }
}

impl RenderOnce for PaletteFrame {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let palette = div()
            .occlude()
            .w(PALETTE_WIDTH)
            .max_w_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme.panel_background)
            .border_1()
            .border_color(theme.border)
            .rounded_md()
            .shadow_lg()
            .text_color(theme.text)
            .text_size(px(12.))
            .when_some(self.input, |palette, input| {
                palette.child(div().border_b_1().border_color(theme.border).child(input))
            })
            .when_some(self.status, |palette, status| {
                palette.child(
                    div()
                        .px_3()
                        .py_1()
                        .text_color(theme.muted_text)
                        .child(status),
                )
            })
            .when_some(self.list, |palette, list| palette.child(list));
        div()
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .px_4()
            .pt(px(48.))
            .child(palette)
    }
}
