// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery's pages: one view each, and the order the sidebar lists
//! them in.

mod chrome;
mod controls;
mod layout;
mod lists;
mod prompts;
mod text;
mod theme;

use gpui::{
    App, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div,
    px,
};
use kiln::{
    prelude::*,
    ui::{ScrollArea, h_flex, v_flex},
};

pub(crate) use chrome::ChromePage;
pub(crate) use controls::ControlsPage;
pub(crate) use layout::LayoutPage;
pub(crate) use lists::ListsPage;
pub(crate) use prompts::PromptsPage;
pub(crate) use text::TextPage;
pub(crate) use theme::ThemePage;

/// A page of the gallery, in sidebar order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Page {
    #[default]
    Controls,
    Text,
    Lists,
    Layout,
    Chrome,
    Prompts,
    Theme,
}

impl Page {
    pub(crate) const ALL: &[Page] = &[
        Page::Controls,
        Page::Text,
        Page::Lists,
        Page::Layout,
        Page::Chrome,
        Page::Prompts,
        Page::Theme,
    ];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Page::Controls => "Controls",
            Page::Text => "Text",
            Page::Lists => "Lists and trees",
            Page::Layout => "Layout",
            Page::Chrome => "Chrome",
            Page::Prompts => "Prompts",
            Page::Theme => "Theme",
        }
    }

    /// Its place in [`Page::ALL`].
    pub(crate) fn index(self) -> usize {
        Page::ALL.iter().position(|page| *page == self).unwrap_or(0)
    }

    /// The page after this one, wrapping round to the first.
    pub(crate) fn next(self) -> Page {
        Page::ALL[(self.index() + 1) % Page::ALL.len()]
    }

    /// The page before this one, wrapping round to the last.
    pub(crate) fn previous(self) -> Page {
        Page::ALL[(self.index() + Page::ALL.len() - 1) % Page::ALL.len()]
    }
}

/// A titled block of a page: a small heading, then its content.
pub(crate) fn section(
    title: impl Into<SharedString>,
    content: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    let c = &cx.theme().colors;
    v_flex()
        .gap_2()
        .child(
            div()
                .text_size(px(11.))
                .text_color(c.text.muted)
                .child(title.into()),
        )
        .child(content)
}

/// A row of labelled content: the label in a fixed-width column.
pub(crate) fn labelled(
    label: impl Into<SharedString>,
    content: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    h_flex()
        .gap_3()
        .items_center()
        .child(
            div()
                .w(px(96.))
                .flex_none()
                .text_color(cx.theme().colors.text.muted)
                .child(label.into()),
        )
        .child(content)
}

/// A page's root: it scrolls, on the window's background. The sidebar
/// beside it is on the panel colour.
pub(crate) fn page_root(id: &'static str, cx: &App) -> ScrollArea {
    ScrollArea::new(id)
        .size_full()
        .bg(cx.theme().colors.surface.background)
}

/// A page's frame: its title and a line saying what it shows, then its
/// content.
pub(crate) fn page_frame(title: &str, blurb: &str, cx: &App) -> gpui::Div {
    let c = &cx.theme().colors;
    v_flex().p_5().gap_5().child(
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_size(px(18.))
                    .text_color(c.text.primary)
                    .child(title.to_owned()),
            )
            .child(div().text_color(c.text.muted).child(blurb.to_owned())),
    )
}

/// A plain labelled panel, for the views that containers such as
/// `SplitPane` and `Tabs` hold: they take views, not elements.
pub(crate) struct Placeholder {
    label: SharedString,
}

impl Placeholder {
    pub(crate) fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl Render for Placeholder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = &cx.theme().colors;
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(c.surface.panel)
            .text_color(c.text.muted)
            .child(self.label.clone())
    }
}
