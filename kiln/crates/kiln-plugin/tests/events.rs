// SPDX-License-Identifier: LicenseRef-Proprietary

//! A plugin's event queue coalesces what waits, holds at its capacity by dropping the oldest for
//! a `lagged`, never waits on the plugin or on other senders, and delivers nothing once the
//! plugin is unloaded. Calls into a fixture that can stall run under [`bounded`], so a broken
//! limit fails the test rather than hanging it.

use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

use kiln_command::Grants;
use kiln_plugin::{
    bindings::kiln::extension::{
        events::{BufferChanged, BufferOpened, BufferSaved, Event, EventKinds, SelectionChanged},
        types::Range,
    },
    engine::PluginEngine,
    events::{Change, Edit, EventQueue, HostEvent, MAX_RANGES, Recipient},
    limits::Limits,
    runner::{Call, CallError, Diagnostic, Plugin, PluginState, Reason},
};

/// Test limits: a deadline well past the fixture's slow calls, and short enough that a call
/// stuck past it ends the test well inside its bound.
fn limits() -> Limits {
    Limits {
        call_deadline: Duration::from_millis(1000),
        ..Limits::default()
    }
}

/// Runs `f` on a thread of its own and returns what it returned, failing the test if that takes
/// two seconds.
fn bounded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (sender, returned) = mpsc::channel();
    thread::spawn(move || drop(sender.send(f())));
    returned
        .recv_timeout(Duration::from_secs(2))
        .expect("still running after 2 s")
}

/// events.wat, waiting `busy` in each call, with its queue taking every kind of event; and what
/// it logs: one `e` per event a `handle-events` was handed, and `r` per `run-command`.
fn load(busy: Duration, limits: Limits) -> (Recipient, Receiver<String>) {
    let text = std::fs::read_to_string("tests/fixtures/events.wat").unwrap();
    let marked = "(i64.const 0)) ;; busy-ns";
    assert!(text.contains(marked));
    let text = text.replace(
        marked,
        &format!("(i64.const {})) ;; busy-ns", busy.as_nanos()),
    );
    let (sender, logged) = mpsc::channel();
    let plugin = Plugin::load_logging_to(
        PluginEngine::shared().unwrap(),
        text.as_bytes(),
        "events",
        &Grants::new(),
        limits,
        Box::new(move |_level, message| drop(sender.send(message))),
    )
    .unwrap();
    let recipient = Recipient::new(plugin, &limits);
    recipient.events().subscribe(EventKinds::all());
    (recipient, logged)
}

/// A queue taking every kind of event.
fn subscribed(limits: &Limits) -> EventQueue {
    let queue = EventQueue::new(limits);
    queue.subscribe(EventKinds::all());
    queue
}

/// Delivers until none waits, labelling each.
fn deliver_all(plugin: &mut Recipient) -> Vec<String> {
    std::iter::from_fn(|| plugin.deliver().unwrap())
        .map(|event| label(&event))
        .collect()
}

fn timed_out<T>(returned: &Result<T, CallError>) -> bool {
    matches!(
        returned,
        Err(CallError::Failed(Diagnostic {
            reason: Reason::Timeout,
            ..
        }))
    )
}

fn opened(buffer: u64) -> HostEvent {
    HostEvent::BufferOpened(BufferOpened {
        buffer,
        version: 1,
        path: Some(format!("/work/{buffer}.txt")),
        language: None,
    })
}

/// A change to `buffer` at `version` of edits `(start, end, inserted)`.
fn changed(buffer: u64, version: u64, edits: &[(u64, u64, u64)]) -> HostEvent {
    HostEvent::BufferChanged(Change {
        buffer,
        version,
        edits: edits
            .iter()
            .map(|&(start, end, inserted)| Edit {
                replaced: Range { start, end },
                inserted,
            })
            .collect(),
    })
}

fn saved(buffer: u64, version: u64) -> HostEvent {
    HostEvent::BufferSaved(BufferSaved {
        buffer,
        version,
        path: format!("/work/{buffer}.txt"),
    })
}

fn ranges(ranges: &[(u64, u64)]) -> Vec<Range> {
    ranges
        .iter()
        .map(|&(start, end)| Range { start, end })
        .collect()
}

/// What a delivered or popped event is, for comparing sequences.
fn label(event: &Event) -> String {
    match event {
        Event::BufferOpened(opened) => format!("opened {}", opened.buffer),
        Event::BufferChanged(changed) => {
            format!(
                "changed {} v{} {:?}",
                changed.buffer, changed.version, changed.ranges
            )
        }
        Event::BufferSaved(saved) => format!("saved {} v{}", saved.buffer, saved.version),
        Event::BufferClosed(buffer) => format!("closed {buffer}"),
        Event::FocusChanged(focus) => format!("focus {focus:?}"),
        Event::SelectionChanged(selection) => {
            format!("selection {} v{}", selection.buffer, selection.version)
        }
        Event::ConfigReloaded => "config".into(),
        Event::Lagged(dropped) => format!("lagged {dropped}"),
    }
}

fn selection(buffer: u64, version: u64, cursors: &[(u64, u64)]) -> HostEvent {
    HostEvent::SelectionChanged(SelectionChanged {
        buffer,
        version,
        ranges: ranges(cursors),
    })
}

fn drain(queue: &EventQueue) -> Vec<String> {
    std::iter::from_fn(|| queue.pop())
        .map(|event| label(&event))
        .collect()
}

#[test]
fn changed_events_for_one_buffer_coalesce() {
    let (mut plugin, logged) = load(Duration::ZERO, limits());
    let queue = plugin.events();
    // Inserts 3 bytes at 10, then 2 at 0, which moves those 3 to 12..15; then deletes 20..25.
    queue.push(changed(1, 2, &[(10, 10, 3)]));
    queue.push(changed(2, 7, &[(0, 4, 4)]));
    queue.push(changed(1, 3, &[(0, 0, 2)]));
    queue.push(changed(1, 4, &[(20, 25, 0)]));
    assert_eq!(queue.len(), 2);

    assert_eq!(
        plugin.deliver().unwrap(),
        Some(Event::BufferChanged(BufferChanged {
            buffer: 1,
            version: 4,
            ranges: ranges(&[(0, 2), (12, 15), (20, 20)]),
        }))
    );
    // One event, in a call of its own.
    assert_eq!(logged.recv().unwrap(), "e");
    assert_eq!(
        deliver_all(&mut plugin),
        ["changed 2 v7 [Range { start: 0, end: 4 }]"]
    );
    // No change merges across one the plugin did not take: the waiting change becomes one to
    // read again, and a waiting selection, which nothing can carry across the gap, goes.
    let not_changes = EventKinds::BUFFER_OPENED
        | EventKinds::BUFFER_SAVED
        | EventKinds::BUFFER_CLOSED
        | EventKinds::FOCUS_CHANGED
        | EventKinds::SELECTION_CHANGED
        | EventKinds::CONFIG_RELOADED;
    queue.push(changed(1, 2, &[(10, 10, 3)]));
    queue.push(selection(1, 2, &[(13, 13)]));
    queue.subscribe(not_changes);
    queue.push(changed(1, 3, &[(0, 0, 2)]));
    queue.subscribe(EventKinds::all());
    queue.push(changed(1, 4, &[(20, 20, 1)]));
    assert_eq!(drain(&queue), ["lagged 1", "changed 1 v4 []"]);
    // A selection pushed after the gap is in the text after it, so it still moves with a merge.
    queue.push(changed(1, 5, &[(0, 0, 1)]));
    queue.subscribe(not_changes);
    queue.push(changed(1, 6, &[(0, 0, 1)]));
    queue.subscribe(EventKinds::all());
    queue.push(selection(1, 6, &[(4, 4)]));
    queue.push(changed(1, 7, &[(0, 0, 1)]));
    assert_eq!(
        std::iter::from_fn(|| queue.pop()).collect::<Vec<_>>(),
        [
            Event::BufferChanged(BufferChanged {
                buffer: 1,
                version: 7,
                ranges: Vec::new(),
            }),
            Event::SelectionChanged(SelectionChanged {
                buffer: 1,
                version: 7,
                ranges: ranges(&[(5, 5)]),
            }),
        ]
    );
}

#[test]
fn a_merged_change_maps_earlier_ranges_into_the_latest_text() {
    let queue = subscribed(&limits());
    // 5..8 and 20..22 changed; then 6..21 is replaced by 1 byte, swallowing the end of one and
    // the start of the other, and 30..30 grows by 4, which lands at 16 in the latest text.
    queue.push(changed(1, 2, &[(5, 8, 3), (20, 22, 2)]));
    queue.push(changed(1, 3, &[(6, 21, 1), (30, 30, 4)]));
    assert_eq!(
        queue.pop(),
        Some(Event::BufferChanged(BufferChanged {
            buffer: 1,
            version: 3,
            ranges: ranges(&[(5, 8), (16, 20)]),
        }))
    );

    // Past the cap, or out of order, a change lists no ranges: the plugin reads it again.
    let apart = |from: u64, count: u64| -> Vec<_> {
        (0..count)
            .map(|at| (from + at * 10, from + at * 10, 1))
            .collect()
    };
    let half = MAX_RANGES as u64 / 2 + 1;
    queue.push(changed(1, 4, &apart(0, half)));
    queue.push(changed(1, 5, &apart(10_000, half)));
    queue.push(changed(2, 2, &[(9, 9, 1), (0, 1, 1)]));
    assert_eq!(drain(&queue), ["changed 1 v5 []", "changed 2 v2 []"]);
    // And once a change says so, merging into it does not undo it.
    queue.push(changed(1, 6, &apart(0, MAX_RANGES as u64 + 1)));
    queue.push(changed(1, 7, &[(0, 0, 1)]));
    assert_eq!(drain(&queue), ["changed 1 v7 []"]);
    // A change of very many edits is one to read again, decided before looking through them.
    queue.push(changed(3, 2, &apart(0, 100_000)));
    assert_eq!(drain(&queue), ["changed 3 v2 []"]);
}

#[test]
fn focus_and_selection_keep_only_the_latest() {
    let (mut plugin, logged) = load(Duration::from_millis(100), limits());
    let queue = plugin.events();
    queue.push(opened(1));
    let busy = thread::spawn(move || {
        let delivered = plugin.deliver();
        (plugin, delivered)
    });
    // The plugin is inside its first call.
    assert_eq!(logged.recv_timeout(Duration::from_secs(2)).unwrap(), "e");
    for version in 0..10 {
        queue.push(HostEvent::FocusChanged(Some(version)));
        queue.push(selection(1, version, &[(version, version)]));
    }
    assert_eq!(queue.len(), 2);

    let (mut plugin, delivered) = busy.join().unwrap();
    assert_eq!(
        delivered.unwrap().map(|event| label(&event)),
        Some("opened 1".into())
    );
    assert_eq!(
        deliver_all(&mut plugin),
        ["focus Some(9)", "selection 1 v9"]
    );
    assert_eq!(logged.try_iter().collect::<Vec<_>>(), ["e", "e"]);
}

#[test]
fn opened_saved_closed_are_never_merged_and_stay_ordered() {
    let queue = subscribed(&limits());
    queue.push(opened(1));
    queue.push(opened(2));
    queue.push(changed(1, 2, &[(0, 0, 1)]));
    queue.push(HostEvent::FocusChanged(Some(1)));
    queue.push(changed(2, 2, &[(0, 0, 1)]));
    queue.push(saved(1, 2));
    queue.push(changed(1, 3, &[(0, 0, 1)]));
    queue.push(changed(2, 3, &[(1, 1, 1)]));
    queue.push(changed(1, 4, &[(1, 1, 1)]));
    queue.push(HostEvent::BufferClosed(2));
    queue.push(saved(1, 4));
    queue.push(saved(1, 4));
    queue.push(HostEvent::ConfigReloaded);
    queue.push(HostEvent::BufferClosed(1));
    queue.push(opened(1));
    queue.push(changed(1, 1, &[(0, 0, 1)]));
    assert_eq!(
        drain(&queue),
        [
            "opened 1",
            "opened 2",
            "changed 1 v2 [Range { start: 0, end: 1 }]",
            "focus Some(1)",
            "changed 2 v3 [Range { start: 0, end: 2 }]",
            "saved 1 v2",
            "changed 1 v4 [Range { start: 0, end: 2 }]",
            "closed 2",
            "saved 1 v4",
            "saved 1 v4",
            "config",
            "closed 1",
            "opened 1",
            "changed 1 v1 [Range { start: 0, end: 1 }]",
        ]
    );

    // A selection waiting behind a change never arrives after a newer version: when a later
    // change merges, the selection moves with it, mapped through that change's edits.
    queue.push(changed(1, 2, &[(0, 0, 1)]));
    queue.push(selection(1, 2, &[(1, 1), (0, 3)]));
    queue.push(changed(1, 3, &[(1, 1, 2)]));
    assert_eq!(
        std::iter::from_fn(|| queue.pop()).collect::<Vec<_>>(),
        [
            Event::BufferChanged(BufferChanged {
                buffer: 1,
                version: 3,
                ranges: ranges(&[(0, 3)]),
            }),
            Event::SelectionChanged(SelectionChanged {
                buffer: 1,
                version: 3,
                ranges: ranges(&[(3, 3), (0, 5)]),
            }),
        ]
    );
    // A change whose edits cannot map it stays behind the selection instead.
    queue.push(changed(1, 2, &[(0, 0, 1)]));
    queue.push(selection(1, 2, &[(1, 1)]));
    queue.push(changed(1, 3, &[(5, 5, 1), (0, 0, 1)]));
    assert_eq!(
        drain(&queue),
        [
            "changed 1 v2 [Range { start: 0, end: 1 }]",
            "selection 1 v2",
            "changed 1 v3 []",
        ]
    );
    // Typing, a change and a cursor move per key, still coalesces to one of each.
    for key in 0..100 {
        queue.push(changed(1, key + 2, &[(key, key, 1)]));
        queue.push(selection(1, key + 2, &[(key + 1, key + 1)]));
    }
    assert_eq!(
        drain(&queue),
        [
            "changed 1 v101 [Range { start: 0, end: 100 }]",
            "selection 1 v101"
        ]
    );
}

#[test]
fn a_full_queue_drops_the_oldest_and_reports_lagged() {
    let limits = Limits::default();
    assert_eq!(limits.event_queue, 256);

    // The first to go is the oldest that coalescing could have absorbed: the change, not the
    // open before it.
    let queue = subscribed(&limits);
    queue.push(opened(0));
    queue.push(changed(1, 2, &[(0, 0, 1)]));
    for buffer in 2..=256 {
        queue.push(opened(buffer));
    }
    assert_eq!(queue.len(), 256);
    let first: Vec<_> = drain(&queue).into_iter().take(3).collect();
    assert_eq!(first, ["lagged 1", "opened 0", "opened 2"]);

    // With none of those left, the oldest goes.
    let (mut plugin, logged) = load(Duration::ZERO, limits);
    let queue = plugin.events();
    let started = Instant::now();
    for buffer in 0..1000 {
        queue.push(opened(buffer));
        assert!(queue.len() <= 256);
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(queue.len(), 256);
    assert_eq!(plugin.deliver().unwrap(), Some(Event::Lagged(744)));
    assert_eq!(
        plugin.deliver().unwrap().map(|event| label(&event)),
        Some("opened 744".into())
    );
    assert_eq!(queue.len(), 255);
    assert_eq!(logged.try_iter().collect::<Vec<_>>(), ["e", "e"]);

    // A full queue never discards the newest focus or selection: they replace the older ones,
    // and the oldest coalescible event makes room.
    let queue = subscribed(&limits);
    queue.push(HostEvent::FocusChanged(Some(0)));
    queue.push(selection(0, 1, &[(0, 0)]));
    queue.push(changed(9, 2, &[(0, 0, 1)]));
    for buffer in 0..253 {
        queue.push(opened(buffer));
    }
    assert_eq!(queue.len(), 256);
    queue.push(HostEvent::FocusChanged(Some(1)));
    queue.push(selection(0, 2, &[(1, 1)]));
    queue.push(selection(1, 1, &[(0, 0)]));
    assert_eq!(queue.len(), 256);
    let delivered = drain(&queue);
    assert_eq!(delivered[0], "lagged 1");
    assert_eq!(
        delivered[delivered.len() - 3..],
        ["focus Some(1)", "selection 0 v2", "selection 1 v1"]
    );
    assert!(!delivered.contains(&"changed 9 v2 [Range { start: 0, end: 1 }]".into()));
    // A queue of no capacity holds nothing, and a push does nothing.
    let queue = subscribed(&Limits {
        event_queue: 0,
        ..limits
    });
    queue.push(opened(1));
    queue.push(HostEvent::FocusChanged(Some(1)));
    assert!(queue.is_empty());
    assert_eq!(queue.pop(), None);
}

#[test]
fn senders_at_once_share_one_capacity_and_lose_nothing_uncounted() {
    let queue = subscribed(&Limits::default());
    let senders: Vec<_> = (0..4u64)
        .map(|sender| {
            let queue = queue.clone();
            thread::spawn(move || {
                for buffer in 0..1000 {
                    queue.push(opened(sender * 1000 + buffer));
                }
            })
        })
        .collect();
    // Popping and counting beside them, as delivery would: nothing waits outside the 256.
    let mut received = 0;
    let mut most = 0;
    while !senders.iter().all(|sender| sender.is_finished()) {
        most = most.max(queue.len());
        received += match queue.pop() {
            Some(Event::Lagged(dropped)) => dropped,
            Some(_) => 1,
            None => 0,
        };
    }
    for sender in senders {
        sender.join().unwrap();
    }
    assert!(most <= 256, "{most} waited");
    assert!(queue.len() <= 256);
    received += std::iter::from_fn(|| queue.pop())
        .map(|event| match event {
            Event::Lagged(dropped) => dropped,
            _ => 1,
        })
        .sum::<u64>();
    assert_eq!(received, 4000);
}

#[test]
fn a_stuck_plugin_does_not_delay_the_sender() {
    let (mut plugin, logged) = load(Duration::from_millis(100), limits());
    let queue = plugin.events();
    queue.push(opened(1));
    let (returned, returns) = mpsc::channel();
    let busy = thread::spawn(move || {
        let delivered = plugin.deliver();
        returned.send(()).ok();
        delivered
    });
    assert_eq!(logged.recv_timeout(Duration::from_secs(2)).unwrap(), "e");

    let started = Instant::now();
    for push in 0..1000 {
        queue.push(match push % 4 {
            0 => opened(push),
            1 => changed(push % 8, push, &[(push, push, 1)]),
            2 => HostEvent::FocusChanged(Some(push)),
            _ => saved(push % 8, push),
        });
    }
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(50),
        "1000 pushes took {took:?}"
    );
    // The plugin is still inside the call the pushes ran beside.
    assert_eq!(returns.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(queue.len(), 256);

    let delivered = busy.join().unwrap().unwrap();
    assert_eq!(
        delivered.map(|event| label(&event)),
        Some("opened 1".into())
    );
}

/// events.wat stuck in every call, past a short deadline.
fn stuck() -> (Recipient, Receiver<String>) {
    load(
        Duration::from_secs(3600),
        Limits {
            call_deadline: Duration::from_millis(250),
            ..Limits::default()
        },
    )
}

/// Whether `queue` is closed, empty, and stays so through more pushes.
fn stays_empty(queue: &EventQueue) -> bool {
    let was = queue.is_closed() && queue.is_empty();
    queue.push(opened(3));
    queue.push(HostEvent::FocusChanged(Some(3)));
    was && queue.is_empty() && queue.pop().is_none()
}

#[test]
fn an_unloaded_plugin_receives_nothing() {
    // An event delivery unloads it.
    let (plugin, logged) = stuck();
    let queue = plugin.events();
    queue.push(opened(1));
    queue.push(opened(2));
    let (delivered, mut plugin) = bounded(move || {
        let mut plugin = plugin;
        (plugin.deliver(), plugin)
    });
    assert!(timed_out(&delivered));
    assert!(stays_empty(&queue));
    assert_eq!(plugin.deliver(), Err(CallError::Unloaded));
    // Only the call that unloaded it ran.
    assert_eq!(logged.try_iter().collect::<Vec<_>>(), ["e"]);

    // A command unloads it.
    let (plugin, logged) = stuck();
    let queue = plugin.events();
    queue.push(opened(1));
    let (ran, mut plugin) = bounded(move || {
        let mut plugin = plugin;
        let ran = plugin.call(Call::RunCommand {
            id: "fixture.run".into(),
            args: "{}".into(),
        });
        (ran, plugin)
    });
    assert!(timed_out(&ran));
    assert!(matches!(plugin.plugin().state(), PluginState::Unloaded(_)));
    assert!(stays_empty(&queue));
    assert_eq!(plugin.deliver(), Err(CallError::Unloaded));
    assert_eq!(logged.try_iter().collect::<Vec<_>>(), ["r"]);

    // It drops, while the host still holds its queue.
    let (plugin, logged) = load(Duration::ZERO, limits());
    let queue = plugin.events();
    queue.push(opened(1));
    drop(plugin);
    assert!(stays_empty(&queue));
    assert_eq!(logged.try_iter().count(), 0);
}

#[test]
fn a_queue_takes_only_the_kinds_subscribed() {
    let queue = EventQueue::new(&limits());
    queue.push(opened(1));
    assert!(queue.is_empty());
    queue.subscribe(EventKinds::BUFFER_SAVED | EventKinds::FOCUS_CHANGED);
    queue.push(opened(1));
    queue.push(saved(1, 1));
    queue.push(HostEvent::FocusChanged(None));
    queue.push(HostEvent::ConfigReloaded);
    assert_eq!(drain(&queue), ["saved 1 v1", "focus None"]);
}

#[test]
fn events_carry_no_text() {
    let queue = subscribed(&limits());
    queue.push(changed(1, 2, &[(0, 4, 2)]));
    // Spelled out field by field, so a field added to either fails to compile here.
    let Some(Event::BufferChanged(BufferChanged {
        buffer,
        version,
        ranges: delivered,
    })) = queue.pop()
    else {
        panic!("not a change");
    };
    assert_eq!((buffer, version, delivered), (1, 2, ranges(&[(0, 2)])));
    let HostEvent::BufferChanged(Change {
        buffer: _,
        version: _,
        edits,
    }) = changed(1, 2, &[(0, 4, 2)])
    else {
        unreachable!();
    };
    let [
        Edit {
            replaced: _,
            inserted: _,
        },
    ] = edits[..]
    else {
        unreachable!();
    };
}
