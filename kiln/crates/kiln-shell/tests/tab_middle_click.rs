// SPDX-License-Identifier: Apache-2.0

//! A middle click on a tab closes it, as the × does: the same neighbour
//! activation, and the same `may_close` veto for a dirty item. Driven
//! through the pointer on the tabs a pane draws.

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, ParentElement as _, Pixels, Point,
    PromptButton, PromptLevel, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};
use kiln_shell::{CloseAnswer, Icon, ItemHandle, ItemPane, PaneItem, PaneView as _, file_icon};

struct Host(Entity<ItemPane>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

struct File {
    name: SharedString,
    dirty: bool,
    focus: FocusHandle,
}

impl Render for File {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for File {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for File {
    const KIND: &'static str = "test.middle";

    fn tab_title(&self, _: &gpui::App) -> SharedString {
        self.name.clone()
    }

    fn tab_icon(&self, _: &gpui::App) -> Option<Icon> {
        Some(file_icon(&self.name))
    }

    fn is_dirty(&self, _: &gpui::App) -> bool {
        self.dirty
    }

    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if !self.dirty {
            return CloseAnswer::Close;
        }
        let buttons = [PromptButton::ok("Close"), PromptButton::cancel("Keep")];
        let answer = window.prompt(PromptLevel::Warning, &self.name, None, &buttons, cx);
        CloseAnswer::Ask(cx.spawn(async move |_, _| answer.await == Ok(0)))
    }
}

/// A pane of `a.txt`, `b.txt`, `c.txt` and a dirty `d.txt`, `b.txt` active.
fn pane(cx: &mut TestAppContext) -> (Entity<ItemPane>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(ItemPane::new)));
    let pane = host.read_with(cx, |host, _| host.0.clone());
    for (name, dirty) in [
        ("a.txt", false),
        ("b.txt", false),
        ("c.txt", false),
        ("d.txt", true),
    ] {
        let file = cx.new(|cx| File {
            name: name.into(),
            dirty,
            focus: cx.focus_handle(),
        });
        let item: Box<dyn ItemHandle> = Box::new(file);
        pane.update(cx, |pane, cx| pane.add_item(item, cx));
    }
    pane.update(cx, |pane, _| pane.pane_mut().set_active(1));
    cx.run_until_parked();
    draw(cx);
    (pane, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn titles(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> (Vec<String>, Option<usize>) {
    cx.update(|_, cx| {
        let pane = pane.read(cx).pane();
        let titles = (pane.items().iter())
            .map(|item| item.tab_title(cx).to_string())
            .collect();
        (titles, pane.active())
    })
}

/// A tab's icon, which sits inside the tab and not on its ×.
fn tab_at(name: &str, cx: &mut VisualTestContext) -> Point<Pixels> {
    let selector: &'static str = format!("tab-icon:{name}:file-text").leak();
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{name}'s tab is drawn"))
        .center()
}

fn click(position: Point<Pixels>, button: MouseButton, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position,
        modifiers: Modifiers::none(),
        button,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position,
        modifiers: Modifiers::none(),
        button,
        click_count: 1,
    });
    draw(cx);
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[gpui::test]
fn middle_click_closes_tab(cx: &mut TestAppContext) {
    let (pane, cx) = pane(cx);

    // Left and right clicks don't close; the left one activates.
    let at = tab_at("c.txt", cx);
    click(at, MouseButton::Right, cx);
    assert_eq!(
        titles(&pane, cx),
        (names(&["a.txt", "b.txt", "c.txt", "d.txt"]), Some(1))
    );
    click(at, MouseButton::Left, cx);
    assert_eq!(
        titles(&pane, cx),
        (names(&["a.txt", "b.txt", "c.txt", "d.txt"]), Some(2))
    );

    // An inactive tab closes, and no other; like the ×, it is activated
    // first, so b is the neighbour left active.
    click(tab_at("a.txt", cx), MouseButton::Middle, cx);
    assert_eq!(
        titles(&pane, cx),
        (names(&["b.txt", "c.txt", "d.txt"]), Some(0))
    );

    // The active tab closes, and the neighbour Close Tab would activate
    // is, as `close_tab_activates` shows.
    click(tab_at("c.txt", cx), MouseButton::Middle, cx);
    assert_eq!(titles(&pane, cx), (names(&["b.txt", "d.txt"]), Some(1)));

    // The active tab itself, d (dirty, so kept): b is activated and closed.
    click(tab_at("b.txt", cx), MouseButton::Left, cx);
    click(tab_at("b.txt", cx), MouseButton::Middle, cx);
    assert_eq!(titles(&pane, cx), (names(&["d.txt"]), Some(0)));
}

/// What Close Tab does at the same spot, which a middle click matches.
#[gpui::test]
fn close_tab_activates(cx: &mut TestAppContext) {
    let (pane, cx) = pane(cx);
    pane.update(cx, |pane, _| pane.pane_mut().set_active(2));
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active_item(window, cx)));
    draw(cx);
    assert_eq!(
        titles(&pane, cx),
        (names(&["a.txt", "b.txt", "d.txt"]), Some(2))
    );
}

#[gpui::test]
fn dirty_tab_keeps_open(cx: &mut TestAppContext) {
    let (pane, cx) = pane(cx);
    let all = names(&["a.txt", "b.txt", "c.txt", "d.txt"]);

    click(tab_at("d.txt", cx), MouseButton::Middle, cx);
    assert_eq!(
        cx.pending_prompt().map(|(title, _)| title),
        Some("d.txt".into())
    );
    assert_eq!(titles(&pane, cx).0, all, "asking, still open");

    cx.simulate_prompt_answer("Keep");
    draw(cx);
    assert_eq!(titles(&pane, cx).0, all, "kept");

    click(tab_at("d.txt", cx), MouseButton::Middle, cx);
    cx.simulate_prompt_answer("Close");
    draw(cx);
    assert_eq!(titles(&pane, cx).0, names(&["a.txt", "b.txt", "c.txt"]));
}
