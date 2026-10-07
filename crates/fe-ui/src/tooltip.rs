use gpui::{
    Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::UiTheme;

/// A small label shown on hover, and a muted shortcut after it if there is
/// one, such as `Search ⇧⌘F`. Built by GPUI's `.tooltip(…)`, which shows it
/// after its default 500 ms delay.
pub struct Tooltip {
    label: SharedString,
    shortcut: Option<SharedString>,
    theme: UiTheme,
}

impl Tooltip {
    pub fn new(
        label: impl Into<SharedString>,
        shortcut: Option<impl Into<SharedString>>,
        theme: &UiTheme,
    ) -> Self {
        Self {
            label: label.into(),
            shortcut: shortcut.map(Into::into),
            theme: *theme,
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
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .flex()
            .gap_2()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(theme.panel_background)
            .border_1()
            .border_color(theme.border)
            .text_size(px(12.))
            .text_color(theme.tab_active_text)
            .child(self.label.clone())
            .when_some(self.shortcut.clone(), |tooltip, shortcut| {
                tooltip.child(div().text_color(theme.muted_text).child(shortcut))
            })
    }
}
