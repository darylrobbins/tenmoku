// SPDX-License-Identifier: Apache-2.0

//! MenuPanel's disabled rows, and its items as menu items for assistive
//! technology.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    Context, IntoElement, Modifiers, ParentElement as _, Render, TestAppContext, VisualTestContext,
    Window, accesskit, div,
};

use crate::{MenuEntry, MenuPanel, Role, UiTheme};

/// A panel of Cut, Copy | Paste, Paste disabled, and what was clicked and
/// hovered, by entry.
#[derive(Default)]
struct Menu {
    clicks: Rc<RefCell<Vec<usize>>>,
    hovers: Rc<RefCell<Vec<usize>>>,
    /// Each item's node as it last rendered, when set.
    nodes: Option<Rc<RefCell<Vec<accesskit::Node>>>>,
}

fn item(title: &'static str) -> MenuEntry {
    MenuEntry::Item {
        title: title.into(),
        shortcut: None,
    }
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let entries = vec![
            item("Cut"),
            item("Copy"),
            MenuEntry::Separator,
            item("Paste"),
        ];
        let (clicks, hovers) = (self.clicks.clone(), self.hovers.clone());
        let mut panel = MenuPanel::new(entries, &UiTheme::default_dark())
            .disabled([3])
            .on_click(move |ix, _, _| clicks.borrow_mut().push(ix))
            .on_hover(move |ix, _, _| hovers.borrow_mut().push(ix));
        if let Some(nodes) = &self.nodes {
            nodes.borrow_mut().clear();
            panel = panel.item_nodes().record_a11y(nodes.clone());
        }
        div().child(panel)
    }
}

fn window(menu: Menu, cx: &mut TestAppContext) -> &mut VisualTestContext {
    let (_, cx) = cx.add_window_view(|_, _| menu);
    cx.run_until_parked();
    cx
}

#[gpui::test]
fn disabled_rows_take_no_click_or_hover(cx: &mut TestAppContext) {
    let menu = Menu::default();
    let (clicks, hovers) = (menu.clicks.clone(), menu.hovers.clone());
    let cx = window(menu, cx);
    let paste = cx.debug_bounds("menu-item:Paste").expect("Paste is drawn");
    let copy = cx.debug_bounds("menu-item:Copy").expect("Copy is drawn");

    cx.simulate_mouse_move(paste.center(), None, Modifiers::none());
    cx.simulate_click(paste.center(), Modifiers::none());
    cx.run_until_parked();
    assert!(clicks.borrow().is_empty(), "{:?}", clicks.borrow());
    assert!(hovers.borrow().is_empty(), "{:?}", hovers.borrow());

    // An enabled row beside it takes both.
    cx.simulate_mouse_move(copy.center(), None, Modifiers::none());
    cx.simulate_click(copy.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(*clicks.borrow(), [1]);
    assert!(hovers.borrow().contains(&1));
    assert!(!hovers.borrow().contains(&3));
}

#[gpui::test]
fn item_nodes_are_menu_items_with_their_disabled_state(cx: &mut TestAppContext) {
    let nodes: Rc<RefCell<Vec<accesskit::Node>>> = Rc::default();
    let menu = Menu {
        nodes: Some(nodes.clone()),
        ..Menu::default()
    };
    let cx = window(menu, cx);
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let nodes = nodes.borrow();
    let got: Vec<(Role, Option<String>, bool)> = (nodes.iter())
        .map(|node| {
            (
                node.role(),
                node.label().map(str::to_owned),
                node.is_disabled(),
            )
        })
        .collect();
    let menu_item = |title: &str, disabled| (Role::MenuItem, Some(title.to_owned()), disabled);
    assert_eq!(
        got,
        [
            menu_item("Cut", false),
            menu_item("Copy", false),
            menu_item("Paste", true),
        ]
    );
    // No separator has a node.
    assert_eq!(nodes.len(), 3);
}
