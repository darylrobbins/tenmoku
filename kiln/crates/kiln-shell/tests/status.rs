// SPDX-License-Identifier: Apache-2.0

//! The status bar's items: the host's over the active item's, sorted by
//! side and priority, a host item replacing one with its id until it's
//! removed; and a click running an item's command in the focused
//! element's context. Generalised from the editor's status bar tests
//! (`host_items`).

use crate::support;

use gpui::{
    Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Render, SharedString, Styled as _, TestAppContext, VisualTestContext,
    Window, div,
};
use kiln_command::Invocation;
use kiln_platform::commands;
use kiln_shell::{StatusItem, StatusItems, StatusSide, status};

use support::pad;

/// A view with the bar along its bottom: the host's items over `own`, the
/// active item's. It handles `pad.clear` and `pad.select_all`.
struct Bench {
    focus: FocusHandle,
    items: StatusItems,
    own: Vec<StatusItem>,
    ran: Vec<&'static str>,
}

impl Bench {
    fn clear(&mut self, _: &pad::Clear, _: &mut Window, _: &mut Context<Self>) {
        self.ran.push("pad.clear");
    }

    fn select_all(&mut self, _: &pad::SelectAll, _: &mut Window, _: &mut Context<Self>) {
        self.ran.push("pad.select_all");
    }
}

impl Focusable for Bench {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Bench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.items.merged(self.own.clone());
        div()
            .key_context("bench")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(Self::select_all))
            .size_full()
            .flex()
            .flex_col()
            .child(div().flex_1())
            .child(status::status_bar(items, cx))
    }
}

fn item(id: &str, side: StatusSide, priority: i32, text: &str) -> StatusItem {
    StatusItem::new(
        SharedString::from(id.to_owned()),
        side,
        priority,
        text.to_owned(),
    )
}

fn texts(items: &[StatusItem]) -> Vec<&str> {
    items.iter().map(|item| item.text.as_ref()).collect()
}

#[test]
fn host_items_sort_and_replace_the_items_own() {
    let own = vec![
        item("language", StatusSide::Right, 500, "Rust"),
        item("cursor", StatusSide::Right, 100, "Ln 1, Col 1"),
        item("mode", StatusSide::Left, 0, "NORMAL"),
    ];
    let mut items = StatusItems::default();
    assert_eq!(
        texts(&items.merged(own.clone())),
        ["NORMAL", "Ln 1, Col 1", "Rust"]
    );

    items.set(item("host.dock", StatusSide::Left, 0, "Dock"));
    items.set(item("language", StatusSide::Right, 500, "Host Rust"));
    items.set(item("host.ready", StatusSide::Right, 100, "Ready"));
    // Ties keep their order: the item's own first, then the host's.
    assert_eq!(
        texts(&items.merged(own.clone())),
        ["NORMAL", "Dock", "Ln 1, Col 1", "Ready", "Host Rust"]
    );

    // Set again, it changes where it was.
    items.set(item("host.dock", StatusSide::Left, 0, "Docked"));
    assert_eq!(texts(items.host()), ["Docked", "Host Rust", "Ready"]);

    assert!(items.remove("language"));
    assert!(!items.remove("language"));
    assert_eq!(
        texts(&items.merged(own)),
        ["NORMAL", "Docked", "Ln 1, Col 1", "Ready", "Rust"]
    );
    // With no active item, only the host's.
    assert_eq!(texts(&items.merged([])), ["Docked", "Ready"]);
}

fn open(cx: &mut TestAppContext) -> (Entity<Bench>, &mut VisualTestContext) {
    cx.update(|cx| {
        commands::init(cx, "Pad");
        pad::register(cx);
    });
    let (bench, cx) = cx.add_window_view(|_, cx| Bench {
        focus: cx.focus_handle(),
        items: StatusItems::default(),
        own: vec![item("cursor", StatusSide::Right, 100, "Ln 1, Col 1")],
        ran: Vec::new(),
    });
    cx.update(|window, cx| window.focus(&bench.focus_handle(cx), cx));
    cx.run_until_parked();
    (bench, cx)
}

fn click(id: &str, cx: &mut VisualTestContext) {
    // `debug_bounds` takes a `&'static str`.
    let bounds = cx
        .debug_bounds(format!("status-item:{id}").leak())
        .expect("drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn a_click_runs_the_items_command_where_focus_is(cx: &mut TestAppContext) {
    let (bench, cx) = open(cx);
    let seen = support::record(cx);
    bench.update(cx, |bench, cx| {
        bench.items.set(
            item("host.clear", StatusSide::Left, 0, "Clear")
                .command(Invocation::of("pad.clear").unwrap()),
        );
        cx.notify();
    });
    cx.run_until_parked();
    let bar = cx.debug_bounds("status-bar").expect("drawn");
    let window = cx.update(|window, _| window.viewport_size());
    assert_eq!(bar.bottom(), window.height);
    let clear = cx.debug_bounds("status-item:host.clear").expect("drawn");
    let cursor = cx.debug_bounds("status-item:cursor").expect("drawn");
    assert!(clear.right() < cursor.left(), "left, then right");

    click("host.clear", cx);
    assert_eq!(
        bench.read_with(cx, |bench, _| bench.ran.clone()),
        ["pad.clear"]
    );
    // Stamped with the focused element's context, as a key would be.
    let seen = seen.borrow();
    let context = &seen.last().expect("heard").context;
    assert!(
        context.frames().iter().any(|frame| frame.has("bench")),
        "{context:?}"
    );

    // An item with no command does nothing.
    click("cursor", cx);
    assert_eq!(bench.read_with(cx, |bench, _| bench.ran.len()), 1);
}
