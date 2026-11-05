// SPDX-License-Identifier: Apache-2.0

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, UiTheme};

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
}

impl PaletteFrame {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(_theme: &UiTheme) -> Self {
        Self {
            input: None,
            status: None,
            list: None,
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
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let (border, muted) = (colors.border.default, colors.text.muted);
        let palette = div()
            .occlude()
            .w(PALETTE_WIDTH)
            .max_w_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(colors.surface.panel)
            .border_1()
            .border_color(border)
            .rounded_md()
            .shadow_lg()
            .text_color(colors.text.primary)
            .text_size(px(12.))
            .when_some(self.input, |palette, input| {
                palette.child(div().border_b_1().border_color(border).child(input))
            })
            .when_some(self.status, |palette, status| {
                palette.child(div().px_3().py_1().text_color(muted).child(status))
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
