// SPDX-License-Identifier: Apache-2.0

use gpui::{
    Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, UiTheme};

/// A small label shown on hover, and a muted shortcut after it if there is
/// one, such as `Search ⇧⌘F`. Built by GPUI's `.tooltip(…)`, which shows it
/// after its default 500 ms delay.
pub struct Tooltip {
    label: SharedString,
    shortcut: Option<SharedString>,
}

impl Tooltip {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(
        label: impl Into<SharedString>,
        shortcut: Option<impl Into<SharedString>>,
        _theme: &UiTheme,
    ) -> Self {
        Self {
            label: label.into(),
            shortcut: shortcut.map(Into::into),
        }
    }

    /// The label, then the shortcut after a space if there is one.
    pub fn text(&self) -> String {
        match &self.shortcut {
            Some(shortcut) => format!("{} {shortcut}", self.label),
            None => self.label.to_string(),
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let muted = colors.text.muted;
        div()
            .flex()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(colors.surface.panel)
            .border_1()
            .border_color(colors.border.default)
            .text_size(px(12.))
            .text_color(colors.tab.active_text)
            .child(self.label.clone())
            .when_some(self.shortcut.clone(), |tooltip, shortcut| {
                tooltip.child(div().text_color(muted).child(shortcut))
            })
    }
}
