// SPDX-License-Identifier: Apache-2.0

//! The frame-request watchdog: its pure policy, then the watchdog on a test
//! window. The test platform has no frame loop, so
//! [`Window::simulate_next_frame`] stands for a frame request reaching the
//! window, and the nudge is a hook.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    Context, FocusHandle, InteractiveElement as _, IntoElement, Render, Styled as _, Task,
    TestAppContext, VisualTestContext, Window, WindowVisibility, div,
};

use super::macos::{
    BACKOFF, Begin, End, FAST_NUDGES, Hooks, Native, POLL, Policy, Record, STALL, Step, watch_with,
};

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// Polls `policy` at `t0 + at`, for a visible key window.
fn poll(policy: &mut Policy, t0: Instant, at: u64) -> Step {
    policy.poll(t0 + ms(at), true, true)
}

#[test]
fn stall_policy() {
    let t0 = Instant::now();

    // Hidden and not key: no probe. Visible, or key: a probe.
    let mut policy = Policy::default();
    assert_eq!(policy.poll(t0, false, false), Step::default());
    assert!(policy.poll(t0, true, false).probe.is_some());
    let mut policy = Policy::default();
    assert!(policy.poll(t0, false, true).probe.is_some());

    // Acknowledged: no stall, and a probe at each poll.
    let mut policy = Policy::default();
    for at in (0..2000).step_by(250) {
        let step = poll(&mut policy, t0, at);
        assert_eq!(step.begin, None, "{at} ms");
        assert!(!step.nudge, "{at} ms");
        policy.acknowledge(step.probe.expect("a probe each poll"));
    }

    // Unacknowledged for longer than STALL, but seen by one poll only (a
    // busy main thread): no stall. The second poll makes it one.
    let mut policy = Policy::default();
    assert!(poll(&mut policy, t0, 0).probe.is_some());
    let step = poll(&mut policy, t0, 600);
    assert!(step.begin.is_none() && !step.nudge, "one poll: {step:?}");
    assert!(step.probe.is_none(), "the probe is still pending");
    let step = poll(&mut policy, t0, 850);
    assert_eq!(
        step.begin,
        Some(Begin {
            unacknowledged: ms(850),
            visible: true,
            key: true
        })
    );
    assert!(step.nudge);

    // Across two polls, STALL apart: a stall and a nudge at the second, which
    // consumes the pending probe.
    let mut policy = Policy::default();
    let first = poll(&mut policy, t0, 0).probe.unwrap();
    assert!(!poll(&mut policy, t0, 250).nudge);
    let step = poll(&mut policy, t0, 500);
    assert!(step.begin.is_some() && step.nudge && step.probe.is_none());
    assert_eq!(step.begin.unwrap().unacknowledged, STALL);
    assert_eq!(policy.nudges(), Some(1));
    // The nudge's frame acknowledges the consumed probe: not recovery, and
    // the next poll posts a fresh probe and doesn't nudge.
    policy.acknowledge(first);
    let step = poll(&mut policy, t0, 750);
    assert!(!step.nudge && step.end.is_none());
    let fresh = step.probe.expect("a fresh probe after the nudge's frame");
    // Unacknowledged at the next poll: a nudge again, and no new begin.
    let step = poll(&mut policy, t0, 1000);
    assert!(step.nudge && step.begin.is_none());
    policy.acknowledge(fresh);
    assert_eq!(policy.nudges(), Some(2), "a consumed probe isn't recovery");

    // A nudge whose frame never runs: no fresh probe is posted (the late
    // frame would acknowledge it), and the next due poll nudges again.
    let mut policy = Policy::default();
    poll(&mut policy, t0, 0);
    poll(&mut policy, t0, 250);
    assert!(poll(&mut policy, t0, 500).nudge);
    assert_eq!(poll(&mut policy, t0, 750), Step::default());
    assert!(poll(&mut policy, t0, 1000).nudge);

    // Back off after FAST_NUDGES: nudges every other poll, then one per
    // BACKOFF; every frame is the nudge's own, and one begin record only.
    let mut policy = Policy::default();
    let mut pending = poll(&mut policy, t0, 0).probe;
    let mut nudged = Vec::new();
    let mut begins = 0;
    for at in (250..=10_000).step_by(250) {
        let step = poll(&mut policy, t0, at);
        begins += usize::from(step.begin.is_some());
        assert_eq!(step.end, None, "{at} ms");
        if step.nudge {
            nudged.push(at);
            // The nudge's frame.
            policy.acknowledge(pending.take().expect("a probe pending at each nudge"));
        }
        pending = pending.or(step.probe);
    }
    assert_eq!(begins, 1);
    let fast: Vec<u64> = (1..=u64::from(FAST_NUDGES)).map(|n| n * 500).collect();
    let slow = (1..=3).map(|n| 4000 + n * BACKOFF.as_millis() as u64);
    assert_eq!(nudged, fast.into_iter().chain(slow).collect::<Vec<_>>());

    // A nudge's frame delayed across hiding and showing the window: no
    // fresh probe is posted until it runs, so it can't count as recovery,
    // and two independent acknowledgements are still needed after it.
    let mut policy = Policy::default();
    let first = poll(&mut policy, t0, 0).probe.unwrap();
    poll(&mut policy, t0, 250);
    assert!(poll(&mut policy, t0, 500).nudge);
    assert_eq!(policy.poll(t0 + ms(750), false, false), Step::default());
    let step = poll(&mut policy, t0, 1000);
    assert_eq!(
        step.probe, None,
        "the consumed probe still holds back fresh ones"
    );
    // The delayed frame runs every pending callback, and nothing else does
    // until 1250.
    policy.acknowledge(first);
    let a = poll(&mut policy, t0, 1250)
        .probe
        .expect("fresh after the frame");
    policy.acknowledge(a);
    let step = poll(&mut policy, t0, 1500);
    assert_eq!(step.end, None, "one independent acknowledgement");
    policy.acknowledge(step.probe.unwrap());
    assert_eq!(
        poll(&mut policy, t0, 1750).end,
        Some(End {
            nudges: 1,
            stalled_for: ms(1750)
        })
    );

    // Recovery: two fresh probes in a row, acknowledged without a nudge.
    let mut policy = Policy::default();
    let first = poll(&mut policy, t0, 0).probe.unwrap();
    poll(&mut policy, t0, 250);
    assert!(poll(&mut policy, t0, 500).nudge);
    policy.acknowledge(first);
    let a = poll(&mut policy, t0, 750).probe.unwrap();
    policy.acknowledge(a);
    let step = poll(&mut policy, t0, 1000);
    assert!(step.end.is_none(), "one fresh probe isn't recovery");
    let b = step.probe.unwrap();
    // A nudge between fresh probes starts the count again.
    let step = poll(&mut policy, t0, 1250);
    assert!(step.nudge && step.end.is_none(), "b went unacknowledged");
    policy.acknowledge(b);
    let c = poll(&mut policy, t0, 1500).probe.unwrap();
    policy.acknowledge(c);
    let d = poll(&mut policy, t0, 1750).probe.unwrap();
    policy.acknowledge(d);
    let step = poll(&mut policy, t0, 2000);
    assert_eq!(
        step.end,
        Some(End {
            nudges: 2,
            stalled_for: ms(2000)
        })
    );
    assert!(!step.nudge && step.probe.is_some());
    assert_eq!(policy.nudges(), None, "the episode is over");
    // Healthy again: no record until the next stall, which begins anew.
    policy.acknowledge(step.probe.unwrap());
    assert_eq!(poll(&mut policy, t0, 2250).end, None);
    poll(&mut policy, t0, 2500);
    let step = poll(&mut policy, t0, 2750);
    assert!(step.begin.is_some() && step.nudge);
    assert_eq!(policy.nudges(), Some(1));
}

/// A view that takes keys and redraws for each.
struct Pad {
    focus: FocusHandle,
    keys: usize,
}

impl Render for Pad {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|pad, _, _, cx| {
                pad.keys += 1;
                cx.notify();
            }))
    }
}

/// The watchdog on a test window, with what it did since `start`.
struct Watched {
    _task: Task<()>,
    nudges: Rc<RefCell<Vec<u64>>>,
    records: Rc<RefCell<Vec<(u64, Record)>>>,
}

impl Watched {
    fn nudges(&self) -> Vec<u64> {
        self.nudges.borrow().clone()
    }

    fn records(&self) -> Vec<(u64, Record)> {
        self.records.borrow().clone()
    }
}

const NATIVE: Native = Native {
    occlusion_visible: Some(false),
    has_screen: Some(true),
    display_id: Some(7),
};

fn elapsed(start: Instant, cx: &gpui::App) -> u64 {
    (cx.background_executor().now() - start).as_millis() as u64
}

/// Watches the window in `cx`. With `frame_on_nudge`, a nudge delivers a
/// frame, as `displayLayer:` does.
fn watch(cx: &mut VisualTestContext, frame_on_nudge: bool) -> Watched {
    let start = cx.executor().now();
    let nudges = Rc::new(RefCell::new(Vec::new()));
    let records = Rc::new(RefCell::new(Vec::new()));
    let hooks = Hooks {
        nudge: Box::new({
            let nudges = nudges.clone();
            move |window, cx| {
                nudges.borrow_mut().push(elapsed(start, cx));
                if frame_on_nudge {
                    window.simulate_next_frame(cx);
                }
            }
        }),
        native: Box::new(|_| NATIVE),
        report: Box::new({
            let records = records.clone();
            let executor = cx.executor();
            move |record| {
                let at = (executor.now() - start).as_millis() as u64;
                records.borrow_mut().push((at, record));
            }
        }),
    };
    let task = cx.update(|window, cx| watch_with(window, cx, hooks));
    Watched {
        _task: task,
        nudges,
        records,
    }
}

/// Moves the clock on by `by`, in small steps, running what comes due.
fn advance(cx: &mut VisualTestContext, by: Duration) {
    let step = ms(10);
    let mut left = by;
    while !left.is_zero() {
        let now = step.min(left);
        cx.executor().advance_clock(now);
        cx.run_until_parked();
        left -= now;
    }
}

/// A frame request reaching the window.
fn frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.simulate_next_frame(cx));
}

fn window(cx: &mut TestAppContext) -> (gpui::Entity<Pad>, &mut VisualTestContext) {
    let (pad, cx) = cx.add_window_view(|_, cx| Pad {
        focus: cx.focus_handle(),
        keys: 0,
    });
    cx.update(|window, cx| {
        let focus = pad.read(cx).focus.clone();
        window.focus(&focus, cx);
        window.activate_window();
    });
    cx.run_until_parked();
    assert!(
        cx.update(|window, _| window.is_window_active() && window.is_visible()),
        "a visible key window"
    );
    (pad, cx)
}

fn begin_at_stall(watched: &Watched, at: u64) {
    assert_eq!(
        watched.records(),
        vec![(
            at,
            Record::Begin(
                Begin {
                    unacknowledged: STALL,
                    visible: true,
                    key: true
                },
                NATIVE
            )
        )]
    );
}

#[gpui::test]
fn healthy_clean_window_never_nudges(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    let watched = watch(cx, true);
    for _ in 0..8 {
        frame(cx);
        advance(cx, POLL);
    }
    assert_eq!(watched.nudges(), Vec::<u64>::new());
    assert_eq!(watched.records(), vec![]);
}

#[gpui::test]
fn no_frame_requests_nudges(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    let watched = watch(cx, false);
    advance(cx, ms(600));
    assert_eq!(watched.nudges(), vec![500]);
    begin_at_stall(&watched, 500);
}

#[gpui::test]
fn key_draws_without_frame_requests_nudge(cx: &mut TestAppContext) {
    let (pad, cx) = window(cx);
    let watched = watch(cx, false);
    for _ in 0..6 {
        cx.simulate_keystrokes("a b");
        advance(cx, ms(100));
    }
    assert_eq!(pad.read_with(cx, |pad, _| pad.keys), 12, "the keys arrived");
    assert_eq!(watched.nudges(), vec![500]);
    begin_at_stall(&watched, 500);
}

#[gpui::test]
fn key_window_reported_hidden_nudges(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    cx.simulate_visibility_change(WindowVisibility::Hidden);
    assert!(cx.update(|window, _| !window.is_visible() && window.is_window_active()));
    let watched = watch(cx, false);
    advance(cx, ms(600));
    assert_eq!(watched.nudges(), vec![500]);
    let records = watched.records();
    assert!(
        matches!(
            records.as_slice(),
            [(
                500,
                Record::Begin(
                    Begin {
                        visible: false,
                        key: true,
                        ..
                    },
                    _
                )
            )]
        ),
        "{records:?}"
    );
}

#[gpui::test]
fn hidden_window_that_is_not_key_is_not_watched(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    cx.simulate_visibility_change(WindowVisibility::Hidden);
    cx.deactivate_window();
    let watched = watch(cx, false);
    advance(cx, ms(2000));
    assert_eq!(watched.nudges(), Vec::<u64>::new());
    assert_eq!(watched.records(), vec![]);
}

#[gpui::test]
fn nudges_that_only_acknowledge_their_own_probe_never_recover(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    let watched = watch(cx, true);
    advance(cx, ms(1000));
    assert_eq!(watched.nudges(), vec![500, 1000], "2 a second");
    advance(cx, ms(9000));
    let fast: Vec<u64> = (1..=u64::from(FAST_NUDGES)).map(|n| n * 500).collect();
    let slow = [6000, 8000, 10_000];
    assert_eq!(
        watched.nudges(),
        fast.into_iter().chain(slow).collect::<Vec<_>>(),
        "then one per 2 s after 8, in one episode"
    );
    begin_at_stall(&watched, 500);
}

#[gpui::test]
fn resumed_delivery_ends_the_episode(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    let watched = watch(cx, false);
    advance(cx, ms(500));
    assert_eq!(watched.nudges(), vec![500]);
    // The nudge's frame, then delivery at each later poll.
    for _ in 0..8 {
        frame(cx);
        advance(cx, POLL);
    }
    assert_eq!(
        watched.nudges(),
        vec![500],
        "no nudge after delivery resumed"
    );
    let records = watched.records();
    assert_eq!(records.len(), 2, "{records:?}");
    // Fresh probes posted at 750 and 1000; the second is seen acknowledged
    // at 1250.
    assert_eq!(
        records[1],
        (
            1250,
            Record::End(End {
                nudges: 1,
                stalled_for: ms(1250)
            })
        )
    );
}

#[gpui::test]
fn entry_point_needs_an_appkit_view(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    // The test platform's windows have no raw handle, so the real nudge
    // has nothing to send to: the exported entry point doesn't watch them.
    assert!(cx.update(super::watch_frame_requests).is_none());
}

#[gpui::test]
fn a_detached_watch_ends_when_its_window_closes(cx: &mut TestAppContext) {
    let (_, cx) = window(cx);
    let Watched {
        _task: task,
        nudges,
        records,
    } = watch(cx, false);
    task.detach();
    advance(cx, POLL);
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    advance(cx, ms(2000));
    assert_eq!(*nudges.borrow(), Vec::<u64>::new());
    assert_eq!(*records.borrow(), vec![]);
    assert_eq!(
        (Rc::strong_count(&nudges), Rc::strong_count(&records)),
        (1, 1),
        "the watch let go of its hooks"
    );
}
