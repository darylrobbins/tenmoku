// SPDX-License-Identifier: LicenseRef-Proprietary

//! The active theme's tokens, group by group, as labelled swatches.

use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px};
use kiln::{
    core::theme::{TOKEN_GROUPS, TOKEN_KEYS},
    prelude::*,
    ui::{Appearance, h_flex, v_flex},
};

use super::{page_frame, page_root, section};

pub(crate) struct ThemePage;

impl Render for ThemePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let c = theme.colors;
        let groups = TOKEN_GROUPS.iter().map(|group| {
            let prefix = format!("{group}.");
            let swatches = TOKEN_KEYS
                .iter()
                .filter(|key| key.starts_with(&prefix))
                .map(|key| {
                    let color = theme.colors.get(key).copied().unwrap_or_default();
                    let written = theme
                        .data()
                        .tokens
                        .get(key)
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    v_flex()
                        .w(px(150.))
                        .gap_1()
                        .child(
                            div()
                                .h(px(40.))
                                .rounded_md()
                                .border_1()
                                .border_color(c.border.default)
                                .bg(color),
                        )
                        .child(div().text_color(c.text.primary).child(key.to_string()))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(c.text.muted)
                                .child(written),
                        )
                });
            section(
                group.to_uppercase(),
                h_flex().flex_wrap().gap_3().children(swatches),
                cx,
            )
        });
        let appearance = match theme.appearance {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        };
        let blurb = format!(
            "“{}”, a {appearance} theme: every token, as cx.theme().colors has it.",
            theme.name
        );
        let groups: Vec<_> = groups.collect();
        page_root("theme-page", cx).child(page_frame("Theme", &blurb, cx).children(groups))
    }
}
