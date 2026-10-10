// SPDX-License-Identifier: LicenseRef-Proprietary

//! A virtualised List of a thousand rows, and a Tree over a small model
//! held in memory.

use gpui::{
    AnyElement, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use kiln::{
    prelude::*,
    ui::{
        Icon, List, ListDelegate, ListEvent, ROW_HEIGHT, RowState, Tree, TreeModel, file_icon,
        folder_icon, h_flex, v_flex,
    },
};

use super::{page_frame, section};

/// How many rows the list has.
pub(crate) const ROWS: usize = 1_000;

/// The list's rows: numbers, drawn with the theme's list colours.
pub(crate) struct Numbers;

impl ListDelegate for Numbers {
    fn count(&self) -> usize {
        ROWS
    }

    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        _: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> AnyElement {
        let c = cx.theme().colors;
        list_row(state, &c)
            .px_3()
            .justify_between()
            .child(format!("Row {}", ix + 1))
            .child(
                div()
                    .text_color(c.text.muted)
                    .child(format!("#{:04x}", ix * 37 % 0xffff)),
            )
            .into_any_element()
    }
}

/// A list row's look for `state`: the list leaves that to its delegate.
pub(crate) fn list_row(state: RowState, c: &kiln::ui::Tokens<gpui::Hsla>) -> gpui::Div {
    h_flex()
        .h(ROW_HEIGHT)
        .w_full()
        .items_center()
        .when(state.hovered && !state.selected, |row| {
            row.bg(c.list.row_hover)
        })
        .when(state.selected && state.focused, |row| {
            row.bg(c.list.row_selected)
                .border_1()
                .border_color(c.list.row_focus_border)
        })
        .when(state.selected && !state.focused, |row| {
            row.bg(c.list.row_selected_inactive)
        })
}

/// A small project, as a table of paths and their children.
pub(crate) struct Project;

const PROJECT: &[(&str, &[&str])] = &[
    ("", &["notes", "assets", "Cargo.toml", "README.md"]),
    ("notes", &["notes/src", "notes/tests", "notes/Cargo.toml"]),
    (
        "notes/src",
        &[
            "notes/src/main.rs",
            "notes/src/store.rs",
            "notes/src/view.rs",
        ],
    ),
    ("notes/tests", &["notes/tests/store.rs"]),
    ("assets", &["assets/icons", "assets/theme.toml"]),
    (
        "assets/icons",
        &["assets/icons/app.svg", "assets/icons/note.png"],
    ),
];

impl TreeModel for Project {
    type Id = &'static str;

    fn roots(&self) -> Vec<Self::Id> {
        self.children(&"")
    }

    fn children(&self, id: &Self::Id) -> Vec<Self::Id> {
        PROJECT
            .iter()
            .find(|(parent, _)| parent == id)
            .map(|(_, children)| children.to_vec())
            .unwrap_or_default()
    }

    fn label(&self, id: &Self::Id) -> SharedString {
        id.rsplit('/').next().unwrap_or(id).to_owned().into()
    }

    fn icon(&self, id: &Self::Id, expanded: bool) -> Option<Icon> {
        Some(if self.is_parent(id) {
            folder_icon(expanded)
        } else {
            file_icon(id)
        })
    }
}

pub(crate) struct ListsPage {
    list: Entity<List<Numbers>>,
    tree: Entity<Tree<Project>>,
    last_event: Option<String>,
    _subscriptions: [Subscription; 2],
}

impl ListsPage {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| List::new(Numbers, cx));
        let tree = cx.new(|cx| {
            let mut tree = Tree::new(Project, cx);
            tree.set_expanded(&"notes", true, cx);
            tree.set_expanded(&"notes/src", true, cx);
            tree
        });
        let list_events = cx.subscribe_in(&list, window, |page, _, event, _, cx| {
            page.last_event = Some(match event {
                ListEvent::Selected(ix) => format!("List: selected row {}", ix + 1),
                ListEvent::Confirmed(ix) => format!("List: confirmed row {}", ix + 1),
            });
            cx.notify();
        });
        let tree_events = cx.subscribe_in(&tree, window, |page, _, event, _, cx| {
            page.last_event = Some(format!("Tree: {event:?}"));
            cx.notify();
        });
        Self {
            list,
            tree,
            last_event: None,
            _subscriptions: [list_events, tree_events],
        }
    }
}

impl Render for ListsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.theme().colors;
        let framed = |view: gpui::AnyView| {
            div()
                .h(px(420.))
                .w(px(300.))
                .border_1()
                .border_color(c.border.default)
                .bg(c.surface.panel)
                .child(view)
        };
        let status = self
            .last_event
            .clone()
            .unwrap_or_else(|| "Click a row, or focus a view and use the arrow keys".into());
        page_frame(
            "Lists and trees",
            "List virtualises its rows; Tree expands a model's nodes. Both keep the selection.",
            cx,
        )
        .child(
            h_flex()
                .gap_5()
                .items_start()
                .child(section(
                    format!("LIST ({ROWS} ROWS)"),
                    framed(self.list.clone().into()),
                    cx,
                ))
                .child(section("TREE", framed(self.tree.clone().into()), cx)),
        )
        .child(v_flex().text_color(c.text.muted).child(status))
    }
}
