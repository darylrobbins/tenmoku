// SPDX-License-Identifier: Apache-2.0

//! A pane of items: items shown, cycled, taken, inserted and reordered, the
//! close that keeps unsaved changes, the tab strip's clicks, drop zones and
//! the insertion marker, and a tab dragged to the pane's own edge.
//! Generalised from the editor's tab and drag tests, over notes.

use crate::support;

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AppContext as _, Bounds, Context, Entity, Focusable, IntoElement, Modifiers, MouseButton,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, point,
    px, size,
};
use kiln_shell::{
    Destination, Direction, ItemHandle, ItemPane, PaneView,
    pane::{DropTarget, DropZone, TabDropped, insertion, marker},
};
use kiln_ui::TAB_HEIGHT;
use support::note::{Note, note};

/// A host drawing one pane, and the tabs dropped on it.
struct Host {
    pane: Entity<ItemPane>,
    dropped: Rc<RefCell<Vec<Destination<ItemPane>>>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.pane.clone())
    }
}

struct Opened<'a> {
    host: Entity<Host>,
    first: Entity<ItemPane>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn titles(&mut self, pane: &Entity<ItemPane>) -> Vec<String> {
        pane.read_with(self.cx, |pane, cx| {
            let items = pane.pane().items();
            items
                .iter()
                .map(|item| item.tab_title(cx).to_string())
                .collect()
        })
    }

    fn active(&mut self, pane: &Entity<ItemPane>) -> Option<usize> {
        pane.read_with(self.cx, |pane, _| pane.pane().active())
    }

    fn add(&mut self, pane: &Entity<ItemPane>, note: &Entity<Note>) {
        let item: Box<dyn ItemHandle> = Box::new(note.clone());
        pane.update(self.cx, |pane, cx| pane.add_item(item, cx));
    }

    fn focused(&mut self, note: &Entity<Note>) -> bool {
        self.cx
            .update(|window, cx| note.focus_handle(cx).is_focused(window))
    }

    fn drag(&mut self, from: gpui::Point<gpui::Pixels>, to: gpui::Point<gpui::Pixels>) {
        let none = Modifiers::none();
        self.cx.simulate_mouse_down(from, MouseButton::Left, none);
        let nudged = point(from.x + px(3.), from.y + px(3.));
        self.cx.simulate_mouse_move(nudged, MouseButton::Left, none);
        self.cx.simulate_mouse_move(to, MouseButton::Left, none);
        self.draw();
    }
}

/// A host with one pane, empty.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    let first = Rc::new(RefCell::new(None));
    let made = first.clone();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let pane = cx.new(ItemPane::new);
        made.replace(Some(pane.clone()));
        let dropped: Rc<RefCell<Vec<Destination<ItemPane>>>> = Rc::default();
        let sink = dropped.clone();
        cx.subscribe(&pane, move |_, _, event: &TabDropped<ItemPane>, _| {
            sink.borrow_mut().push(event.destination.clone())
        })
        .detach();
        Host { pane, dropped }
    });
    let first = first.take().unwrap();
    let mut opened = Opened { host, first, cx };
    opened.draw();
    opened
}

#[gpui::test]
fn items_show_cycle_and_close(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let pane = s.first.clone();
    let [a, b, c] = ["a", "b", "c"].map(|title| note(title, s.cx));
    for note in [&a, &b, &c] {
        s.add(&pane, note);
    }
    assert_eq!(s.titles(&pane), ["a", "b", "c"]);
    // Added items show, unfocused.
    assert_eq!(s.active(&pane), Some(2));
    assert!(!s.focused(&c));

    let activate = |s: &mut Opened, ix: usize| {
        let pane = s.first.clone();
        s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate_item(ix, window, cx)))
    };
    assert!(activate(&mut s, 0));
    assert!(s.focused(&a));
    assert!(!activate(&mut s, 7));
    let cycle = |s: &mut Opened, back: bool| {
        let pane = s.first.clone();
        s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.cycle_items(back, window, cx)));
        s.active(&pane)
    };
    assert_eq!(cycle(&mut s, true), Some(2), "wraps back");
    assert_eq!(cycle(&mut s, false), Some(0), "wraps on");

    // Closing activates the right neighbour, or the left if it was last.
    let close = |s: &mut Opened| {
        let pane = s.first.clone();
        s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active_item(window, cx)));
    };
    activate(&mut s, 1);
    close(&mut s);
    assert_eq!(s.titles(&pane), ["a", "c"]);
    assert_eq!(s.active(&pane), Some(1));
    assert!(s.focused(&c));

    // Unsaved changes keep an item open, unless another item shows its
    // document.
    c.update(s.cx, |note, _| note.dirty = true);
    close(&mut s);
    assert_eq!(s.titles(&pane), ["a", "c"]);
    let copy = s.cx.update(|window, cx| {
        let item: &dyn ItemHandle = &c;
        item.split(window, cx).unwrap()
    });
    pane.update(s.cx, |pane, cx| pane.add_item(copy, cx));
    activate(&mut s, 1);
    close(&mut s);
    assert_eq!(s.titles(&pane), ["a", "c"], "the copy stays");

    // Closing the last item empties the pane, once.
    close(&mut s);
    close(&mut s);
    assert_eq!(s.titles(&pane), Vec::<String>::new());
    let emptied = |s: &mut Opened| pane.update(s.cx, |pane, _| pane.pane_mut().take_emptied());
    assert!(emptied(&mut s));
    assert!(!emptied(&mut s));
}

#[gpui::test]
fn take_insert_and_reorder(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let pane = s.first.clone();
    let [a, b, c] = ["a", "b", "c"].map(|title| note(title, s.cx));
    for note in [&a, &b, &c] {
        s.add(&pane, note);
    }
    let taken = s.cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            pane.activate_item(1, window, cx);
            pane.take_item(0, window, cx)
        })
    });
    assert_eq!(taken.map(|item| item.id()), Some(a.id()));
    assert_eq!(s.active(&pane), Some(0), "b, still active");
    let item: Box<dyn ItemHandle> = Box::new(a.clone());
    s.cx.update(|window, cx| {
        pane.update(cx, |pane, cx| pane.insert_item(item, Some(1), window, cx))
    });
    assert_eq!(s.titles(&pane), ["b", "a", "c"]);
    assert_eq!(s.active(&pane), Some(1));
    assert!(s.focused(&a));
    // Before the item at an index, or the end.
    s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.reorder_item(0, Some(3), window, cx)));
    assert_eq!(s.titles(&pane), ["a", "c", "b"]);
    assert_eq!(s.active(&pane), Some(2));
    s.cx.update(|window, cx| pane.update(cx, |pane, cx| pane.reorder_item(2, Some(0), window, cx)));
    assert_eq!(s.titles(&pane), ["b", "a", "c"]);
    assert!(s.focused(&b));
}

#[gpui::test]
fn the_tab_strip_draws_and_clicks_activate(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let pane = s.first.clone();
    let [a, b] = ["a", "b"].map(|title| note(title, s.cx));
    s.add(&pane, &a);
    s.add(&pane, &b);
    s.draw();
    let (first, strip, content) = pane.read_with(s.cx, |pane, _| {
        let state = pane.pane();
        (
            state.tab_bounds(0).expect("drawn"),
            state.tab_bar_bounds(),
            state.content_bounds(),
        )
    });
    assert_eq!(strip.size.height, TAB_HEIGHT);
    assert_eq!(content.top(), strip.bottom());
    s.cx.simulate_click(first.center(), Modifiers::none());
    s.draw();
    assert_eq!(s.active(&pane), Some(0));
    assert!(s.focused(&a));
}

#[gpui::test]
fn drop_zones_and_the_marker() {
    let pane = Bounds::new(point(px(100.), px(0.)), size(px(400.), px(330.)));
    let area = Bounds::new(point(px(100.), px(30.)), size(px(400.), px(300.)));
    let at = |x: f32, y: f32| DropZone::at(area, point(px(100. + x), px(30. + y)));
    assert_eq!(at(10., 150.), DropZone::Left);
    assert_eq!(at(390., 150.), DropZone::Right);
    assert_eq!(at(200., 10.), DropZone::Up);
    assert_eq!(at(200., 290.), DropZone::Down);
    assert_eq!(at(200., 150.), DropZone::Centre);
    // In a corner, the nearer edge wins.
    assert_eq!(at(20., 30.), DropZone::Left);
    assert_eq!(at(40., 15.), DropZone::Up);
    let half = Bounds::new(point(px(100.), px(0.)), size(px(200.), px(330.)));
    assert_eq!(DropZone::Left.overlay(pane, area), half);
    assert_eq!(DropZone::Centre.overlay(pane, area), area);

    let tabs = [0., 80.].map(|x| Bounds::new(point(px(x), px(0.)), size(px(80.), TAB_HEIGHT)));
    assert_eq!(insertion(&tabs, px(30.)), 0);
    assert_eq!(insertion(&tabs, px(50.)), 1);
    assert_eq!(insertion(&tabs, px(150.)), 2);
    let bar = Bounds::new(point(px(0.), px(0.)), size(px(300.), TAB_HEIGHT));
    assert_eq!(marker(&tabs, 1, bar).left(), px(79.));
    assert_eq!(marker(&tabs, 2, bar).left(), px(159.));
    assert_eq!(marker(&[], 0, bar).left(), px(0.));
}

#[gpui::test]
fn a_tab_dragged_to_its_panes_edge_splits_it(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let pane = s.first.clone();
    let [a, b] = ["a", "b"].map(|title| note(title, s.cx));
    s.add(&pane, &a);
    s.draw();
    // A pane's only tab dropped on its own pane stays as it is.
    let tab = pane.read_with(s.cx, |pane, _| pane.pane().tab_bounds(0).unwrap().center());
    let content = pane.read_with(s.cx, |pane, _| pane.pane().content_bounds());
    let edge = point(content.left() + px(10.), content.center().y);
    s.drag(tab, edge);
    s.cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
    s.draw();
    assert!(
        s.host
            .read_with(s.cx, |host, _| host.dropped.borrow().is_empty())
    );

    // With two, its left edge splits it, as the hint shows.
    s.add(&pane, &b);
    s.draw();
    let tab = pane.read_with(s.cx, |pane, _| pane.pane().tab_bounds(1).unwrap().center());
    s.drag(tab, edge);
    let hovered = pane.read_with(s.cx, |pane, _| pane.pane().hovered_drop());
    assert_eq!(hovered, Some(DropTarget::Content(DropZone::Left)));
    let hint = pane
        .read_with(s.cx, |pane, _| pane.pane().drop_hint_bounds())
        .unwrap();
    let whole = pane.read_with(s.cx, |pane, _| pane.pane().bounds());
    assert_eq!(hint.size.width, whole.size.width / 2.);
    s.cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
    s.draw();
    let dropped = s
        .host
        .read_with(s.cx, |host, _| host.dropped.borrow().clone());
    assert_eq!(
        dropped,
        [Destination::Split {
            pane: pane.clone(),
            direction: Direction::Left,
        }]
    );
    assert_eq!(
        pane.read_with(s.cx, |pane, _| pane.pane().hovered_drop()),
        None
    );
}
