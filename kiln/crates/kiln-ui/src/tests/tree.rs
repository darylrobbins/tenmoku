// SPDX-License-Identifier: Apache-2.0

//! Tree: a model's nodes flattened to rows, expanded by the chevron or
//! `right`, collapsed by the chevron or `left`, selected like a list's.

use gpui::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, Modifiers, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, TestAppContext, VisualTestContext, Window,
    div, px,
};

use crate::{Icon, ROW_HEIGHT, Tree, TreeEvent, TreeModel};

/// src/{lib.rs, ui/{button.rs}}, README, and an empty dir.
struct Files;

impl TreeModel for Files {
    type Id = &'static str;

    fn roots(&self) -> Vec<&'static str> {
        vec!["src", "README", "empty"]
    }

    fn children(&self, id: &&'static str) -> Vec<&'static str> {
        match *id {
            "src" => vec!["lib.rs", "ui"],
            "ui" => vec!["button.rs"],
            _ => Vec::new(),
        }
    }

    fn is_parent(&self, id: &&'static str) -> bool {
        matches!(*id, "src" | "ui" | "empty")
    }

    fn label(&self, id: &&'static str) -> SharedString {
        (*id).into()
    }

    fn icon(&self, id: &&'static str, expanded: bool) -> Option<Icon> {
        let icon = if expanded {
            Icon::FolderOpen
        } else {
            Icon::Folder
        };
        self.is_parent(id).then_some(icon)
    }
}

struct Host {
    tree: Entity<Tree<Files>>,
    events: Vec<TreeEvent<&'static str>>,
    _events: Subscription,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .w(px(240.))
                .h(ROW_HEIGHT * 10.)
                .child(self.tree.clone()),
        )
    }
}

fn host(cx: &mut TestAppContext) -> (Entity<Host>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|window, cx| {
        let tree = cx.new(|cx| Tree::new(Files, cx));
        let events = cx.subscribe_in(&tree, window, |host: &mut Host, _, event, _, _| {
            host.events.push(event.clone())
        });
        Host {
            tree,
            events: Vec::new(),
            _events: events,
        }
    });
    draw(cx);
    (host, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn tree(host: &Entity<Host>, cx: &mut VisualTestContext) -> Entity<Tree<Files>> {
    host.read_with(cx, |host, _| host.tree.clone())
}

fn visible(host: &Entity<Host>, cx: &mut VisualTestContext) -> Vec<(&'static str, usize)> {
    let tree = tree(host, cx);
    tree.read_with(cx, |tree, _| {
        tree.visible().map(|(id, depth)| (*id, depth)).collect()
    })
}

fn selected(host: &Entity<Host>, cx: &mut VisualTestContext) -> Option<&'static str> {
    let tree = tree(host, cx);
    tree.read_with(cx, |tree, _| tree.selected_id().copied())
}

fn take_events(host: &Entity<Host>, cx: &mut VisualTestContext) -> Vec<TreeEvent<&'static str>> {
    host.update(cx, |host, _| std::mem::take(&mut host.events))
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn expanded_nodes_flatten_to_rows(cx: &mut TestAppContext) {
    let (host, cx) = host(cx);
    assert_eq!(
        visible(&host, cx),
        [("src", 0), ("README", 0), ("empty", 0)]
    );

    let tree = tree(&host, cx);
    tree.update(cx, |tree, cx| {
        tree.set_expanded(&"ui", true, cx);
        tree.set_expanded(&"src", true, cx);
    });
    assert_eq!(
        visible(&host, cx),
        [
            ("src", 0),
            ("lib.rs", 1),
            ("ui", 1),
            ("button.rs", 2),
            ("README", 0),
            ("empty", 0),
        ]
    );
    tree.update(cx, |tree, cx| tree.set_expanded(&"src", false, cx));
    assert_eq!(visible(&host, cx).len(), 3);
    tree.update(cx, |tree, cx| tree.set_expanded(&"src", true, cx));
    assert_eq!(visible(&host, cx).len(), 6, "ui stayed expanded inside src");
    assert!(
        take_events(&host, cx).is_empty(),
        "the owner's changes emit nothing"
    );
}

#[gpui::test]
fn chevron_click_toggles_and_row_click_selects(cx: &mut TestAppContext) {
    let (host, cx) = host(cx);

    click("tree-chevron:src:closed", cx);
    assert_eq!(visible(&host, cx).len(), 5);
    assert_eq!(selected(&host, cx), None, "the chevron doesn't select");
    assert_eq!(take_events(&host, cx), [TreeEvent::Expanded("src")]);

    assert!(cx.debug_bounds("tree-icon:src:folder-open").is_some());
    // Click the label, right of the chevron and icon columns.
    let ui = cx.debug_bounds("tree-chevron:ui:closed").unwrap();
    cx.simulate_click(
        gpui::point(ui.right() + px(60.), ui.center().y),
        Modifiers::none(),
    );
    draw(cx);
    assert_eq!(selected(&host, cx), Some("ui"));
    assert_eq!(take_events(&host, cx), [TreeEvent::Selected("ui")]);

    click("tree-chevron:src:open", cx);
    assert_eq!(visible(&host, cx).len(), 3);
    assert_eq!(
        selected(&host, cx),
        Some("src"),
        "collapsing the selection's ancestor selects it"
    );
    assert_eq!(
        take_events(&host, cx),
        [TreeEvent::Collapsed("src"), TreeEvent::Selected("src")]
    );
}

#[gpui::test]
fn keys_walk_expand_and_collapse(cx: &mut TestAppContext) {
    let (host, cx) = host(cx);
    let tree = tree(&host, cx);
    cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));

    press("down", cx);
    assert_eq!(selected(&host, cx), Some("src"));
    press("right", cx);
    assert_eq!(visible(&host, cx).len(), 5, "right expands");
    assert_eq!(selected(&host, cx), Some("src"));
    press("right", cx);
    assert_eq!(
        selected(&host, cx),
        Some("lib.rs"),
        "then goes to the first child"
    );
    press("right", cx);
    assert_eq!(
        selected(&host, cx),
        Some("lib.rs"),
        "a leaf has nowhere to go"
    );
    press("down right right", cx);
    assert_eq!(selected(&host, cx), Some("button.rs"));
    press("left", cx);
    assert_eq!(selected(&host, cx), Some("ui"), "left goes to the parent");
    press("left", cx);
    assert_eq!(visible(&host, cx).len(), 5, "then collapses it");
    press("left left", cx);
    assert_eq!(selected(&host, cx), Some("src"));
    assert_eq!(visible(&host, cx).len(), 3);
    take_events(&host, cx);

    press("end enter", cx);
    assert_eq!(
        take_events(&host, cx),
        [TreeEvent::Selected("empty"), TreeEvent::Confirmed("empty")]
    );
    press("right", cx);
    assert_eq!(
        take_events(&host, cx),
        [TreeEvent::Expanded("empty")],
        "a parent with no children expands"
    );
    press("right", cx);
    assert_eq!(selected(&host, cx), Some("empty"), "to no child");
}

#[gpui::test]
fn the_selection_follows_its_node(cx: &mut TestAppContext) {
    let (host, cx) = host(cx);
    let tree = tree(&host, cx);
    tree.update(cx, |tree, cx| tree.set_selected(Some(&"README"), cx));
    tree.update(cx, |tree, cx| tree.set_expanded(&"src", true, cx));
    assert_eq!(selected(&host, cx), Some("README"), "moved down two rows");
    assert!(tree.read_with(cx, |tree, _| tree.is_expanded(&"src")));
}
