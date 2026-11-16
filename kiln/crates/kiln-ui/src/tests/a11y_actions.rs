// SPDX-License-Identifier: Apache-2.0

//! `on_action` records the a11y actions an element registers, per draw, and
//! `perform` runs the recorded listeners as a screen reader's request would.

use std::{cell::Cell, rc::Rc};

use gpui::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    TestAppContext, VisualTestContext, Window, accesskit::Action, div,
};

use crate::{actions, on_action, perform, perform_in};

/// Two elements; the first's Click bumps `hits` and, if `twice`, does so
/// from a second registration too. `with_second` says whether the second
/// element is drawn at all.
struct Host {
    hits: Rc<Cell<u32>>,
    bump: u32,
    twice: bool,
    with_second: bool,
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (hits, bump) = (self.hits.clone(), self.bump);
        let mut first = on_action(
            div().id("first"),
            window,
            cx,
            Action::Click,
            move |_, _, _| hits.set(hits.get() + bump),
        );
        if self.twice {
            let hits = self.hits.clone();
            first = on_action(first, window, cx, Action::Click, move |_, _, _| {
                hits.set(hits.get() + 100)
            });
        }
        let first = on_action(first, window, cx, Action::Focus, |_, _, _| {});
        let mut root = div().child(first);
        if self.with_second {
            root = root.child(on_action(
                div().id("second"),
                window,
                cx,
                Action::Click,
                |_, _, _| {},
            ));
        }
        root
    }
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn host(hits: &Rc<Cell<u32>>, twice: bool) -> Host {
    Host {
        hits: hits.clone(),
        bump: 1,
        twice,
        with_second: true,
    }
}

#[gpui::test]
fn registered_actions_are_recorded_replaced_and_performed(cx: &mut TestAppContext) {
    let hits = Rc::new(Cell::new(0));
    let (host, cx) = cx.add_window_view(|_, _| self::host(&hits, false));
    redraw(cx);
    let recorded = cx.update(|window, cx| actions(window, cx));
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].actions, [Action::Click, Action::Focus]);
    assert_eq!(recorded[1].actions, [Action::Click]);

    // A screen reader's request runs the recorded listener.
    let first = recorded[0].id.clone();
    assert!(cx.update(|window, cx| perform(window, &first, Action::Click, cx)));
    assert_eq!(hits.get(), 1);
    // An action never registered has nothing to run.
    assert!(!cx.update(|window, cx| perform(window, &first, Action::Expand, cx)));

    // A redraw replaces the list and the listener, and drops what is gone.
    host.update(cx, |host, cx| {
        host.bump = 10;
        host.with_second = false;
        cx.notify();
    });
    redraw(cx);
    assert_eq!(cx.update(|window, cx| actions(window, cx)).len(), 1);
    assert!(cx.update(|window, cx| perform(window, &first, Action::Click, cx)));
    assert_eq!(hits.get(), 11);
}

#[gpui::test]
fn an_action_registered_twice_runs_both_listeners(cx: &mut TestAppContext) {
    let hits = Rc::new(Cell::new(0));
    let (_host, cx) = cx.add_window_view(|_, _| host(&hits, true));
    redraw(cx);
    let recorded = cx.update(|window, cx| actions(window, cx));
    assert_eq!(
        recorded[0].actions,
        [Action::Click, Action::Click, Action::Focus]
    );
    let first = recorded[0].id.clone();
    assert!(cx.update(|window, cx| perform(window, &first, Action::Click, cx)));
    assert_eq!(hits.get(), 101);
}

/// Two views drawing the same local id are two nodes, each with its own
/// listener, told apart by the view.
struct Pair(Rc<Cell<u32>>, Rc<Cell<u32>>);

impl Render for Pair {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (a, b) = (self.0.clone(), self.1.clone());
        let left = cx.new(|_| Counter(a, 1));
        let right = cx.new(|_| Counter(b, 1));
        div().child(left).child(right)
    }
}

struct Counter(Rc<Cell<u32>>, u32);

impl Render for Counter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hits = self.0.clone();
        let step = self.1;
        on_action(
            div().id("same"),
            window,
            cx,
            Action::Click,
            move |_, _, _| hits.set(hits.get() + step),
        )
    }
}

#[gpui::test]
fn the_same_id_under_two_views_is_two_nodes(cx: &mut TestAppContext) {
    let (a, b) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let (_pair, cx) = cx.add_window_view(|_, _| Pair(a.clone(), b.clone()));
    redraw(cx);
    let recorded = cx.update(|window, cx| actions(window, cx));
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].id, recorded[1].id);
    assert_ne!(recorded[0].view, recorded[1].view);
    let (second, id) = (recorded[1].view, recorded[1].id.clone());
    assert!(cx.update(|window, cx| perform_in(window, second, &id, Action::Click, cx)));
    assert_eq!((a.get(), b.get()), (0, 1));
}
