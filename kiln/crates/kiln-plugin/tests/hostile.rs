// SPDX-License-Identifier: LicenseRef-Proprietary

//! Hostile plugins cannot hang, exhaust or abort the host. Each case is a fixture under
//! `tests/fixtures/hostile/`; every test runs under [`bounded`], so a broken limit fails it
//! rather than hanging it.

use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use kiln_command::Grants;
use kiln_plugin::{
    bindings::LogSink,
    engine::PluginEngine,
    limits::{Limits, MAX_READ_BYTES},
    runner::{Call, CallError, LoadError, Plugin, PluginState, Reason, Reply},
};

/// Small limits: a deadline short enough that every case ends well under two seconds, long
/// enough that loading a fixture meets it on a loaded gate machine.
fn limits() -> Limits {
    Limits {
        call_deadline: Duration::from_millis(250),
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

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/hostile/{name}.wat")).unwrap()
}

fn load_bytes(bytes: &[u8], limits: Limits) -> Result<Plugin, LoadError> {
    let engine = PluginEngine::shared().unwrap();
    Plugin::load(engine, bytes, "hostile", &Grants::new(), limits)
}

fn load(name: &str) -> Result<Plugin, LoadError> {
    load_bytes(fixture(name).as_bytes(), limits())
}

fn run_command(plugin: &mut Plugin) -> Result<Reply, CallError> {
    plugin.call(Call::RunCommand {
        id: "fixture.run".into(),
        args: "{}".into(),
    })
}

fn failed_with(error: CallError, reason: Reason) {
    let CallError::Failed(diagnostic) = error else {
        panic!("not a failure that unloaded: {error:?}");
    };
    assert_eq!(diagnostic.reason, reason, "{diagnostic}");
}

#[test]
fn unbounded_recursion_traps_and_unloads() {
    bounded(|| {
        let mut plugin = load("recursion").unwrap();
        let started = Instant::now();
        failed_with(run_command(&mut plugin).unwrap_err(), Reason::Trap);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(matches!(
            plugin.state(),
            PluginState::Unloaded(diagnostic) if diagnostic.reason == Reason::Trap
        ));
        assert!(!plugin.thread_running());
    });
}

#[test]
fn a_looping_init_is_interrupted() {
    bounded(|| {
        let error = load("loop_init").err().unwrap();
        assert!(
            matches!(&error, LoadError::Unloaded(diagnostic) if diagnostic.reason == Reason::Timeout),
            "{error:?}"
        );
    });
}

#[test]
fn a_looping_start_function_is_interrupted() {
    bounded(|| {
        let error = load("loop_start").err().unwrap();
        assert!(
            matches!(&error, LoadError::Unloaded(diagnostic) if diagnostic.reason == Reason::Timeout),
            "{error:?}"
        );
    });
}

#[test]
fn a_huge_memory_grow_is_refused_not_aborted() {
    bounded(|| {
        let mut plugin = load("grow_huge").unwrap();
        // The fixture traps unless the grow returned -1.
        assert!(matches!(
            run_command(&mut plugin),
            Ok(Reply::RunCommand(Ok(())))
        ));
        assert_eq!(*plugin.state(), PluginState::Running);
        assert!(run_command(&mut plugin).is_ok());
    });
}

#[test]
fn a_huge_table_grow_is_refused() {
    bounded(|| {
        let mut plugin = load("grow_table").unwrap();
        // The fixture traps unless the grow returned -1.
        assert!(matches!(
            run_command(&mut plugin),
            Ok(Reply::RunCommand(Ok(())))
        ));
        assert_eq!(*plugin.state(), PluginState::Running);
    });
}

#[test]
fn garbage_and_invalid_components_are_load_errors() {
    bounded(|| {
        let garbage = load_bytes(b"this is not a component", limits())
            .err()
            .unwrap();
        assert!(matches!(garbage, LoadError::Invalid { .. }), "{garbage:?}");
        let invalid = load("invalid_core").err().unwrap();
        assert!(matches!(invalid, LoadError::Invalid { .. }), "{invalid:?}");
    });
}

#[test]
fn an_oversized_component_is_refused_unread() {
    bounded(|| {
        let over = vec![0; MAX_READ_BYTES + 1];
        let error = load_bytes(&over, limits()).err().unwrap();
        assert!(matches!(error, LoadError::TooLarge { .. }), "{error:?}");
    });
}

#[test]
fn one_hostile_plugin_does_not_stall_another() {
    bounded(|| {
        let engine = PluginEngine::shared().unwrap();
        let slow = Limits {
            call_deadline: Duration::from_millis(800),
            ..Limits::default()
        };
        let (entered, looping_entered) = mpsc::channel();
        let sink: LogSink = Box::new(move |_, _| entered.send(()).unwrap_or(()));
        let mut looping = Plugin::load_logging_to(
            engine,
            fixture("loop_call").as_bytes(),
            "looping",
            &Grants::new(),
            slow,
            sink,
        )
        .unwrap();
        let mut neighbour = Plugin::load(
            engine,
            include_str!("fixtures/empty.wat").as_bytes(),
            "neighbour",
            &Grants::new(),
            limits(),
        )
        .unwrap();
        let spinning = thread::spawn(move || {
            let result = run_command(&mut looping);
            (looping, result)
        });
        // The fixture logs just before its loop: ask the neighbour once that is running.
        looping_entered
            .recv_timeout(Duration::from_millis(500))
            .expect("the looping plugin never started");
        let asked = Instant::now();
        assert!(run_command(&mut neighbour).is_ok());
        assert!(asked.elapsed() < Duration::from_millis(200));
        assert_eq!(*neighbour.state(), PluginState::Running);
        assert!(
            !spinning.is_finished(),
            "the loop ended before the neighbour answered"
        );
        let (looping, result) = spinning.join().unwrap();
        failed_with(result.unwrap_err(), Reason::Timeout);
        assert!(matches!(looping.state(), PluginState::Unloaded(_)));
    });
}
