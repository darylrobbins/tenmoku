//! The operation in progress on the status bar
//! ([`crate::operations_status`]): its item, the list a click opens and
//! how often it repaints.

use std::time::Duration;

use fe_buffer::Buffer;
use fe_ui::Role;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_l10n::tr;

use crate::{
    Launch, Workbench,
    operations::{OperationId, Operations},
    operations_status::{BAR_WIDTH, OperationsStatus, count_in, item_name},
    tests::open,
};

const ITEM: &str = "status-operation";

/// A workbench on a temporary project, with no language server.
fn opened(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.run_until_parked();
    // The directory outlives the test's use of it.
    std::mem::forget(dir);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn operations(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Operations> {
    workbench.read_with(cx, |workbench, _| workbench.operations().clone())
}

fn item(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<OperationsStatus> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.operations_item())
}

fn begin(workbench: &Entity<Workbench>, label: &str, cx: &mut VisualTestContext) -> OperationId {
    operations(workbench, cx).update(cx, |operations, cx| operations.begin(label, cx))
}

fn say(
    workbench: &Entity<Workbench>,
    id: &OperationId,
    fraction: Option<f32>,
    detail: Option<&str>,
    cx: &mut VisualTestContext,
) {
    let detail = detail.map(str::to_owned);
    operations(workbench, cx).update(cx, |operations, cx| {
        assert!(operations.update(id, fraction, detail, cx));
    });
}

fn renders(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    item(workbench, cx).read_with(cx, |item, _| item.renders)
}

#[test]
fn counts_are_read_from_messages() {
    assert_eq!(count_in("3/12"), Some((3, 12)));
    assert_eq!(count_in("Indexing 3/12 crates"), Some((3, 12)));
    assert_eq!(count_in("v2 then 40/100"), Some((40, 100)));
    assert_eq!(count_in("Indexing"), None);
    assert_eq!(count_in("12/ crates"), None);
    assert_eq!(count_in("50%"), None);
}

#[gpui::test]
fn the_item_is_absent_when_idle_and_the_bar_does_not_shift(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    draw(cx);
    assert_eq!(cx.debug_bounds(ITEM), None);
    let bar = cx.debug_bounds("status-bar").expect("the bar");
    let bell = cx.debug_bounds("status-item:notifications");

    let id = begin(&workbench, "Indexing", cx);
    draw(cx);
    assert!(cx.debug_bounds(ITEM).is_some());
    assert_eq!(cx.debug_bounds("status-bar"), Some(bar));
    assert_eq!(cx.debug_bounds("status-item:notifications"), bell);

    operations(&workbench, cx).update(cx, |operations, cx| operations.end(&id, cx));
    draw(cx);
    assert_eq!(cx.debug_bounds(ITEM), None);
    assert_eq!(cx.debug_bounds("status-operation-spinner"), None);
    assert_eq!(cx.debug_bounds("status-bar"), Some(bar));
    assert_eq!(cx.debug_bounds("status-item:notifications"), bell);
}

#[gpui::test]
fn label_bar_count_and_plus_n(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    let first = begin(&workbench, "Indexing", cx);
    draw(cx);

    // A label and a spinner alone: no bar, count or +N.
    let spinner = cx
        .debug_bounds("status-operation-spinner")
        .expect("spinner");
    assert_eq!(
        (spinner.size.width, spinner.size.height),
        (gpui::px(12.), gpui::px(12.))
    );
    assert!(cx.debug_bounds("status-operation-label").is_some());
    assert_eq!(cx.debug_bounds("status-operation-bar"), None);
    let drawn =
        |cx: &mut VisualTestContext| item(&workbench, cx).read_with(cx, |i, _| i.drawn.clone());
    assert_eq!(drawn(cx), ("Indexing".into(), None, None));

    // A fraction draws the 72 px bar, 4 px tall, filled that far.
    say(&workbench, &first, Some(0.25), None, cx);
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    let bar = cx.debug_bounds("status-operation-bar").expect("bar");
    assert_eq!(
        (bar.size.width, bar.size.height),
        (gpui::px(BAR_WIDTH), gpui::px(4.))
    );
    let fill = cx.debug_bounds("status-operation-fill").expect("fill");
    assert_eq!(fill.size.width, gpui::px(BAR_WIDTH * 0.25));
    assert!(bar.left() > cx.debug_bounds("status-operation-label").unwrap().right());

    // A message with a count shows it, after the bar.
    say(
        &workbench,
        &first,
        Some(0.25),
        Some("Indexing 3/12 crates"),
        cx,
    );
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    let count = tr!(
        "workbench-status-operation-count",
        done = 3u64,
        total = 12u64
    );
    assert_eq!(drawn(cx), ("Indexing".into(), Some(count), None));
    let counted = cx.debug_bounds("status-operation-count").expect("count");
    assert!(counted.left() >= bar.right());

    // The others are +N, last.
    begin(&workbench, "Checking", cx);
    begin(&workbench, "Building", cx);
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    let more = tr!("workbench-status-operation-more", count = 2usize);
    assert_eq!(more, "+2");
    let (_, count, plus) = drawn(cx);
    assert!(count.is_some());
    assert_eq!(plus, Some(more));
    let plus = cx.debug_bounds("status-operation-more").expect("+N");
    assert!(plus.left() >= counted.right());
}

#[gpui::test]
fn click_lists_every_operation(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    begin(&workbench, "Indexing", cx);
    let second = begin(&workbench, "Checking", cx);
    begin(&workbench, "Building", cx);
    say(&workbench, &second, Some(0.5), None, cx);
    draw(cx);
    assert_eq!(cx.debug_bounds("status-operations-list"), None);

    let at = cx.debug_bounds(ITEM).expect("item").center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    let list = cx.debug_bounds("status-operations-list").expect("the list");
    let item_bounds = cx.debug_bounds(ITEM).unwrap();
    // Above the item.
    assert!(
        list.bottom() <= item_bounds.top(),
        "{list:?} {item_bounds:?}"
    );
    let rows: Vec<_> = [
        "status-operations-row:0",
        "status-operations-row:1",
        "status-operations-row:2",
        "status-operations-row:3",
    ]
    .map(|name| cx.debug_bounds(name))
    .into();
    assert!(rows[..3].iter().all(Option::is_some), "{rows:?}");
    assert_eq!(rows[3], None);
    assert!(list.contains(&rows[0].unwrap().origin));
    assert!(rows[0].unwrap().top() < rows[1].unwrap().top());
    assert!(item(&workbench, cx).read_with(cx, |item, _| item.is_open()));

    // Esc closes it.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(cx.debug_bounds("status-operations-list"), None);

    // A click on the item opens it and a second closes it.
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert!(cx.debug_bounds("status-operations-list").is_some());
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(cx.debug_bounds("status-operations-list"), None);

    // A click elsewhere closes it.
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert!(cx.debug_bounds("status-operations-list").is_some());
    cx.simulate_click(
        gpui::point(gpui::px(500.), gpui::px(200.)),
        Modifiers::none(),
    );
    draw(cx);
    assert_eq!(cx.debug_bounds("status-operations-list"), None);
}

#[gpui::test]
fn repaint_is_capped_at_10_per_second(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    let id = begin(&workbench, "Indexing", cx);
    draw(cx);
    let before = renders(&workbench, cx);
    let ticks = |cx: &mut VisualTestContext| item(&workbench, cx).read_with(cx, |i, _| i.repaints);
    let ticks_before = ticks(cx);

    // The operation saying something new every millisecond repaints
    // nothing between two ticks.
    for ms in 0..90u32 {
        say(&workbench, &id, Some(ms as f32 / 1000.), Some("1/2"), cx);
        cx.executor().advance_clock(Duration::from_millis(1));
        draw(cx);
    }
    assert_eq!(renders(&workbench, cx), before);
    assert_eq!(ticks(cx), ticks_before);

    // Over a second of the fake clock, it repaints on each of 10 ticks.
    for ms in 90..1000u32 {
        say(&workbench, &id, Some(ms as f32 / 1000.), Some("1/2"), cx);
        cx.executor().advance_clock(Duration::from_millis(1));
        draw(cx);
    }
    let ticked = ticks(cx) - ticks_before;
    assert!((9..=10).contains(&ticked), "{ticked} ticks in a second");
    assert!(renders(&workbench, cx) - before >= ticked);

    // Idle, nothing paints.
    operations(&workbench, cx).update(cx, |operations, cx| operations.end(&id, cx));
    draw(cx);
    let idle = renders(&workbench, cx);
    let idle_ticks = ticks(cx);
    for _ in 0..20 {
        cx.executor().advance_clock(Duration::from_millis(100));
        draw(cx);
    }
    assert_eq!(renders(&workbench, cx), idle);
    assert_eq!(ticks(cx), idle_ticks);
}

#[gpui::test]
fn the_accessible_name_reads_label_and_n_more(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    let name = |cx: &mut VisualTestContext| {
        // Every view draws again, so each records its node.
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.refresh();
                window.draw(cx).clear(cx)
            });
        }
        let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
        nodes
            .into_iter()
            .filter(|node| node.role == Role::Button && node.label.starts_with("Indexing"))
            .map(|node| node.label.to_string())
            .next_back()
    };
    begin(&workbench, "Indexing", cx);
    draw(cx);
    assert_eq!(name(cx), Some(item_name("Indexing", 0).to_string()));
    assert_eq!(name(cx), Some("Indexing".to_owned()));

    begin(&workbench, "Checking", cx);
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    assert_eq!(name(cx), Some(item_name("Indexing", 1).to_string()));
    assert_eq!(name(cx), Some("Indexing, 1 more".to_owned()));

    begin(&workbench, "Building", cx);
    cx.executor().advance_clock(Duration::from_millis(100));
    draw(cx);
    assert_eq!(name(cx), Some(item_name("Indexing", 2).to_string()));
    assert_eq!(name(cx), Some("Indexing, 2 more".to_owned()));
}

#[gpui::test]
fn the_last_operation_ending_gives_focus_back(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let id = begin(&workbench, "Indexing", cx);
    draw(cx);
    cx.update(|window, cx| workbench.update(cx, |w, cx| w.show_operations(window, cx)));
    draw(cx);
    assert!(cx.debug_bounds("status-operations-list").is_some());
    let focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| workbench.focus_handle(cx).is_focused(window))
    };
    assert!(!focused(cx));

    operations(&workbench, cx).update(cx, |operations, cx| operations.end(&id, cx));
    draw(cx);
    assert_eq!(cx.debug_bounds("status-operations-list"), None);
    assert!(focused(cx));
}
