// SPDX-License-Identifier: Apache-2.0

//! The Announcer accepts a message only while the window it speaks in has a
//! screen reader listening, judged when the message arrives.

use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, TestAppContext,
    VisualTestContext, Window, div,
};

use crate::{Announcer, Politeness, Role, announce, node, nodes, set_listening_for_tests};

/// Announces once from its own render, before the Announcer draws.
struct Speaker {
    say: Option<&'static str>,
}

impl Render for Speaker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(text) = self.say.take() {
            announce(cx, text, Politeness::Polite);
        }
        div()
            .child(node(div().id("x"), window, cx, Role::Button, "X"))
            .child(Announcer::mount(window, cx))
    }
}

fn spoken(cx: &mut VisualTestContext) -> Option<String> {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| {
        nodes(window, cx)
            .into_iter()
            .find(|n| n.role == Role::Status)
            .and_then(|n| n.states.value.map(|v| v.to_string()))
    })
}

#[gpui::test]
fn a_message_from_a_view_rendering_after_deactivation_is_not_replayed(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Speaker { say: None });
    cx.run_until_parked();
    cx.update(|_, cx| set_listening_for_tests(cx, false));
    view.update(cx, |s, cx| {
        s.say = Some("while off");
        cx.notify();
    });
    assert_eq!(spoken(cx), None);
    cx.update(|window, cx| {
        set_listening_for_tests(cx, true);
        window.refresh();
    });
    assert_eq!(spoken(cx), None, "reactivation must not replay it");
}

#[gpui::test]
fn a_message_from_a_render_is_judged_by_the_live_window(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, _| Speaker { say: None });
    cx.run_until_parked();
    view.update(cx, |s, cx| {
        s.say = Some("while on");
        cx.notify();
    });
    assert_eq!(spoken(cx).as_deref(), Some("while on"));
}
