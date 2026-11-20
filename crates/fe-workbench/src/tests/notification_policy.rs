//! `workbench.notifications` and Focus mode: which toasts show at once, which
//! go to the inbox alone, and which Focus mode holds until the user is idle.

use std::time::Duration;

use fe_command::CommandId;
use fe_config::NotificationLevel;
use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    workbench::toast::{Toast, ToastLayer, notification_inbox::FOCUS_IDLE, show},
};

fn opened(
    level: NotificationLevel,
    cx: &mut TestAppContext,
) -> (Entity<ToastLayer>, &mut VisualTestContext) {
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
    layer.update(cx, |layer, cx| layer.set_level(level, cx));
    (layer, cx)
}

fn shown(layer: &Entity<ToastLayer>, cx: &mut VisualTestContext) -> Vec<String> {
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn kept(layer: &Entity<ToastLayer>, cx: &mut VisualTestContext) -> Vec<String> {
    layer.read_with(cx, |layer, _| {
        (layer.inbox().all().iter())
            .map(|item| item.title.clone())
            .collect()
    })
}

fn select_all() -> CommandId {
    CommandId::parse("editor.select_all").unwrap()
}

#[gpui::test]
fn needs_you_sends_toasts_without_actions_to_the_inbox(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::NeedsYou, cx);
    cx.update(|_, cx| {
        show(cx, Toast::info("saved"));
        show(
            cx,
            Toast::info("update ready").action("Restart", select_all()),
        );
    });
    assert_eq!(shown(&layer, cx), ["update ready"]);
    assert_eq!(kept(&layer, cx), ["saved", "update ready"]);
}

#[gpui::test]
fn off_sends_everything_to_the_inbox_but_errors_still_show_once(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::Off, cx);
    cx.update(|_, cx| {
        show(cx, Toast::info("saved"));
        show(
            cx,
            Toast::info("update ready").action("Restart", select_all()),
        );
        show(cx, Toast::error("failed"));
    });
    assert_eq!(shown(&layer, cx), ["failed"]);
    assert_eq!(kept(&layer, cx), ["saved", "update ready", "failed"]);
}

#[gpui::test]
fn focus_mode_holds_non_urgent_toasts_until_20_seconds_idle(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::All, cx);
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    cx.update(|_, cx| show(cx, Toast::info("saved")));
    assert_eq!(shown(&layer, cx), Vec::<String>::new());
    assert_eq!(kept(&layer, cx), ["saved"]);

    // Input restarts the 20 seconds.
    cx.executor()
        .advance_clock(FOCUS_IDLE - Duration::from_secs(1));
    cx.run_until_parked();
    layer.update(cx, |layer, cx| layer.note_input(cx));
    cx.executor()
        .advance_clock(FOCUS_IDLE - Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(shown(&layer, cx), Vec::<String>::new());

    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(shown(&layer, cx), ["saved"]);
    assert_eq!(kept(&layer, cx), ["saved"], "shown, but kept once");
}

#[gpui::test]
fn urgent_toasts_show_at_once(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::All, cx);
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    cx.update(|_, cx| {
        show(cx, Toast::info("saved"));
        show(cx, Toast::error("failed"));
        show(
            cx,
            Toast::info("update ready").action("Restart", select_all()),
        );
    });
    assert_eq!(shown(&layer, cx), ["failed", "update ready"]);

    // Turning it off shows what it held.
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    assert_eq!(shown(&layer, cx), ["failed", "update ready", "saved"]);
}

#[gpui::test]
fn identical_held_toasts_release_as_one_notification_that_dismisses_whole(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::All, cx);
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    cx.update(|_, cx| {
        show(cx, Toast::info("saved"));
        show(cx, Toast::info("saved"));
    });
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    assert_eq!(shown(&layer, cx), ["saved"]);
    let counts: Vec<usize> = layer.read_with(cx, |layer, _| {
        layer.inbox().all().iter().map(|item| item.count).collect()
    });
    assert_eq!(counts, [2]);

    let id = layer.read_with(cx, |layer, _| layer.queue().shown()[0].id);
    layer.update(cx, |layer, cx| layer.dismiss(id, cx));
    assert_eq!(
        layer.read_with(cx, |layer, _| layer.inbox().needs_you_count()),
        0
    );
    assert!(
        layer.read_with(cx, |layer, _| layer.inbox().all().iter().all(|item| item
            .state
            != crate::workbench::toast::notification_inbox::State::Open))
    );
}

#[gpui::test]
fn dismissing_the_id_of_a_toast_that_didnt_show_resolves_its_notification(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::NeedsYou, cx);
    let id = layer.update(cx, |layer, cx| layer.push(Toast::info("saved"), cx));
    layer.update(cx, |layer, cx| layer.dismiss(id, cx));
    let states = layer.read_with(cx, |layer, _| {
        (layer.inbox().all().iter())
            .map(|item| item.state)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        states,
        [crate::workbench::toast::notification_inbox::State::Dismissed]
    );
}

#[gpui::test]
fn a_level_set_while_toasts_are_held_applies_when_they_would_show(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::All, cx);
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    cx.update(|_, cx| show(cx, Toast::info("saved")));
    layer.update(cx, |layer, cx| layer.set_level(NotificationLevel::Off, cx));
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    assert_eq!(shown(&layer, cx), Vec::<String>::new());
    assert_eq!(kept(&layer, cx), ["saved"]);
}

#[gpui::test]
fn a_question_with_off_waits_in_the_inbox_to_be_answered(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::Off, cx);
    let answers = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let sink = answers.clone();
    cx.update(|_, cx| {
        show(
            cx,
            Toast::info("Trust?").choices(vec!["Yes".into()], move |c| sink.borrow_mut().push(c)),
        )
    });
    assert_eq!(shown(&layer, cx), Vec::<String>::new());
    let id = layer.read_with(cx, |layer, _| layer.inbox().all()[0].id);
    assert!(layer.update(cx, |layer, cx| layer.answer_notification(id, 0, cx)));
    assert_eq!(*answers.borrow(), [Some(0)]);
}

#[gpui::test]
fn both_handles_of_merged_held_toasts_dismiss_the_one_notification(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::All, cx);
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    let first = layer.update(cx, |layer, cx| layer.push(Toast::info("saved"), cx));
    let second = layer.update(cx, |layer, cx| layer.push(Toast::info("saved"), cx));
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    // More than the inbox keeps, suppressed meanwhile.
    layer.update(cx, |layer, cx| layer.set_level(NotificationLevel::Off, cx));
    for n in 0..300 {
        layer.update(cx, |layer, cx| layer.push(Toast::info(format!("n{n}")), cx));
    }
    layer.update(cx, |layer, cx| layer.dismiss(second, cx));
    assert_eq!(shown(&layer, cx), Vec::<String>::new());
    let _ = first;
}

#[gpui::test]
fn an_inbox_only_action_keeps_its_arguments(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::Off, cx);
    cx.update(|_, cx| {
        show(
            cx,
            Toast::info("Lane done")
                .action("Open", select_all())
                .action_args(serde_json::json!({"lane": 2})),
        )
    });
    let args = layer.read_with(cx, |layer, _| layer.inbox().all()[0].action_args.clone());
    assert_eq!(args, Some(serde_json::json!({"lane": 2})));
}

#[gpui::test]
fn scrolling_restarts_the_idle_period(cx: &mut TestAppContext) {
    let (layer, cx) = opened(NotificationLevel::All, cx);
    layer.update(cx, |layer, cx| layer.toggle_focus_mode(cx));
    cx.update(|_, cx| show(cx, Toast::info("saved")));
    cx.executor()
        .advance_clock(FOCUS_IDLE - Duration::from_secs(1));
    cx.run_until_parked();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: gpui::point(gpui::px(100.), gpui::px(100.)),
        delta: gpui::ScrollDelta::Lines(gpui::point(0., -1.)),
        ..Default::default()
    });
    // Without the scroll it would have shown a second ago, and be gone by
    // the time the new 20 seconds are up.
    cx.executor()
        .advance_clock(FOCUS_IDLE - Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(shown(&layer, cx), Vec::<String>::new());
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(shown(&layer, cx), ["saved"]);
}

#[test]
fn the_bell_is_slashed_while_focus_mode_is_on() {
    use crate::status_bar::bell_item;
    assert_eq!(bell_item(0, false).icon, Some(fe_ui::Icon::Bell));
    assert_eq!(bell_item(0, true).icon, Some(fe_ui::Icon::BellOff));
}
