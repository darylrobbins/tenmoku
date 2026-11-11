// SPDX-License-Identifier: Apache-2.0

//! The a11y kit: the recorder, and the Announcer's politeness.

use gpui::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    TestAppContext, Window, div,
};

use crate::{Announcer, Politeness, Role, States, a11y, announce, node, node_with, nodes};

struct Chrome;

impl Render for Chrome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .child(node(div().id("save"), window, cx, Role::Button, "Save"))
            .child(node_with(
                div().id("tab"),
                window,
                cx,
                Role::Tab,
                "a.rs",
                States {
                    selected: Some(true),
                    ..States::default()
                },
            ))
            .child(Announcer::mount(window, cx))
    }
}

#[gpui::test]
fn the_recorder_lists_id_role_label_and_states(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Chrome);
    cx.run_until_parked();
    cx.update(|window, cx| {
        let got = nodes(window, cx);
        assert_eq!(got[0].id, "save".into());
        assert_eq!((got[0].role, got[0].label.as_ref()), (Role::Button, "Save"));
        assert_eq!(got[1].role, Role::Tab);
        assert_eq!(got[1].states.selected, Some(true));
        // One entry per id, however often it draws.
        window.refresh();
        assert_eq!(nodes(window, cx).len(), 3);
    });
}

fn status(cx: &mut gpui::VisualTestContext) -> a11y::Node {
    cx.run_until_parked();
    cx.update(|window, cx| {
        nodes(window, cx)
            .into_iter()
            .find(|n| n.role == Role::Status)
            .expect("the announcer's Status node")
    })
}

#[gpui::test]
fn an_announcement_reaches_the_status_node_with_its_politeness(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Chrome);
    assert_eq!(status(cx).states.value, None);
    cx.update(|_, cx| announce(cx, "Saved", Politeness::Polite));
    let node = status(cx);
    assert_eq!(node.states.value.as_deref(), Some("Saved"));
    assert_eq!(node.states.live, Some(Politeness::Polite));
    cx.update(|_, cx| announce(cx, "Disk full", Politeness::Assertive));
    let node = status(cx);
    assert_eq!(node.states.value.as_deref(), Some("Disk full"));
    assert_eq!(node.states.live, Some(Politeness::Assertive));
}

#[gpui::test]
fn the_last_announcement_in_a_frame_wins(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Chrome);
    cx.update(|_, cx| {
        announce(cx, "one", Politeness::Polite);
        announce(cx, "two", Politeness::Polite);
    });
    assert_eq!(status(cx).states.value.as_deref(), Some("two"));
}

/// Draws one button per name, all with the local id "button".
struct Buttons(Vec<&'static str>);

impl Render for Buttons {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div();
        for (i, name) in self.0.iter().enumerate() {
            let button = div().id(("button", i));
            row = row.child(node(button, window, cx, Role::Button, *name));
        }
        row
    }
}

#[gpui::test]
fn a_node_the_next_draw_drops_leaves_the_list(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Buttons(vec!["A", "B", "C"]));
    cx.run_until_parked();
    let labels = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, cx| {
            nodes(window, cx)
                .iter()
                .map(|n| n.label.to_string())
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(labels(cx), ["A", "B", "C"]);
    view.update(cx, |buttons, cx| {
        buttons.0 = vec!["C", "A"];
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(labels(cx), ["C", "A"]);
}

struct Two(gpui::Entity<Buttons>, gpui::Entity<Buttons>);

impl Render for Two {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(self.0.clone()).child(self.1.clone())
    }
}

#[gpui::test]
fn the_same_id_under_two_views_is_two_nodes(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, cx| {
        Two(
            cx.new(|_| Buttons(vec!["left"])),
            cx.new(|_| Buttons(vec!["right"])),
        )
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let labels: Vec<_> = nodes(window, cx)
            .iter()
            .map(|n| n.label.to_string())
            .collect();
        assert_eq!(labels, ["left", "right"]);
    });
}
