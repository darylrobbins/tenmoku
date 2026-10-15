//! Toasts: the queue and its timers, with a fake clock; a window's layer,
//! its close buttons, `toast.dismiss_all` and actions; and focus and vim's
//! mode, which nothing a toast does moves.

use std::time::{Duration, Instant};

use fe_buffer::Buffer;
use fe_command::{CommandId, ContextFrame, ContextStack, Invocation, Source};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand, set_vim_enabled};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent,
    TestAppContext, VisualTestContext, WindowHandle, point, px, size,
};

use super::{LIFETIME, MAX_SHOWN, MAX_WAITING, Toast, ToastQueue, show};
use crate::{Launch, Workbench};

/// What each shown toast says, top first, with its count.
fn shown(queue: &ToastQueue) -> Vec<(String, usize)> {
    (queue.shown().iter())
        .map(|entry| (entry.toast.message.clone(), entry.count))
        .collect()
}

mod queue {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn at_most_three_show_and_more_wait() {
        let (mut queue, t0) = (ToastQueue::default(), Instant::now());
        let ids: Vec<_> = (0..5)
            .map(|n| queue.push(Toast::error(format!("{n}")), t0))
            .collect();
        assert_eq!(MAX_SHOWN, 3);
        assert_eq!(
            shown(&queue),
            [("0".into(), 1), ("1".into(), 1), ("2".into(), 1)]
        );
        assert_eq!(queue.waiting(), 2);
        // Dismissing one makes room for the next, at the bottom.
        assert!(queue.dismiss(ids[1], t0));
        assert_eq!(
            shown(&queue),
            [("0".into(), 1), ("2".into(), 1), ("3".into(), 1)]
        );
        assert_eq!(queue.waiting(), 1);
        // A waiting one can be dismissed too, and a gone one isn't there.
        assert!(queue.dismiss(ids[4], t0));
        assert!(!queue.dismiss(ids[4], t0));
        assert_eq!(queue.waiting(), 0);
        assert!(queue.dismiss_all());
        assert!(queue.shown().is_empty());
        assert!(!queue.dismiss_all());
    }

    #[test]
    fn past_the_waiting_limit_the_oldest_waiting_is_dropped() {
        let (mut queue, t0) = (ToastQueue::default(), Instant::now());
        for n in 0..MAX_SHOWN + MAX_WAITING + 2 {
            queue.push(Toast::error(format!("{n}")), t0);
        }
        assert_eq!(queue.waiting(), MAX_WAITING);
        queue.dismiss(queue.shown()[0].id, t0);
        let last = shown(&queue).pop().unwrap();
        assert_eq!(last, (format!("{}", MAX_SHOWN + 2), 1));
    }

    #[test]
    fn an_identical_shown_toast_counts_up() {
        let (mut queue, t0) = (ToastQueue::default(), Instant::now());
        let first = queue.push(Toast::warning("disk full"), t0);
        queue.push(Toast::info("saved"), t0);
        assert_eq!(queue.push(Toast::warning("disk full"), t0), first);
        assert_eq!(queue.push(Toast::warning("disk full"), t0), first);
        assert_eq!(
            shown(&queue),
            [("disk full".into(), 3), ("saved".into(), 1)]
        );
        // Another level, detail or action is another toast.
        queue.push(Toast::error("disk full"), t0);
        assert_eq!(shown(&queue).len(), 3);
        queue.push(Toast::warning("disk full").detail("/tmp"), t0);
        let action = CommandId::parse("view.toggle_breadcrumbs").unwrap();
        queue.push(Toast::warning("disk full").action("Hide", action), t0);
        assert_eq!(queue.waiting(), 2);
        // An identical waiting toast counts up as well, and keeps its place.
        queue.push(Toast::warning("disk full").detail("/tmp"), t0);
        assert_eq!(queue.waiting(), 2);
        queue.dismiss_all();
        // Once dismissed, it's a new toast.
        queue.push(Toast::warning("disk full"), t0);
        assert_eq!(shown(&queue), [("disk full".into(), 1)]);
    }

    #[test]
    fn info_and_warnings_expire_after_six_seconds_and_errors_stay() {
        let (mut queue, t0) = (ToastQueue::default(), Instant::now());
        assert_eq!(LIFETIME, 6 * SECOND);
        queue.push(Toast::info("info"), t0);
        queue.push(Toast::error("error"), t0);
        queue.push(Toast::warning("warning"), t0 + SECOND);
        queue.push(Toast::info("waiting"), t0 + 2 * SECOND);
        assert_eq!(queue.next_deadline(), Some(t0 + LIFETIME));
        assert!(!queue.expire(t0 + LIFETIME - Duration::from_millis(1)));
        assert_eq!(shown(&queue).len(), 3);
        // The waiting toast takes the info's place, its time from now.
        assert!(queue.expire(t0 + LIFETIME));
        assert_eq!(
            shown(&queue),
            [
                ("error".into(), 1),
                ("warning".into(), 1),
                ("waiting".into(), 1)
            ]
        );
        assert_eq!(queue.next_deadline(), Some(t0 + SECOND + LIFETIME));
        assert!(queue.expire(t0 + SECOND + LIFETIME));
        assert_eq!(queue.next_deadline(), Some(t0 + 2 * LIFETIME));
        assert!(queue.expire(t0 + 2 * LIFETIME));
        assert_eq!(shown(&queue), [("error".into(), 1)]);
        assert_eq!(queue.next_deadline(), None);
        assert!(!queue.expire(t0 + 1000 * LIFETIME));
        assert_eq!(shown(&queue), [("error".into(), 1)]);
    }

    #[test]
    fn counting_up_starts_the_time_again() {
        let (mut queue, t0) = (ToastQueue::default(), Instant::now());
        queue.push(Toast::info("again"), t0);
        queue.push(Toast::info("again"), t0 + 5 * SECOND);
        assert!(!queue.expire(t0 + LIFETIME));
        assert_eq!(queue.next_deadline(), Some(t0 + 5 * SECOND + LIFETIME));
        assert!(queue.expire(t0 + 5 * SECOND + LIFETIME));
    }

    #[test]
    fn a_hovered_toast_does_not_expire() {
        let (mut queue, t0) = (ToastQueue::default(), Instant::now());
        let hovered = queue.push(Toast::info("hovered"), t0);
        queue.push(Toast::info("other"), t0);
        queue.set_hovered(hovered, true, t0 + 2 * SECOND);
        assert!(queue.shown()[0].hovered());
        assert!(queue.expire(t0 + 100 * LIFETIME));
        assert_eq!(shown(&queue), [("hovered".into(), 1)]);
        assert_eq!(queue.next_deadline(), None);
        // Counted up under the pointer, it still waits for it to leave.
        queue.push(Toast::info("hovered"), t0 + 100 * LIFETIME);
        assert_eq!(queue.next_deadline(), None);
        // Left, it has its whole time again.
        let left = t0 + 200 * LIFETIME;
        queue.set_hovered(hovered, false, left);
        assert_eq!(queue.next_deadline(), Some(left + LIFETIME));
        // Hovered and left again, it has the time it had left.
        queue.set_hovered(hovered, true, left + 4 * SECOND);
        queue.set_hovered(hovered, false, left + 10 * SECOND);
        assert_eq!(queue.next_deadline(), Some(left + 12 * SECOND));
        assert!(!queue.expire(left + 12 * SECOND - Duration::from_millis(1)));
        assert!(queue.expire(left + 12 * SECOND));
        assert!(queue.shown().is_empty());
    }

    #[test]
    fn a_toast_hovered_past_its_time_stays_until_the_pointer_leaves() {
        let (mut queue, t0) = (ToastQueue::default(), Instant::now());
        let id = queue.push(Toast::warning("late"), t0);
        // Its timer hadn't fired yet.
        queue.set_hovered(id, true, t0 + LIFETIME + SECOND);
        assert!(!queue.expire(t0 + 2 * LIFETIME));
        queue.set_hovered(id, false, t0 + 2 * LIFETIME);
        assert!(queue.expire(t0 + 2 * LIFETIME));
    }
}

/// A workbench showing `text` in a file, focused, in a window of its own,
/// drawn.
fn open(
    text: &str,
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    WindowHandle<Workbench>,
    VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((dir.path().join("a.txt"), Some(Buffer::new(text).into()))),
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let workbench = window.root(cx).unwrap();
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut vcx);
    (dir, window, vcx)
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn workbench(cx: &mut VisualTestContext) -> Entity<Workbench> {
    cx.update(|window, _| window.root::<Workbench>().flatten().unwrap())
}

/// What the window's shown toasts say, top first, with their counts.
fn toasts(cx: &mut VisualTestContext) -> Vec<(String, usize)> {
    let layer = workbench(cx).read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| shown(layer.queue()))
}

/// Where `selector` was drawn in the last frame.
fn bounds(selector: String, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(selector.leak())
}

/// Runs `command` in the workbench, as a key bound to it would.
fn run(command: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new("workbench"));
    let invocation = Invocation::of(command).unwrap().context(stack);
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    draw(cx);
}

/// Whether the panes' breadcrumbs show, which the action toggles.
fn breadcrumbs(cx: &mut VisualTestContext) -> bool {
    workbench(cx).read_with(cx, |workbench, _| workbench.breadcrumbs)
}

fn breadcrumbs_action() -> Toast {
    let command = CommandId::parse("view.toggle_breadcrumbs").unwrap();
    Toast::info("Breadcrumbs").action("Toggle", command)
}

mod window {
    use super::*;

    #[gpui::test]
    fn a_toast_shows_in_the_active_window_else_the_first(cx: &mut TestAppContext) {
        init(cx);
        let (_a, _, mut first) = open("one\n", cx);
        let (_b, _, mut second) = open("two\n", cx);
        // The older window, while active, not the newest.
        first.update(|window, _| window.activate_window());
        cx.update(|cx| show(cx, Toast::error("to the first, active")));
        draw(&mut first);
        draw(&mut second);
        assert_eq!(toasts(&mut first), [("to the first, active".into(), 1)]);
        assert_eq!(toasts(&mut second), []);
        cx.update(|cx| show(cx, Toast::error("to the first, active")));
        assert_eq!(toasts(&mut first), [("to the first, active".into(), 2)]);
        run("toast.dismiss_all", &mut first);
        assert_eq!(toasts(&mut first), []);

        second.update(|window, _| window.activate_window());
        cx.update(|cx| show(cx, Toast::info("to the active window")));
        draw(&mut first);
        draw(&mut second);
        assert_eq!(toasts(&mut first), []);
        assert_eq!(toasts(&mut second), [("to the active window".into(), 1)]);
        let card = bounds("toast-0".into(), &mut second).expect("the toast is drawn");
        // At the bottom right, above the status bar.
        let window = second.update(|window, _| window.viewport_size());
        let bar = bounds("status-bar".into(), &mut second).expect("the status bar shows");
        assert!(card.bottom() < bar.top(), "{card:?} {bar:?}");
        assert!(card.right() > window.width - px(40.), "{card:?} {window:?}");
        assert!(bounds("toast-0".into(), &mut first).is_none());

        // With the active window gone, the first one open shows it.
        second.update(|window, _| window.remove_window());
        cx.run_until_parked();
        cx.update(|cx| show(cx, Toast::info("to the first")));
        draw(&mut first);
        assert_eq!(toasts(&mut first), [("to the first".into(), 1)]);
        assert!(bounds("toast-1".into(), &mut first).is_some());
    }

    #[gpui::test]
    fn in_a_small_window_every_toasts_buttons_can_be_reached(cx: &mut TestAppContext) {
        init(cx);
        let (_dir, _, mut cx) = open("one\n", cx);
        cx.simulate_resize(size(px(480.), px(320.)));
        draw(&mut cx);
        let long = "a long message that wraps ".repeat(40);
        cx.update(|_, cx| {
            for n in 0..3 {
                let toast = Toast::error(format!("{n} {long}")).detail(long.clone());
                show(cx, toast);
            }
        });
        draw(&mut cx);
        let viewport = cx.update(|window, _| window.viewport_size());
        let stack = bounds("toasts".into(), &mut cx).unwrap();
        let first = bounds("toast-0".into(), &mut cx).unwrap();
        let last = bounds("toast-2".into(), &mut cx).unwrap();
        assert!(
            first.size.height + last.size.height > stack.size.height,
            "{stack:?}"
        );
        assert!(
            stack.top() >= px(0.) && stack.bottom() <= viewport.height,
            "{stack:?}"
        );
        let visible = |button: Bounds<Pixels>| {
            stack.contains(&button.origin) && stack.contains(&button.bottom_right())
        };
        // The newest shows.
        let close = bounds("icon-button:toast-close-2".into(), &mut cx).unwrap();
        assert!(visible(close), "{close:?} {stack:?}");
        // The oldest, scrolled to.
        cx.simulate_event(ScrollWheelEvent {
            position: stack.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(10_000.))),
            ..Default::default()
        });
        draw(&mut cx);
        let close = bounds("icon-button:toast-close-0".into(), &mut cx).unwrap();
        assert!(visible(close), "{close:?} {stack:?}");
        cx.simulate_click(close.center(), Modifiers::none());
        draw(&mut cx);
        assert_eq!(toasts(&mut cx).len(), 2);
        assert!(bounds("toast-0".into(), &mut cx).is_none());
    }

    #[gpui::test]
    fn the_close_button_dismisses_its_toast(cx: &mut TestAppContext) {
        init(cx);
        let (_dir, _, mut cx) = open("one\n", cx);
        cx.update(|_, cx| {
            show(cx, Toast::error("first"));
            show(cx, Toast::error("second"));
            show(cx, Toast::error("second"));
        });
        draw(&mut cx);
        assert_eq!(toasts(&mut cx), [("first".into(), 1), ("second".into(), 2)]);
        let close = bounds("icon-button:toast-close-0".into(), &mut cx).unwrap();
        cx.simulate_click(close.center(), Modifiers::none());
        draw(&mut cx);
        assert_eq!(toasts(&mut cx), [("second".into(), 2)]);
        assert!(bounds("toast-0".into(), &mut cx).is_none());
        let close = bounds("icon-button:toast-close-1".into(), &mut cx).unwrap();
        cx.simulate_click(close.center(), Modifiers::none());
        draw(&mut cx);
        assert_eq!(toasts(&mut cx), []);
    }

    #[gpui::test]
    fn dismiss_all_dismisses_every_toast(cx: &mut TestAppContext) {
        init(cx);
        let (_dir, _, mut cx) = open("one\n", cx);
        let show_five = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| (0..5).for_each(|n| show(cx, Toast::error(format!("{n}")))));
            draw(cx);
            assert_eq!(toasts(cx).len(), 3);
        };
        show_five(&mut cx);
        run("toast.dismiss_all", &mut cx);
        assert_eq!(toasts(&mut cx), []);
        let layer = workbench(&mut cx).read_with(&cx, |workbench, _| workbench.toasts().clone());
        assert_eq!(layer.read_with(&cx, |layer, _| layer.queue().waiting()), 0);

        // From the command palette, by its title.
        show_five(&mut cx);
        cx.simulate_keystrokes("cmd-shift-p");
        draw(&mut cx);
        cx.simulate_input("Dismiss Notifications");
        draw(&mut cx);
        cx.simulate_keystrokes("enter");
        draw(&mut cx);
        assert_eq!(toasts(&mut cx), []);
    }

    #[gpui::test]
    fn clicking_the_action_runs_its_command_and_dismisses_the_toast(cx: &mut TestAppContext) {
        init(cx);
        let (_dir, _, mut cx) = open("one\n", cx);
        cx.update(|_, cx| {
            show(cx, breadcrumbs_action());
            show(cx, Toast::error("stays"));
        });
        draw(&mut cx);
        assert!(breadcrumbs(&mut cx));
        let action = bounds("toast-action-0".into(), &mut cx).unwrap();
        cx.simulate_click(action.center(), Modifiers::none());
        draw(&mut cx);
        assert!(!breadcrumbs(&mut cx), "the command ran");
        let ran = workbench(&mut cx).read_with(&cx, |workbench, _| workbench.commands_run.clone());
        let run = ran.last().expect("the command reached the workbench");
        assert_eq!(run.command.as_str(), "view.toggle_breadcrumbs");
        assert_eq!(run.source, Source::Mouse);
        assert_eq!(toasts(&mut cx), [("stays".into(), 1)]);
    }

    #[gpui::test]
    fn info_goes_after_six_seconds_unless_hovered(cx: &mut TestAppContext) {
        init(cx);
        let (_dir, _, mut cx) = open("one\n", cx);
        // The hovered one at the bottom, where the others going doesn't move
        // it from under the pointer.
        cx.update(|_, cx| {
            show(cx, Toast::warning("not hovered"));
            show(cx, Toast::error("error"));
            show(cx, Toast::info("hovered"));
        });
        draw(&mut cx);
        let hovered = bounds("toast-2".into(), &mut cx).unwrap();
        cx.simulate_mouse_move(hovered.center(), None, Modifiers::none());
        draw(&mut cx);
        cx.executor().advance_clock(LIFETIME * 3);
        draw(&mut cx);
        assert_eq!(
            toasts(&mut cx),
            [("error".into(), 1), ("hovered".into(), 1)]
        );
        // Typing doesn't count as leaving it.
        cx.simulate_keystrokes("x");
        draw(&mut cx);
        cx.executor().advance_clock(LIFETIME * 3);
        draw(&mut cx);
        assert_eq!(
            toasts(&mut cx),
            [("error".into(), 1), ("hovered".into(), 1)]
        );
        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::none());
        draw(&mut cx);
        cx.executor()
            .advance_clock(LIFETIME - Duration::from_millis(10));
        draw(&mut cx);
        assert_eq!(toasts(&mut cx).len(), 2);
        cx.executor().advance_clock(Duration::from_millis(10));
        draw(&mut cx);
        assert_eq!(toasts(&mut cx), [("error".into(), 1)]);
    }
}

mod focus_and_vim {
    use super::*;

    /// Focus, the editor's vim mode and its selections.
    fn state(cx: &mut VisualTestContext) -> (bool, Option<&'static str>, String) {
        let editor = workbench(cx).read_with(cx, |workbench, cx| {
            workbench.active_editor(cx).cloned().unwrap()
        });
        let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
        editor.read_with(cx, |editor, _| {
            let selections = format!("{:?}", editor.model().selections());
            (focused, editor.vim_mode(), selections)
        })
    }

    /// Shows, hovers, clicks the action of and dismisses toasts with vim
    /// in the mode `keys` take it to, checking focus and the mode after
    /// each.
    fn leave_focus_and_mode(keys: &str, mode: &str, cx: &mut TestAppContext) {
        init(cx);
        let (_dir, window, mut cx) = open("one two\nthree\n", cx);
        cx.update(|window, cx| {
            set_vim_enabled(cx, true);
            window.activate_window();
        });
        draw(&mut cx);
        cx.simulate_keystrokes(keys);
        draw(&mut cx);
        let before = state(&mut cx);
        assert!(before.0, "the editor has focus");
        assert_eq!(before.1, Some(mode));
        let check = |step: &str, cx: &mut VisualTestContext| {
            assert_eq!(state(cx), before, "{step}");
            assert_eq!(
                cx.update(|_, cx| cx.active_window()),
                Some(window.into()),
                "{step}"
            );
        };

        cx.update(|_, cx| {
            show(cx, breadcrumbs_action());
            show(cx, Toast::error("error"));
            show(cx, Toast::info("info"));
        });
        draw(&mut cx);
        check("shown", &mut cx);

        let card = bounds("toast-0".into(), &mut cx).unwrap();
        cx.simulate_mouse_move(card.center(), None, Modifiers::none());
        draw(&mut cx);
        check("hovered", &mut cx);
        // A press on the card itself, off its buttons.
        cx.simulate_click(card.origin + point(px(6.), px(6.)), Modifiers::none());
        draw(&mut cx);
        check("card clicked", &mut cx);
        assert_eq!(toasts(&mut cx).len(), 3);

        assert!(breadcrumbs(&mut cx));
        let action = bounds("toast-action-0".into(), &mut cx).unwrap();
        cx.simulate_click(action.center(), Modifiers::none());
        draw(&mut cx);
        check("action clicked", &mut cx);
        assert!(!breadcrumbs(&mut cx), "the command ran");
        let ran = workbench(&mut cx).read_with(&cx, |workbench, _| workbench.commands_run.clone());
        assert_eq!(
            ran.last().unwrap().command.as_str(),
            "view.toggle_breadcrumbs"
        );
        assert_eq!(toasts(&mut cx).len(), 2);

        let close = bounds("icon-button:toast-close-1".into(), &mut cx).unwrap();
        cx.simulate_mouse_move(close.center(), None, Modifiers::none());
        draw(&mut cx);
        cx.simulate_click(close.center(), Modifiers::none());
        draw(&mut cx);
        check("closed", &mut cx);
        assert_eq!(toasts(&mut cx), [("info".into(), 1)]);

        cx.simulate_mouse_move(point(px(1.), px(1.)), None, Modifiers::none());
        cx.executor().advance_clock(LIFETIME);
        draw(&mut cx);
        check("expired", &mut cx);
        assert_eq!(toasts(&mut cx), []);

        cx.update(|_, cx| show(cx, Toast::error("error")));
        draw(&mut cx);
        run("toast.dismiss_all", &mut cx);
        check("dismissed all", &mut cx);
        assert_eq!(toasts(&mut cx), []);
    }

    #[gpui::test]
    fn in_insert_mode(cx: &mut TestAppContext) {
        leave_focus_and_mode("i", "insert", cx);
    }

    #[gpui::test]
    fn in_visual_mode(cx: &mut TestAppContext) {
        leave_focus_and_mode("v l", "visual", cx);
    }
}
