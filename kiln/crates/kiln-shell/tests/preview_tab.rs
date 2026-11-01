// SPDX-License-Identifier: LicenseRef-Proprietary

//! A pane's preview item: its tab is drawn in italics, and the next
//! preview takes its place; an edit, a double-click on its tab or pinning
//! keeps it, as an ordinary item the next preview doesn't replace.

use crate::support;

use gpui::{
    AppContext as _, Context, Entity, IntoElement, Modifiers, MouseButton, MouseDownEvent,
    MouseUpEvent, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext,
    Window, div,
};
use kiln_shell::{ItemPane, PaneView as _};
use support::note::{Note, note};

struct Host(Entity<ItemPane>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

fn window(cx: &mut TestAppContext) -> (Entity<ItemPane>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(ItemPane::new)));
    let pane = host.read_with(cx, |host, _| host.0.clone());
    (pane, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn add(pane: &Entity<ItemPane>, item: &Entity<Note>, cx: &mut VisualTestContext) {
    pane.update(cx, |pane, cx| pane.add_item(Box::new(item.clone()), cx));
}

fn preview(pane: &Entity<ItemPane>, item: &Entity<Note>, cx: &mut VisualTestContext) -> usize {
    pane.update(cx, |pane, cx| pane.add_preview(Box::new(item.clone()), cx))
}

/// Each tab's title, with `*`s round it if it's drawn in italics.
fn tabs(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        let state = pane.read(cx).pane();
        let tabs = state.tabs(cx);
        (state.items().iter().zip(tabs))
            .map(|(item, tab)| {
                let title = item.tab_title(cx);
                if tab.is_preview() {
                    format!("*{title}*")
                } else {
                    title.to_string()
                }
            })
            .collect()
    })
}

fn active(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Option<usize> {
    pane.read_with(cx, |pane, _| pane.pane().active())
}

fn click_tab(pane: &Entity<ItemPane>, ix: usize, clicks: usize, cx: &mut VisualTestContext) {
    let at = pane.read_with(cx, |pane, _| {
        pane.pane().tab_bounds(ix).expect("drawn").center()
    });
    for click_count in 1..=clicks {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
}

#[gpui::test]
fn a_preview_is_italic_and_the_next_preview_takes_its_place(cx: &mut TestAppContext) {
    let (pane, cx) = window(cx);
    let [a, b, one, two] = ["a", "b", "one", "two"].map(|title| note(title, cx));
    add(&pane, &a, cx);
    assert_eq!(preview(&pane, &one, cx), 1);
    add(&pane, &b, cx);
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["a", "*one*", "b"]);
    assert_eq!(active(&pane, cx), Some(2));

    assert_eq!(preview(&pane, &two, cx), 1, "in the preview's place");
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["a", "*two*", "b"], "one is closed");
    assert_eq!(active(&pane, cx), Some(2), "b is still the active item");

    pane.update(cx, |pane, _| pane.pane_mut().set_active(1));
    assert_eq!(preview(&pane, &one, cx), 1);
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["a", "*one*", "b"]);
    assert_eq!(
        active(&pane, cx),
        Some(1),
        "the new preview shows in its place"
    );
}

#[gpui::test]
fn an_edit_keeps_the_preview(cx: &mut TestAppContext) {
    let (pane, cx) = window(cx);
    let [one, two] = ["one", "two"].map(|title| note(title, cx));
    preview(&pane, &one, cx);
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["*one*"]);

    one.update(cx, |note, cx| {
        note.dirty = true;
        cx.notify();
    });
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["one"], "an edited preview is kept");

    // Kept for good: saved, it doesn't go back to being the preview.
    one.update(cx, |note, cx| {
        note.dirty = false;
        cx.notify();
    });
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["one"]);
    assert_eq!(
        preview(&pane, &two, cx),
        1,
        "the next preview goes after it"
    );
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["one", "*two*"]);
}

#[gpui::test]
fn a_dirty_preview_is_kept_before_its_observer_hears(cx: &mut TestAppContext) {
    let (pane, cx) = window(cx);
    let [one, two] = ["one", "two"].map(|title| note(title, cx));
    preview(&pane, &one, cx);
    // Dirty, with no notify yet.
    one.update(cx, |note, _| note.dirty = true);
    assert_eq!(preview(&pane, &two, cx), 1);
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["one", "*two*"]);
}

#[gpui::test]
fn a_double_click_on_its_tab_keeps_the_preview(cx: &mut TestAppContext) {
    let (pane, cx) = window(cx);
    let [a, one, two] = ["a", "one", "two"].map(|title| note(title, cx));
    add(&pane, &a, cx);
    preview(&pane, &one, cx);
    draw(cx);

    click_tab(&pane, 1, 1, cx);
    assert_eq!(active(&pane, cx), Some(1));
    assert_eq!(
        tabs(&pane, cx),
        ["a", "*one*"],
        "a single click only shows it"
    );

    click_tab(&pane, 1, 2, cx);
    assert_eq!(tabs(&pane, cx), ["a", "one"], "a double-click keeps it");
    preview(&pane, &two, cx);
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["a", "one", "*two*"]);
}

#[gpui::test]
fn pinning_keeps_the_preview(cx: &mut TestAppContext) {
    let (pane, cx) = window(cx);
    let [a, one, two] = ["a", "one", "two"].map(|title| note(title, cx));
    add(&pane, &a, cx);
    preview(&pane, &one, cx);
    pane.update(cx, |pane, cx| pane.pin_item(0, cx));
    draw(cx);
    assert_eq!(
        tabs(&pane, cx),
        ["a", "*one*"],
        "pinning another item keeps it the preview"
    );

    pane.update(cx, |pane, cx| pane.pin_item(1, cx));
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["a", "one"]);
    preview(&pane, &two, cx);
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["a", "one", "*two*"]);
}

#[gpui::test]
fn a_preview_taken_out_of_the_pane_is_no_longer_its_preview(cx: &mut TestAppContext) {
    let (pane, cx) = window(cx);
    let [a, one, two] = ["a", "one", "two"].map(|title| note(title, cx));
    add(&pane, &a, cx);
    preview(&pane, &one, cx);
    let taken = cx.update(|window, cx| pane.update(cx, |pane, cx| pane.take_item(1, window, cx)));
    assert!(taken.is_some());
    pane.update(cx, |pane, cx| pane.add_item(taken.unwrap(), cx));
    draw(cx);
    assert_eq!(
        tabs(&pane, cx),
        ["a", "one"],
        "back in, it's an ordinary item"
    );
    preview(&pane, &two, cx);
    draw(cx);
    assert_eq!(tabs(&pane, cx), ["a", "one", "*two*"]);
}
