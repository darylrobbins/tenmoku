// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery's pages: one view each, and the order the sidebar lists
//! them in.

mod controls;
mod lists;
mod text;

use gpui::{App, IntoElement, ParentElement as _, SharedString, Styled as _, div, px};
use kiln::{
    prelude::*,
    ui::{h_flex, v_flex},
};

pub(crate) use controls::ControlsPage;
pub(crate) use lists::{ListsPage, list_row};
pub(crate) use text::TextPage;

/// A page of the gallery, in sidebar order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Page {
    #[default]
    Controls,
    Text,
    Lists,
}

impl Page {
    pub(crate) const ALL: [Page; 3] = [Page::Controls, Page::Text, Page::Lists];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Page::Controls => "Controls",
            Page::Text => "Text",
            Page::Lists => "Lists and trees",
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
