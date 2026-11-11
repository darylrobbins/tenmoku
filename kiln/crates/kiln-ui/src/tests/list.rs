// SPDX-License-Identifier: Apache-2.0

//! List: a delegate's rows, selected by click and keys, confirmed by a
//! double click or `enter`, its events read with `cx.subscribe_in`; and
//! what a screen reader sees of it, a `ListBox` of `ListBoxOption`s whose
//! selected row is the active descendant while the list has focus, as in
//! a [`PaletteFrame`] that holds focus.

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    ParentElement as _, Pixels, Render, SharedString, Styled as _, Subscription, TestAppContext,
    VisualTestContext, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};

use crate::{
    ActiveTheme as _, List, ListDelegate, ListEvent, Node, PaletteFrame, ROW_HEIGHT, Role,
    RowState, UiTheme, nodes,
};

struct Numbers(usize);

impl ListDelegate for Numbers {
    fn count(&self) -> usize {
        self.0
    }

    fn render_row(
        &mut self,
        ix: usize,
        state: RowState,
        _: &mut Window,
        cx: &mut Context<List<Self>>,
    ) -> AnyElement {
        let list = cx.theme().colors.list;
        div()
            .h(ROW_HEIGHT)
            .debug_selector(move || format!("row-{ix}"))
            .when(state.selected, |row| row.bg(list.row_selected))
            .when(state.hovered, |row| row.bg(list.row_hover))
            .child(format!("Row {ix}"))
            .into_any_element()
    }

    fn accessible_name(&self, _: &App) -> SharedString {
        "Numbers".into()
    }

    fn row_name(&self, ix: usize, _: &App) -> SharedString {
        format!("Row {ix}").into()
    }
}

/// A view holding a list ten rows tall, and the events it emitted.
struct Host {
    list: Entity<List<Numbers>>,
    events: Vec<ListEvent>,
    _events: Subscription,
}

impl Host {
    fn new(rows: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let list = cx.new(|cx| List::new(Numbers(rows), cx));
        let events = cx.subscribe_in(&list, window, |host, _, event, _, _| {
            host.events.push(*event)
        });
        Self {
            list,
            events: Vec::new(),
            _events: events,
        }
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .w(px(200.))
                .h(ROW_HEIGHT * 10.)
                .debug_selector(|| "list".into())
                .child(self.list.clone()),
        )
    }
}

fn host(rows: usize, cx: &mut TestAppContext) -> (Entity<Host>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|window, cx| Host::new(rows, window, cx));
    draw(cx);
    (host, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn focus(host: &Entity<Host>, cx: &mut VisualTestContext) {
    let list = host.read_with(cx, |host, _| host.list.clone());
    cx.update(|window, cx| window.focus(&list.focus_handle(cx), cx));
}

fn selected(host: &Entity<Host>, cx: &mut VisualTestContext) -> Option<usize> {
    host.read_with(cx, |host, cx| host.list.read(cx).selected())
}

fn take_events(host: &Entity<Host>, cx: &mut VisualTestContext) -> Vec<ListEvent> {
    host.update(cx, |host, _| std::mem::take(&mut host.events))
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

#[gpui::test]
fn click_selects_and_double_click_confirms(cx: &mut TestAppContext) {
    let (host, cx) = host(5, cx);
    assert_eq!(selected(&host, cx), None, "nothing selected at first");

    let row = cx.debug_bounds("row-2").expect("row 2 drawn");
    cx.simulate_click(row.center(), Modifiers::none());
    draw(cx);
    assert_eq!(selected(&host, cx), Some(2));
    assert_eq!(take_events(&host, cx), [ListEvent::Selected(2)]);
    let focused = host.read_with(cx, |host, _| host.list.clone());
    assert!(
        cx.update(|window, cx| focused.focus_handle(cx).is_focused(window)),
        "a click focuses the list"
    );

    let row = cx.debug_bounds("row-4").unwrap().center();
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            position: row,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: row,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
    assert_eq!(
        take_events(&host, cx),
        [ListEvent::Selected(4), ListEvent::Confirmed(4)]
    );
}

#[gpui::test]
fn keys_move_the_selection_and_enter_confirms(cx: &mut TestAppContext) {
    let (host, cx) = host(50, cx);
    focus(&host, cx);

    press("down", cx);
    assert_eq!(selected(&host, cx), Some(0), "down from nothing: the first");
    press("down down up", cx);
    assert_eq!(selected(&host, cx), Some(1));
    press("up up", cx);
    assert_eq!(selected(&host, cx), Some(0), "up stops at the first row");
    press("pagedown", cx);
    assert_eq!(
        selected(&host, cx),
        Some(10),
        "a page is the ten rows shown"
    );
    press("pageup", cx);
    assert_eq!(selected(&host, cx), Some(0));
    press("end", cx);
    assert_eq!(selected(&host, cx), Some(49));
    press("down", cx);
    assert_eq!(selected(&host, cx), Some(49), "down stops at the last row");
    press("home", cx);
    assert_eq!(selected(&host, cx), Some(0));
    take_events(&host, cx);

    press("down enter", cx);
    assert_eq!(
        take_events(&host, cx),
        [ListEvent::Selected(1), ListEvent::Confirmed(1)]
    );
    press("ctrl-down", cx);
    assert_eq!(
        selected(&host, cx),
        Some(1),
        "a modified key isn't the list's"
    );
}

#[gpui::test]
fn the_selected_row_scrolls_into_view(cx: &mut TestAppContext) {
    let (host, cx) = host(100, cx);
    focus(&host, cx);
    let list = cx.debug_bounds("list").unwrap();
    assert!(
        cx.debug_bounds("row-99").is_none(),
        "the last row isn't built"
    );

    press("end", cx);
    let last = cx.debug_bounds("row-99").expect("the last row drawn");
    assert!(
        last.top() >= list.top() && last.bottom() <= list.bottom() + px(0.5),
        "{last:?} within {list:?}"
    );
    assert!(
        cx.debug_bounds("row-0").is_none(),
        "scrolled past the first"
    );

    press("home", cx);
    let first = cx.debug_bounds("row-0").expect("back at the top");
    assert_eq!(first.top(), list.top());
}

#[gpui::test]
fn changed_rows_keep_the_selection_in_range(cx: &mut TestAppContext) {
    let (host, cx) = host(10, cx);
    let list = host.read_with(cx, |host, _| host.list.clone());
    list.update(cx, |list, cx| list.set_selected(Some(8), cx));
    list.update(cx, |list, cx| {
        list.delegate_mut().0 = 3;
        list.rows_changed(cx);
    });
    assert_eq!(selected(&host, cx), Some(2));
    list.update(cx, |list, cx| {
        list.delegate_mut().0 = 0;
        list.rows_changed(cx);
    });
    assert_eq!(selected(&host, cx), None);
    assert!(
        take_events(&host, cx).is_empty(),
        "the owner's changes emit nothing"
    );
}

#[gpui::test]
fn the_scrollbar_drags_the_rows(cx: &mut TestAppContext) {
    use crate::{SCROLLBAR_WIDTH, scroll_area::thumb_bounds};
    use gpui::{Bounds, point, size};

    let (_, cx) = host(100, cx);
    let list = cx.debug_bounds("list").unwrap();
    let track = Bounds::new(
        point(list.right() - SCROLLBAR_WIDTH, list.top()),
        size(SCROLLBAR_WIDTH, list.size.height),
    );
    // Ninety rows past the ten in view.
    let max = ROW_HEIGHT * 90.;
    let thumb = thumb_bounds(track, px(0.), max).unwrap();
    let room = track.size.height - thumb.size.height;
    let (from, none) = (thumb.center(), Modifiers::none());
    let to = point(from.x, from.y + room / 2.);
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    draw(cx);
    assert!(cx.debug_bounds("row-0").is_none(), "scrolled away");
    let row = cx
        .debug_bounds("row-45")
        .expect("half way: row 45 at the top");
    assert!((row.top() - list.top()).abs() <= px(1.), "{row:?}");
}

#[test]
fn list_rows_show_their_state() {
    let theme = crate::Theme::dark();
    let (c, list) = (&theme.colors, &theme.colors.list);
    let state = |selected, focused, hovered| RowState {
        selected,
        focused,
        hovered,
    };
    assert_eq!(state(false, true, false).colors(c), (None, None));
    assert_eq!(
        state(false, false, true).colors(c),
        (Some(list.row_hover), None)
    );
    assert_eq!(
        state(true, true, true).colors(c),
        (Some(list.row_selected), Some(c.accent.base)),
        "selected wins over hovered"
    );
    assert_eq!(
        state(true, false, false).colors(c),
        (Some(list.row_selected_inactive), None)
    );
}

/// What the recorder saw in a frame that drew every view afresh.
fn drawn(cx: &mut VisualTestContext) -> Vec<Node> {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
        nodes(window, cx)
    })
}

/// The recorded options, as (label, selected).
fn options(nodes: &[Node]) -> Vec<(String, bool)> {
    (nodes.iter())
        .filter(|n| n.role == Role::ListBoxOption)
        .map(|n| (n.label.to_string(), n.states.selected == Some(true)))
        .collect()
}

/// Where the row the active descendant is drawn, if one is.
fn active(cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(RowState::ACTIVE_DESCENDANT)
}

/// Whether `active` is where `row` (the delegate's element) was drawn.
fn is_row(active: Option<Bounds<Pixels>>, row: &'static str, cx: &mut VisualTestContext) -> bool {
    let row = cx
        .debug_bounds(row)
        .unwrap_or_else(|| panic!("{row} drawn"));
    active.is_some_and(|active| active.top() == row.top() && active.bottom() == row.bottom())
}

#[gpui::test]
fn the_list_is_a_named_listbox_of_named_options(cx: &mut TestAppContext) {
    let (host, cx) = host(3, cx);
    focus(&host, cx);
    press("down down", cx);
    let nodes = drawn(cx);
    let list = (nodes.iter())
        .find(|n| n.role == Role::ListBox)
        .expect("the list is a ListBox");
    assert_eq!(list.label, "Numbers");
    assert_eq!(
        options(&nodes),
        [
            ("Row 0".into(), false),
            ("Row 1".into(), true),
            ("Row 2".into(), false)
        ]
    );
}

#[gpui::test]
fn the_active_descendant_follows_the_selection(cx: &mut TestAppContext) {
    let (host, cx) = host(50, cx);
    focus(&host, cx);
    drawn(cx);
    assert_eq!(active(cx), None, "nothing selected, nothing active");

    press("down", cx);
    drawn(cx);
    assert!(is_row(active(cx), "row-0", cx), "down: the first row");
    press("down down", cx);
    drawn(cx);
    assert!(is_row(active(cx), "row-2", cx));
    press("end", cx);
    let nodes = drawn(cx);
    assert!(is_row(active(cx), "row-49", cx), "scrolled to the last");
    assert_eq!(
        options(&nodes)
            .into_iter()
            .filter(|(_, s)| *s)
            .collect::<Vec<_>>(),
        [("Row 49".to_string(), true)],
        "only the active row is selected"
    );

    // The rows change under the selection, as a filter would: it is kept
    // in range, and the active descendant goes with it.
    let list = host.read_with(cx, |host, _| host.list.clone());
    list.update(cx, |list, cx| {
        list.delegate_mut().0 = 5;
        list.rows_changed(cx);
    });
    drawn(cx);
    assert!(is_row(active(cx), "row-4", cx), "clamped to the last row");
    list.update(cx, |list, cx| list.set_selected(Some(1), cx));
    drawn(cx);
    assert!(is_row(active(cx), "row-1", cx), "the owner's selection too");

    // Without focus the list's rows aren't where a screen reader is.
    cx.update(|window, cx| window.blur(cx));
    drawn(cx);
    assert_eq!(active(cx), None, "unfocused, no active descendant");
    assert!(
        options(&drawn(cx)).contains(&("Row 1".into(), true)),
        "still selected"
    );
}

/// A palette holding focus itself, as a tab switcher does, over `count`
/// rows of which `selected` is selected.
struct Switcher {
    focus: FocusHandle,
    selected: usize,
    count: usize,
}

impl Switcher {
    fn rows(
        &mut self,
        range: std::ops::Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let focused = self.focus.is_focused(window);
        range
            .map(|ix| {
                let state = RowState {
                    selected: ix == self.selected,
                    hovered: false,
                    focused,
                };
                let name = format!("Tab {ix}");
                state
                    .option(div().id(ix), name, (ix, self.count), window, cx)
                    .h(ROW_HEIGHT)
                    .debug_selector(move || format!("tab-{ix}"))
                    .into_any_element()
            })
            .collect()
    }
}

impl Render for Switcher {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = uniform_list(
            "switcher-rows",
            self.count,
            cx.processor(|switcher, range, window, cx| switcher.rows(range, window, cx)),
        )
        .h(ROW_HEIGHT * self.count as f32);
        div().size_full().child(
            PaletteFrame::new(&UiTheme::default_dark())
                .name("Switch Tab")
                .track_focus(&self.focus)
                .list(list),
        )
    }
}

#[gpui::test]
fn a_palette_holding_focus_is_a_dialog_over_its_active_row(cx: &mut TestAppContext) {
    let (switcher, cx) = cx.add_window_view(|_, cx| Switcher {
        focus: cx.focus_handle(),
        selected: 1,
        count: 3,
    });
    let focus = switcher.read_with(cx, |s, _| s.focus.clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    let nodes = drawn(cx);
    let roles: Vec<_> = (nodes.iter())
        .filter(|n| matches!(n.role, Role::Dialog | Role::ListBox))
        .map(|n| (n.role, n.label.to_string()))
        .collect();
    assert_eq!(
        roles,
        [
            (Role::Dialog, "Switch Tab".into()),
            (Role::ListBox, "Switch Tab".into())
        ]
    );
    assert!(is_row(active(cx), "tab-1", cx));

    switcher.update(cx, |s, cx| {
        s.selected = 2;
        cx.notify();
    });
    let nodes = drawn(cx);
    assert!(is_row(active(cx), "tab-2", cx), "follows the selection");
    assert_eq!(
        options(&nodes),
        [
            ("Tab 0".into(), false),
            ("Tab 1".into(), false),
            ("Tab 2".into(), true)
        ]
    );
}
