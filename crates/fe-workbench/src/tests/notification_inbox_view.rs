//! The notification bell at the right end of the status bar and the inbox
//! popup it opens (`notification_inbox_view.rs`): the bell's dot while
//! something needs you and its counted accessible name; the popup's Needs
//! you and Earlier groups, a row's action, answers and dismiss, Clear
//! earlier, its keys, the selection following its notification, and ages
//! that count on. Notifications are numbered from 0 in the order raised,
//! which names their rows' elements (`inbox-action-{n}`).

use std::{cell::RefCell, fs, rc::Rc, time::Duration};

use fe_buffer::Buffer;
use fe_command::{CommandId, Invocation, Source};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, SharedString, TestAppContext, VisualTestContext};
use kiln_l10n::tr;
use kiln_shell::StatusSide;

use super::open;
use crate::{
    Launch, StatusItem, Workbench,
    workbench::{
        notification_inbox_view::{Group, NotificationInboxView},
        toast::{Toast, notification_inbox::NotificationSource, show},
    },
};

fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, _| window.activate_window());
    draw(cx);
    // Where a window's focus is: its editor.
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx)
        });
    }
}

/// Runs command `id` from where focus is, as its key would.
fn run(id: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context = kiln_platform::keys::context_stack(&window.context_stack(), cx);
        let invocation = Invocation::of(id).unwrap().context(context);
        window.dispatch_action(Box::new(RunCommand(invocation)), cx);
    });
    draw(cx);
}

fn inbox(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<NotificationInboxView> {
    workbench.read_with(cx, |workbench, _| workbench.inbox.clone())
}

/// Each row the popup holds: its group and title, as drawn.
fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Group, String)> {
    let inbox = inbox(workbench, cx);
    inbox.read_with(cx, |inbox, cx| {
        (inbox.rows(cx).into_iter())
            .map(|(group, item)| (group, item.title.clone()))
            .collect()
    })
}

fn toggle_breadcrumbs() -> CommandId {
    CommandId::parse("view.toggle_breadcrumbs").unwrap()
}

/// A toast that needs you: one with an action.
fn needs_you(title: &str) -> Toast {
    Toast::error(title).action("Show", toggle_breadcrumbs())
}

fn dot(cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds("status-dot:notifications").is_some()
}

#[gpui::test]
fn the_bell_dot_shows_only_with_needs_you_items(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    // Last on the right, with no text, from the start.
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let right = bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Right));
    let last = right.last().expect("items on the right");
    assert_eq!(
        (last.0.as_ref(), last.1.as_ref()),
        (StatusItem::NOTIFICATIONS, "")
    );
    let bell = cx
        .debug_bounds("status-item:notifications")
        .expect("the bell is drawn");
    assert!(cx.debug_bounds("status-icon:notifications").is_some());
    assert!(!dot(cx));

    // Nothing to do about it: no dot.
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    assert!(!dot(cx));

    cx.update(|_, cx| show(cx, needs_you("Build failed")));
    draw(cx);
    let at = cx
        .debug_bounds("status-dot:notifications")
        .expect("the dot is drawn");
    assert_eq!(at.size.width, gpui::px(6.));
    assert!(bell.contains(&at.origin), "{at:?} on {bell:?}");

    // Dismissed, from its toast: it no longer needs you.
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts.clone());
    layer.update(cx, |layer, cx| layer.dismiss_all(cx));
    draw(cx);
    assert!(!dot(cx));
    let kept = layer.read_with(cx, |layer, _| layer.inbox().all().len());
    assert_eq!(kept, 2);
}

#[gpui::test]
fn the_popup_groups_rows_and_runs_a_rows_action(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.update(|_, cx| {
        show(cx, Toast::info("Saved"));
        show(
            cx,
            needs_you("Build failed")
                .source(NotificationSource::Task)
                .detail("2 tests failed"),
        );
        show(
            cx,
            Toast::info("Lane 2 is done")
                .source(NotificationSource::Agent)
                .action("Review", toggle_breadcrumbs()),
        );
    });
    draw(cx);
    assert!(cx.debug_bounds("notification-inbox").is_none());
    assert!(cx.debug_bounds("toast-0").is_some());

    // The bell's click runs `notifications.show`.
    let bell = cx.debug_bounds("status-item:notifications").unwrap();
    cx.simulate_click(bell.center(), gpui::Modifiers::none());
    draw(cx);
    let popup = cx
        .debug_bounds("notification-inbox")
        .expect("the popup is open");
    // Above the bell, over the toasts it holds as rows.
    assert!(popup.bottom() <= bell.top(), "{popup:?} over {bell:?}");
    assert_eq!(
        popup.right(),
        bell.right() - gpui::px(8.),
        "{popup:?} {bell:?}"
    );
    assert!(cx.debug_bounds("toast-0").is_none());
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::NeedsYou, "Lane 2 is done".to_owned()),
            (Group::NeedsYou, "Build failed".to_owned()),
            (Group::Earlier, "Saved".to_owned()),
        ]
    );
    // Saved is 0, Build failed 1, Lane 2 is done 2.
    for selector in [
        "inbox-header-needs-you",
        "inbox-header-earlier",
        "inbox-action-2",
        "inbox-action-1",
        "icon-button:inbox-dismiss-0",
    ] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector}");
    }
    // Earlier rows have no action.
    assert!(cx.debug_bounds("inbox-action-0").is_none());
    // The agent's row in the agent's colour, the task's in its level's.
    let colors = cx.update(|_, cx| fe_ui::ActiveTheme::theme(cx).colors);
    let (_, agent) = crate::workbench::notification_inbox_view::source_icon(
        NotificationSource::Agent,
        crate::workbench::toast::Level::Info,
        &colors,
    );
    assert_eq!(agent, colors.agent.base);
    assert_ne!(agent, colors.accent.base);

    // The second row's action, clicked: it runs from the editor, where
    // focus was, the popup closes and the row moves to Earlier.
    let action = cx.debug_bounds("inbox-action-1").unwrap();
    cx.simulate_click(action.center(), gpui::Modifiers::none());
    draw(cx);
    let ran = workbench.read_with(cx, |workbench, _| workbench.commands_run.last().cloned());
    let ran = ran.expect("the action ran");
    assert_eq!(ran.command, toggle_breadcrumbs());
    assert_eq!(ran.source, Source::Mouse);
    assert!(
        ran.context
            .frames()
            .iter()
            .any(|frame| frame.name == "editor")
    );
    assert!(cx.debug_bounds("notification-inbox").is_none());
    run("notifications.show", cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::NeedsYou, "Lane 2 is done".to_owned()),
            (Group::Earlier, "Build failed".to_owned()),
            (Group::Earlier, "Saved".to_owned()),
        ]
    );

    // Dismissed: a Needs-you row moves to Earlier, an Earlier one goes.
    let dismiss = cx.debug_bounds("icon-button:inbox-dismiss-2").unwrap();
    cx.simulate_click(dismiss.center(), gpui::Modifiers::none());
    draw(cx);
    let dismiss = cx.debug_bounds("icon-button:inbox-dismiss-0").unwrap();
    cx.simulate_click(dismiss.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::Earlier, "Lane 2 is done".to_owned()),
            (Group::Earlier, "Build failed".to_owned()),
        ]
    );
    assert!(!dot(cx));

    // `notifications.show` again closes it.
    run("notifications.show", cx);
    assert!(cx.debug_bounds("notification-inbox").is_none());
}

#[gpui::test]
fn escape_closes_and_arrows_move(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.update(|_, cx| {
        show(cx, Toast::info("Saved"));
        show(cx, needs_you("First"));
        show(cx, needs_you("Second"));
    });
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    let inbox = inbox(&workbench, cx);
    // The selected row's place.
    let selected = |cx: &mut VisualTestContext| {
        inbox.read_with(cx, |inbox, cx| {
            let rows = inbox.rows(cx);
            (rows.iter()).position(|(_, item)| Some(item.id) == inbox.selected())
        })
    };

    run("notifications.show", cx);
    assert!(inbox.read_with(cx, |inbox, _| inbox.is_open()));
    assert_eq!(selected(cx), Some(0));
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(selected(cx), Some(1));
    // Down past the last stays there; up past the first too.
    cx.simulate_keystrokes("down down down");
    draw(cx);
    assert_eq!(selected(cx), Some(2));
    cx.simulate_keystrokes("up up up up");
    draw(cx);
    assert_eq!(selected(cx), Some(0));
    // The editor didn't move its caret meanwhile: the keys were the popup's.
    let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "fn a() {}\n");

    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!inbox.read_with(cx, |inbox, _| inbox.is_open()));
    assert!(cx.debug_bounds("notification-inbox").is_none());
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // Enter runs the selected row's action from the keyboard: the newest,
    // first.
    run("notifications.show", cx);
    cx.simulate_keystrokes("down up enter");
    draw(cx);
    let ran = workbench.read_with(cx, |workbench, _| workbench.commands_run.last().cloned());
    let ran = ran.expect("the action ran");
    assert_eq!(ran.command, toggle_breadcrumbs());
    assert_eq!(ran.source, Source::Keyboard);
    assert!(!inbox.read_with(cx, |inbox, _| inbox.is_open()));
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::NeedsYou, "First".to_owned()),
            (Group::Earlier, "Second".to_owned()),
            (Group::Earlier, "Saved".to_owned()),
        ]
    );

    // Focus going elsewhere closes it.
    run("notifications.show", cx);
    assert!(inbox.read_with(cx, |inbox, _| inbox.is_open()));
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    assert!(!inbox.read_with(cx, |inbox, _| inbox.is_open()));
}

#[gpui::test]
fn the_accessible_name_counts_with_plurals(cx: &mut TestAppContext) {
    let (_dir, _workbench, cx) = opened(cx);
    let named = |cx: &mut VisualTestContext| -> Vec<SharedString> {
        let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
        (nodes.iter())
            .filter(|node| node.role == fe_ui::Role::Button)
            .filter(|node| node.id.to_string().contains("status-marked:notifications"))
            .map(|node| node.label.clone())
            .collect()
    };
    let bell = |count: usize| tr!("workbench-inbox-bell", count = count);
    // Three messages: none, one and more, each its own.
    assert_ne!(bell(0), bell(1));
    assert_ne!(bell(1), bell(2));
    assert!(!bell(0).contains('0'), "{}", bell(0));
    assert!(bell(2).contains('2'), "{}", bell(2));

    assert_eq!(named(cx), [bell(0)]);
    cx.update(|_, cx| show(cx, needs_you("First")));
    draw(cx);
    assert_eq!(named(cx), [bell(1)]);
    cx.update(|_, cx| show(cx, needs_you("Second")));
    draw(cx);
    assert_eq!(named(cx), [bell(2)]);
    // The popup is named too.
    run("notifications.show", cx);
    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    assert!(
        (nodes.iter())
            .any(|node| node.role == fe_ui::Role::Dialog
                && node.label == tr!("workbench-inbox-title")),
        "{nodes:?}"
    );
}

#[gpui::test]
fn clear_earlier_empties_that_group(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.update(|_, cx| {
        show(cx, Toast::info("Saved"));
        show(cx, Toast::warning("Slow"));
        show(cx, needs_you("Build failed"));
    });
    draw(cx);
    run("notifications.show", cx);
    assert_eq!(rows(&workbench, cx).len(), 3);
    let button = cx
        .debug_bounds("inbox-clear-earlier")
        .expect("the Clear earlier button");
    cx.simulate_click(button.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(
        rows(&workbench, cx),
        [(Group::NeedsYou, "Build failed".to_owned())]
    );
    assert!(cx.debug_bounds("inbox-header-earlier").is_none());
    assert!(dot(cx));

    // With nothing left, it says so. Build failed is 2.
    let dismiss = cx.debug_bounds("icon-button:inbox-dismiss-2").unwrap();
    cx.simulate_click(dismiss.center(), gpui::Modifiers::none());
    draw(cx);
    let button = cx.debug_bounds("inbox-clear-earlier").unwrap();
    cx.simulate_click(button.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(rows(&workbench, cx), []);
    assert!(cx.debug_bounds("inbox-empty").is_some());
    assert!(!dot(cx));
}

/// Presses each of `keys` in turn, down and up, drawing after each: a
/// focused button clicks on Enter's release.
fn press(cx: &mut VisualTestContext, keys: &str) {
    for key in keys.split_whitespace() {
        cx.simulate_keystrokes(key);
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
        });
        draw(cx);
    }
}

fn layer(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<crate::workbench::toast::ToastLayer> {
    workbench.read_with(cx, |workbench, _| workbench.toasts.clone())
}

/// The toasts the queue holds: those shown, by message, and how many wait.
fn queued(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (Vec<String>, usize) {
    let layer = layer(workbench, cx);
    layer.read_with(cx, |layer, _| {
        let queue = layer.queue();
        let shown = (queue.shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect();
        (shown, queue.waiting())
    })
}

fn is_open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let inbox = inbox(workbench, cx);
    inbox.read_with(cx, |inbox, _| inbox.is_open())
}

fn commands_run(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.commands_run.len())
}

/// Enter goes to a button that has keyboard focus, never to the selected
/// row: Tab reaches a row's buttons, Dismiss and Clear earlier among them.
#[gpui::test]
fn a_focused_button_takes_enter_itself(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.update(|_, cx| {
        show(cx, needs_you("First"));
        show(cx, needs_you("Second"));
        show(cx, Toast::info("Saved"));
    });
    draw(cx);
    run("notifications.show", cx);
    let before = commands_run(&workbench, cx);

    // Tab: the selected row's (Second's) action, then its dismiss, pressed.
    press(cx, "tab tab enter");
    assert_eq!(commands_run(&workbench, cx), before, "no action ran");
    assert!(is_open(&workbench, cx));
    // Earlier is newest first by when each was raised: Saved after Second.
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::NeedsYou, "First".to_owned()),
            (Group::Earlier, "Saved".to_owned()),
            (Group::Earlier, "Second".to_owned()),
        ]
    );

    // Focus is back on the popup: Tab starts at the selected row (Second,
    // now in Earlier, its dismiss), and Shift-Tab goes back past Saved's
    // dismiss and Clear earlier to First's dismiss and action, which Enter
    // presses.
    press(cx, "tab shift-tab shift-tab shift-tab shift-tab enter");
    let ran = workbench.read_with(cx, |workbench, _| workbench.commands_run.last().cloned());
    let ran = ran.expect("First's action ran");
    assert_eq!(ran.command, toggle_breadcrumbs());
    assert_eq!(ran.source, Source::Keyboard);
    assert!(!is_open(&workbench, cx));
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::Earlier, "Saved".to_owned()),
            (Group::Earlier, "Second".to_owned()),
            (Group::Earlier, "First".to_owned()),
        ]
    );

    // Clear earlier from the keyboard: the first row's dismiss, then back
    // to the button before it.
    run("notifications.show", cx);
    press(cx, "tab shift-tab enter");
    assert!(is_open(&workbench, cx));
    assert_eq!(rows(&workbench, cx), []);
    assert!(cx.debug_bounds("inbox-empty").is_some());
    // The popup still has focus: Escape closes it.
    press(cx, "escape");
    assert!(!is_open(&workbench, cx));
}

/// The replies questions got: which question, and the answer.
type Replies = Rc<RefCell<Vec<(&'static str, Option<usize>)>>>;

/// A question's answers are its row's buttons: clicking one, or Enter on
/// a question of one, answers it; Enter on a question of more moves focus
/// to its first answer.
#[gpui::test]
fn a_question_is_answered_from_its_row(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    let got: Replies = Rc::default();
    cx.update(|_, cx| {
        let sink = got.clone();
        show(
            cx,
            Toast::warning("Tests failed").choices(vec!["Show".into()], move |c| {
                sink.borrow_mut().push(("one", c))
            }),
        );
        let sink = got.clone();
        show(
            cx,
            Toast::info("Save changes?").choices(vec!["Save".into(), "Discard".into()], move |c| {
                sink.borrow_mut().push(("two", c))
            }),
        );
    });
    draw(cx);
    run("notifications.show", cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::NeedsYou, "Save changes?".to_owned()),
            (Group::NeedsYou, "Tests failed".to_owned()),
        ]
    );
    for selector in ["inbox-answer-0-0", "inbox-answer-1-0", "inbox-answer-1-1"] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector}");
    }

    // Clicked: the reply has it, the toast goes, the row moves to Earlier.
    let show_output = cx.debug_bounds("inbox-answer-0-0").unwrap();
    cx.simulate_click(show_output.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(*got.borrow(), [("one", Some(0))]);
    assert!(!is_open(&workbench, cx));
    assert_eq!(
        queued(&workbench, cx),
        (vec!["Save changes?".to_owned()], 0)
    );

    // Enter on a question of two: focus to its first answer, nothing
    // answered; Tab to the second, Enter answers with it.
    run("notifications.show", cx);
    press(cx, "enter");
    assert!(is_open(&workbench, cx));
    assert_eq!(got.borrow().len(), 1);
    press(cx, "tab enter");
    assert_eq!(*got.borrow(), [("one", Some(0)), ("two", Some(1))]);
    assert_eq!(queued(&workbench, cx), (Vec::new(), 0));
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::Earlier, "Save changes?".to_owned()),
            (Group::Earlier, "Tests failed".to_owned()),
        ]
    );
    assert!(!dot(cx));

    // Enter on a question of one answers it.
    cx.update(|_, cx| {
        let sink = got.clone();
        show(
            cx,
            Toast::info("Reload?").choices(vec!["Reload".into()], move |c| {
                sink.borrow_mut().push(("three", c))
            }),
        );
    });
    draw(cx);
    run("notifications.show", cx);
    press(cx, "enter");
    assert_eq!(got.borrow().last(), Some(&("three", Some(0))));
    assert_eq!(queued(&workbench, cx), (Vec::new(), 0));
}

/// The selection is a notification: it stays on it as others come, merge
/// and move, and falls to the row in its place when it goes.
#[gpui::test]
fn the_selection_follows_its_notification(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.update(|_, cx| {
        show(cx, needs_you("First"));
        show(cx, needs_you("Second"));
    });
    draw(cx);
    let inbox = inbox(&workbench, cx);
    let selected = |cx: &mut VisualTestContext| {
        inbox.read_with(cx, |inbox, _| inbox.selected().map(|id| id.value()))
    };
    run("notifications.show", cx);
    // First, the second row.
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(selected(cx), Some(0));

    // A new one comes, and Second again, merged and moved to the top.
    cx.update(|_, cx| {
        show(cx, needs_you("Third"));
        show(cx, needs_you("Second"));
    });
    draw(cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::NeedsYou, "Second".to_owned()),
            (Group::NeedsYou, "Third".to_owned()),
            (Group::NeedsYou, "First".to_owned()),
        ]
    );
    assert_eq!(selected(cx), Some(0));
    // Its row's element moved with it: now the lowest.
    let first = cx.debug_bounds("inbox-row-0").expect("First's row");
    let third = cx.debug_bounds("inbox-row-2").expect("Third's row");
    assert!(first.top() > third.top(), "{first:?} under {third:?}");

    // Enter acts on it, First, not on the row that was second.
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        rows(&workbench, cx),
        [
            (Group::NeedsYou, "Second".to_owned()),
            (Group::NeedsYou, "Third".to_owned()),
            (Group::Earlier, "First".to_owned()),
        ]
    );

    // Gone: the row in its place is selected.
    run("notifications.show", cx);
    cx.simulate_keystrokes("down down");
    draw(cx);
    assert_eq!(selected(cx), Some(0));
    let dismiss = cx.debug_bounds("icon-button:inbox-dismiss-0").unwrap();
    cx.simulate_click(dismiss.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(rows(&workbench, cx).len(), 2);
    assert_eq!(selected(cx), Some(2), "Third, in the last place");
}

/// Dismissing an Earlier row, or Clear earlier, takes its toasts too,
/// shown or waiting: an error without an action stays until dismissed.
#[gpui::test]
fn dismissing_earlier_takes_its_toasts(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.update(|_, cx| {
        for message in ["Disk full", "No network", "e2", "e3", "e4"] {
            show(cx, Toast::error(message));
        }
    });
    draw(cx);
    assert_eq!(
        queued(&workbench, cx),
        (
            vec!["Disk full".into(), "No network".into(), "e2".into()],
            2
        )
    );
    run("notifications.show", cx);
    let dismiss = cx.debug_bounds("icon-button:inbox-dismiss-0").unwrap();
    cx.simulate_click(dismiss.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(rows(&workbench, cx).len(), 4);
    assert_eq!(
        queued(&workbench, cx),
        (vec!["No network".into(), "e2".into(), "e3".into()], 1)
    );
    let clear = cx.debug_bounds("inbox-clear-earlier").unwrap();
    cx.simulate_click(clear.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(rows(&workbench, cx), []);
    assert_eq!(queued(&workbench, cx), (Vec::new(), 0));
    // Closed, no toast comes back.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(cx.debug_bounds("toast-1").is_none());
}

/// An open inbox's ages count on by themselves, redrawn as the first of
/// them steps on; a closed one doesn't wake.
#[gpui::test]
fn ages_count_on_while_open(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.update(|_, cx| show(cx, Toast::info("Saved")));
    draw(cx);
    let inbox = inbox(&workbench, cx);
    let ages = |cx: &mut VisualTestContext| {
        inbox.read_with(cx, |inbox, _| {
            let (ages, ticks) = inbox.drawn_ages();
            (
                ages.iter().map(|(_, age)| age.clone()).collect::<Vec<_>>(),
                ticks,
            )
        })
    };
    run("notifications.show", cx);
    assert_eq!(ages(cx), (vec![tr!("workbench-inbox-age-now")], 0));

    cx.executor().advance_clock(Duration::from_secs(30));
    cx.run_until_parked();
    assert_eq!(ages(cx).1, 0, "no redraw before the minute");
    cx.executor().advance_clock(Duration::from_secs(31));
    cx.run_until_parked();
    let (_, ticks) = ages(cx);
    assert!(ticks >= 1, "redrawn at the minute");
    draw(cx);
    assert_eq!(ages(cx).0, [tr!("workbench-inbox-age-minutes", count = 1)]);

    cx.simulate_keystrokes("escape");
    draw(cx);
    let (_, closed) = ages(cx);
    cx.executor().advance_clock(Duration::from_secs(3_600));
    cx.run_until_parked();
    assert_eq!(ages(cx).1, closed, "no ticking while closed");
}
