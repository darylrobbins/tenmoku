//! The notification inbox: every toast a window shows is kept, split into
//! Needs you and Earlier.

use std::{cell::RefCell, rc::Rc, time::SystemTime};

use fe_command::CommandId;
use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    workbench::toast::{
        Toast, ToastId, ToastLayer,
        notification_inbox::{CAP, NotificationInbox, NotificationSource, Resolution, State},
        show,
    },
};

fn opened(cx: &mut TestAppContext) -> (Entity<ToastLayer>, &mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts.clone());
    (layer, cx)
}

fn titles<'a>(
    items: impl Iterator<Item = &'a crate::workbench::toast::notification_inbox::Notification>,
) -> Vec<String> {
    items.map(|item| item.title.clone()).collect()
}

fn select_all() -> CommandId {
    CommandId::parse("editor.select_all").unwrap()
}

#[gpui::test]
fn every_pushed_toast_is_recorded(cx: &mut TestAppContext) {
    let (layer, cx) = opened(cx);
    cx.update(|_, cx| {
        show(
            cx,
            Toast::info("saved")
                .source(NotificationSource::Git)
                .detail("a.txt"),
        );
        show(cx, Toast::error("failed"));
        // Past what the queue shows at once: kept all the same.
        for n in 0..5 {
            show(cx, Toast::warning(format!("w{n}")));
        }
    });
    layer.read_with(cx, |layer, _| {
        let all = layer.inbox().all();
        assert_eq!(all.len(), 7);
        assert_eq!(all[0].title, "saved");
        assert_eq!(all[0].source, NotificationSource::Git);
        assert_eq!(all[0].detail.as_deref(), Some("a.txt"));
        assert_eq!(all[1].source, NotificationSource::App);
        assert_eq!(all[1].count, 1);
        assert_eq!(layer.inbox().needs_you_count(), 0);
    });
    // The toasts go; the record stays.
    layer.update(cx, |layer, cx| layer.dismiss_all(cx));
    layer.read_with(cx, |layer, _| assert_eq!(layer.inbox().all().len(), 7));
}

#[gpui::test]
fn merged_duplicates_merge_in_the_inbox(cx: &mut TestAppContext) {
    let (layer, cx) = opened(cx);
    cx.update(|_, cx| {
        show(cx, Toast::info("saved"));
        show(cx, Toast::info("saved"));
        show(cx, Toast::info("saved"));
        show(cx, Toast::info("other"));
    });
    layer.read_with(cx, |layer, _| {
        let all = layer.inbox().all();
        assert_eq!(titles(all.iter()), ["saved", "other"]);
        assert_eq!(all[0].count, 3);
        assert_eq!(all[1].count, 1);
    });
    // Once the toast is gone, an identical one is a new notification.
    layer.update(cx, |layer, cx| layer.dismiss_all(cx));
    cx.update(|_, cx| show(cx, Toast::info("saved")));
    layer.read_with(cx, |layer, _| assert_eq!(layer.inbox().all().len(), 3));
}

#[gpui::test]
fn needs_you_while_it_has_an_action(cx: &mut TestAppContext) {
    let (layer, cx) = opened(cx);
    cx.update(|_, cx| {
        show(cx, Toast::info("plain"));
        show(cx, Toast::info("update").action("Restart", select_all()));
        show(
            cx,
            Toast::info("which?").choices(vec!["A".into(), "B".into()], |_| {}),
        );
    });
    layer.read_with(cx, |layer, _| {
        let inbox = layer.inbox();
        assert_eq!(titles(inbox.needs_you()), ["update", "which?"]);
        assert_eq!(titles(inbox.earlier()), ["plain"]);
        assert_eq!(inbox.needs_you_count(), 2);
    });
    // Its toast timing out doesn't resolve it.
    cx.executor()
        .advance_clock(crate::workbench::toast::LIFETIME * 2);
    cx.run_until_parked();
    layer.read_with(cx, |layer, _| {
        assert!(
            layer
                .queue()
                .shown()
                .iter()
                .all(|e| e.toast.choices.is_some())
        );
        assert_eq!(layer.inbox().needs_you_count(), 2);
    });
}

#[gpui::test]
fn acting_or_dismissing_moves_it_to_earlier(cx: &mut TestAppContext) {
    let (layer, cx) = opened(cx);
    let ids: Rc<RefCell<Vec<ToastId>>> = Rc::default();
    cx.update(|_, cx| {
        show(cx, Toast::info("one").action("Do", select_all()));
        show(cx, Toast::info("two").action("Do", select_all()));
        show(cx, Toast::info("three").action("Do", select_all()));
    });
    layer.update(cx, |layer, _| {
        *ids.borrow_mut() = layer.queue().shown().iter().map(|e| e.id).collect();
    });
    layer.update(cx, |layer, cx| layer.dismiss(ids.borrow()[0], cx));
    layer.read_with(cx, |layer, _| {
        assert_eq!(titles(layer.inbox().needs_you()), ["two", "three"]);
        assert_eq!(layer.inbox().all()[0].state, State::Dismissed);
    });
    layer.update(cx, |layer, cx| layer.dismiss_all(cx));
    layer.read_with(cx, |layer, _| {
        assert_eq!(layer.inbox().needs_you_count(), 0);
        assert_eq!(titles(layer.inbox().earlier()), ["one", "two", "three"]);
    });
}

#[test]
fn acting_on_a_notification_by_id_moves_it() {
    let mut inbox = NotificationInbox::default();
    let toast = Toast::info("x").action("Do", select_all());
    let id = inbox.record(&toast, queued(0), false, SystemTime::now());
    assert_eq!(inbox.needs_you_count(), 1);
    assert!(inbox.resolve(id, Resolution::Acted));
    assert!(!inbox.resolve(id, Resolution::Dismissed), "once");
    assert_eq!(inbox.needs_you_count(), 0);
    assert_eq!(inbox.all()[0].state, State::Acted);
}

#[gpui::test]
fn answering_choices_moves_it(cx: &mut TestAppContext) {
    let (layer, cx) = opened(cx);
    let answers: Rc<RefCell<Vec<Option<usize>>>> = Rc::default();
    cx.update(|_, cx| {
        let sink = answers.clone();
        show(
            cx,
            Toast::info("which?").choices(vec!["A".into(), "B".into()], move |c| {
                sink.borrow_mut().push(c);
            }),
        );
    });
    // The inbox keeps no handle on the question: it doesn't hold the reply.
    layer.update(cx, |layer, cx| {
        let entry = layer.queue().shown()[0].clone();
        let (id, choices) = (entry.id, entry.toast.choices.clone().unwrap());
        drop(entry);
        assert_eq!(layer.inbox().needs_you_count(), 1);
        choices.choose(1);
        layer.dismiss(id, cx);
    });
    assert_eq!(*answers.borrow(), [Some(1)]);
    layer.read_with(cx, |layer, _| {
        assert_eq!(layer.inbox().needs_you_count(), 0);
        assert_eq!(layer.inbox().all()[0].state, State::Dismissed);
    });
    // A direct answer records as answered.
    let mut inbox = NotificationInbox::default();
    let toast = Toast::info("q").choices(vec!["A".into()], |_| {});
    let id = inbox.record(&toast, queued(0), false, SystemTime::now());
    assert!(inbox.resolve(id, Resolution::Answered));
    assert_eq!(inbox.all()[0].state, State::Answered);
    assert_eq!(inbox.needs_you_count(), 0);
}

#[test]
fn the_cap_of_200_drops_the_oldest_earlier_first() {
    let mut inbox = NotificationInbox::default();
    let at = SystemTime::now();
    // The oldest needs you; the rest are Earlier.
    inbox.record(
        &Toast::info("ask").action("Do", select_all()),
        queued(0),
        false,
        at,
    );
    for n in 1..CAP {
        inbox.record(&Toast::info(format!("n{n}")), queued(n as u64), false, at);
    }
    assert_eq!(inbox.all().len(), CAP);
    inbox.record(&Toast::info("newest"), queued(999), false, at);
    assert_eq!(inbox.all().len(), CAP);
    let titles = titles(inbox.all().iter());
    assert_eq!(
        titles[0], "ask",
        "a notification that needs you outlasts older Earlier ones"
    );
    assert_eq!(titles[1], "n2", "n1, the oldest Earlier, went");
    assert_eq!(titles.last().unwrap(), "newest");
    assert_eq!(inbox.needs_you_count(), 1);
}

/// A queue toast id, as the queue hands them out.
fn queued(n: u64) -> ToastId {
    let mut queue = crate::workbench::toast::ToastQueue::default();
    let now = std::time::Instant::now();
    let mut id = queue.push(Toast::error("0"), now);
    for i in 1..=n {
        id = queue.push(Toast::error(format!("{i}")), now);
    }
    id
}

#[gpui::test]
fn a_toast_dropped_from_a_full_queue_is_dismissed_in_the_inbox(cx: &mut TestAppContext) {
    let (layer, cx) = opened(cx);
    let answers: Rc<RefCell<Vec<Option<usize>>>> = Rc::default();
    cx.update(|_, cx| {
        // Fills the shown slots first, so the question waits, then is
        // pushed out by what follows.
        for n in 0..crate::workbench::toast::MAX_SHOWN {
            show(cx, Toast::error(format!("e{n}")));
        }
        let sink = answers.clone();
        show(
            cx,
            Toast::info("which?").choices(vec!["A".into()], move |c| sink.borrow_mut().push(c)),
        );
        for n in 0..crate::workbench::toast::MAX_WAITING {
            show(cx, Toast::error(format!("w{n}")));
        }
    });
    assert_eq!(*answers.borrow(), [None]);
    layer.read_with(cx, |layer, _| {
        let asked = (layer.inbox().all().iter())
            .find(|item| item.title == "which?")
            .expect("kept");
        assert_eq!(asked.state, State::Dismissed);
        assert!(!asked.needs_you());
    });
}

#[test]
fn empty_choices_do_not_need_you() {
    let mut inbox = NotificationInbox::default();
    let at = SystemTime::now();
    let empty = || Toast::info("q").choices(Vec::new(), |_| {});
    inbox.record(&empty(), queued(0), false, at);
    assert_eq!(inbox.needs_you_count(), 0);
    inbox.record(&empty().action("Do", select_all()), queued(1), false, at);
    assert_eq!(inbox.needs_you_count(), 1);
}

#[test]
fn a_merged_notification_is_the_newest_for_order_and_the_cap() {
    let mut inbox = NotificationInbox::default();
    let at = SystemTime::now();
    let first = queued(0);
    inbox.record(&Toast::info("again"), first, false, at);
    for n in 1..CAP {
        inbox.record(&Toast::info(format!("n{n}")), queued(n as u64), false, at);
    }
    inbox.record(&Toast::info("again"), first, true, at);
    assert_eq!(inbox.all().len(), CAP);
    assert_eq!(inbox.all().last().unwrap().title, "again");
    assert_eq!(inbox.all().last().unwrap().count, 2);
    // The cap drops the oldest by last raised: n1, not the merged one.
    inbox.record(&Toast::info("newest"), queued(999), false, at);
    let all = titles(inbox.all().iter());
    assert_eq!(all[0], "n2");
    assert!(all.contains(&"again".to_owned()));
}
