// SPDX-License-Identifier: LicenseRef-Proprietary

//! Plugins run on their own threads under their limits, and a trap or an overrun unloads them.
//! Every call into a hostile fixture runs under [`bounded`], so a broken limit fails the test
//! rather than hanging it.

use std::{
    cell::RefCell,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

use kiln_command::Grants;
use kiln_plugin::{
    bindings::{Level, LogSink},
    engine::PluginEngine,
    fs::Workspace,
    limits::{CALL_DEADLINE, EVENT_QUEUE, Limits, MAX_READ_BYTES, MEMORY_BYTES, StoreLimiter},
    runner::{
        Call, CallError, Diagnostic, LoadError, Plugin, PluginState, Reason, Reply, wasi_context,
    },
};
use wasmtime::{ResourceLimiter, component::ResourceTable};
use wasmtime_wasi::{
    cli::WasiCliCtxView,
    filesystem::WasiFilesystemCtxView,
    p2::{
        StreamError,
        bindings::{
            cli::{environment, stdin, terminal_stderr, terminal_stdin, terminal_stdout},
            filesystem::preopens,
        },
    },
};

/// Test limits: a deadline short enough that every test stays well under a second, long enough
/// that loading a fixture meets it on a loaded gate machine.
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

/// Sends on its channel when dropped: as the thread it was left on ends.
struct OnExit(Sender<()>);

impl Drop for OnExit {
    fn drop(&mut self) {
        self.0.send(()).ok();
    }
}

thread_local! {
    static ON_EXIT: RefCell<Option<OnExit>> = const { RefCell::new(None) };
}

/// What a plugin logged, by message.
struct Logged {
    /// Each message, with the thread it was logged on.
    messages: Receiver<(ThreadId, String)>,
    /// Hears once the thread that logged first has ended.
    exited: Receiver<()>,
}

/// A sink that records each message and the thread it ran on, and `delay`s each.
fn sink(delay: Duration) -> (LogSink, Logged) {
    let (message, messages) = mpsc::channel();
    let (exit, exited) = mpsc::channel();
    let sink: LogSink = Box::new(move |_level: Level, text| {
        ON_EXIT.with_borrow_mut(|on_exit| {
            on_exit.get_or_insert_with(|| OnExit(exit.clone()));
        });
        drop(message.send((thread::current().id(), text)));
        thread::sleep(delay);
    });
    (sink, Logged { messages, exited })
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}.wat")).unwrap()
}

fn try_load(
    name: &str,
    text: &str,
    limits: Limits,
    delay: Duration,
) -> (Result<Plugin, LoadError>, Logged) {
    let engine = PluginEngine::shared().unwrap();
    let (sink, logged) = sink(delay);
    let plugin =
        Plugin::load_logging_to(engine, text.as_bytes(), name, &Grants::new(), limits, sink);
    (plugin, logged)
}

fn load(name: &str, limits: Limits) -> (Plugin, Logged) {
    let (plugin, logged) = try_load(name, &fixture(name), limits, Duration::ZERO);
    (plugin.unwrap(), logged)
}

/// block.wat, blocking in `export`: `init`, `run-command` or `shutdown`.
fn blocking_in(export: &str) -> String {
    let value = match export {
        "init" => 1,
        "run-command" => 2,
        "shutdown" => 3,
        _ => unreachable!("{export}"),
    };
    let text = fixture("block");
    let marked = "(i32.const 0)) ;; blocks-in";
    assert!(text.contains(marked));
    text.replace(marked, &format!("(i32.const {value})) ;; blocks-in"))
}

fn run_command(plugin: &mut Plugin) -> Result<Reply, CallError> {
    plugin.call(Call::RunCommand {
        id: "fixture.run".into(),
        args: "{}".into(),
    })
}

fn timed_out(error: &CallError) -> bool {
    matches!(
        error,
        CallError::Failed(Diagnostic {
            reason: Reason::Timeout,
            ..
        })
    )
}

#[test]
fn the_default_limits_are_the_named_constants() {
    assert_eq!(CALL_DEADLINE, Duration::from_millis(100));
    assert_eq!(MEMORY_BYTES, 64 * 1024 * 1024);
    assert_eq!(EVENT_QUEUE, 256);
    assert_eq!(MAX_READ_BYTES, 16 * 1024 * 1024);
    assert_eq!(
        Limits::default(),
        Limits {
            call_deadline: CALL_DEADLINE,
            memory_bytes: MEMORY_BYTES,
            event_queue: EVENT_QUEUE,
            max_read_bytes: MAX_READ_BYTES,
        }
    );
}

#[test]
fn an_infinite_loop_is_interrupted() {
    let limits = Limits {
        call_deadline: Duration::from_millis(20),
        ..Limits::default()
    };
    // Loading meets 20 ms on an idle machine; a loaded gate may overrun it before the call
    // under test, so the load alone gets a few tries.
    let text = fixture("loop_forever");
    let mut plugin = (0..5)
        .find_map(
            |_| match try_load("loop_forever", &text, limits, Duration::ZERO).0 {
                Ok(plugin) => Some(plugin),
                Err(LoadError::Unloaded(Diagnostic {
                    reason: Reason::Timeout,
                    ..
                })) => None,
                Err(error) => panic!("{error:?}"),
            },
        )
        .expect("loop_forever loads within 20 ms in five tries");
    let (plugin, error, elapsed) = bounded(move || {
        let started = Instant::now();
        let error = run_command(&mut plugin).unwrap_err();
        (plugin, error, started.elapsed())
    });
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    let CallError::Failed(diagnostic) = error else {
        panic!("{error:?}");
    };
    assert_eq!(diagnostic.reason, Reason::Timeout, "{diagnostic}");
    assert_eq!(plugin.state(), &PluginState::Unloaded(diagnostic));
    assert!(!plugin.thread_running());
}

#[test]
fn a_call_that_returns_past_its_deadline_times_out() {
    // grow_memory's `run-command` ends in a `log`, which this sink holds past the deadline:
    // no epoch check runs after it, so only the time it returned at shows the overrun.
    // Loading meets the 250 ms deadline on an idle machine; a loaded gate may overrun it before
    // the call under test, so the load alone gets a few tries.
    let text = fixture("grow_memory");
    let (mut plugin, logged) = (0..5)
        .find_map(
            |_| match try_load("grow_memory", &text, limits(), Duration::from_millis(400)) {
                (Ok(plugin), logged) => Some((plugin, logged)),
                (
                    Err(LoadError::Unloaded(Diagnostic {
                        reason: Reason::Timeout,
                        ..
                    })),
                    _,
                ) => None,
                (Err(error), _) => panic!("{error:?}"),
            },
        )
        .expect("grow_memory loads within 250 ms in five tries");
    let (plugin, error) = bounded(move || {
        let error = run_command(&mut plugin).unwrap_err();
        (plugin, error)
    });
    assert!(timed_out(&error), "{error:?}");
    assert_eq!(logged.messages.recv().unwrap().1, "grown");
    assert!(matches!(plugin.state(), PluginState::Unloaded(_)));
}

#[test]
fn a_blocking_wasi_import_times_out_in_init() {
    let text = blocking_in("init");
    let (loaded, elapsed) = bounded(move || {
        let started = Instant::now();
        let (loaded, _logged) = try_load("block", &text, limits(), Duration::ZERO);
        (loaded.err(), started.elapsed())
    });
    assert!(
        matches!(
            loaded,
            Some(LoadError::Unloaded(Diagnostic {
                reason: Reason::Timeout,
                ..
            }))
        ),
        "{loaded:?}"
    );
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
}

#[test]
fn a_blocking_wasi_import_times_out_in_a_call() {
    let (plugin, _logged) = try_load(
        "block",
        &blocking_in("run-command"),
        limits(),
        Duration::ZERO,
    );
    let mut plugin = plugin.unwrap();
    let (plugin, error) = bounded(move || {
        let error = run_command(&mut plugin).unwrap_err();
        (plugin, error)
    });
    assert!(timed_out(&error), "{error:?}");
    assert!(!plugin.thread_running());
}

#[test]
fn a_blocking_wasi_import_times_out_in_shutdown() {
    let (plugin, _logged) = try_load("block", &blocking_in("shutdown"), limits(), Duration::ZERO);
    let mut plugin = plugin.unwrap();
    let elapsed = bounded(move || {
        assert_eq!(run_command(&mut plugin), Ok(Reply::RunCommand(Ok(()))));
        let started = Instant::now();
        drop(plugin);
        started.elapsed()
    });
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
}

#[test]
fn memory_growth_past_the_limit_fails() {
    // The fixture grows by 4 MiB a call: granted under the default limit, refused under 2 MiB.
    let (mut plugin, logged) = load("grow_memory", Limits::default());
    let (_plugin, replied) = bounded(move || {
        let replied = run_command(&mut plugin);
        (plugin, replied)
    });
    assert_eq!(replied, Ok(Reply::RunCommand(Ok(()))));
    assert_eq!(logged.messages.recv().unwrap().1, "grown");

    let (mut plugin, logged) = load(
        "grow_memory",
        Limits {
            memory_bytes: 2 << 20,
            ..limits()
        },
    );
    let (plugin, replies) = bounded(move || {
        let replies = [run_command(&mut plugin), run_command(&mut plugin)];
        (plugin, replies)
    });
    for replied in replies {
        assert_eq!(replied, Ok(Reply::RunCommand(Ok(()))));
        assert_eq!(logged.messages.recv().unwrap().1, "refused");
    }
    assert_eq!(plugin.state(), &PluginState::Running);
}

#[test]
fn a_failed_growth_gives_back_its_charge() {
    const MIB: usize = 1 << 20;
    let mut limiter = StoreLimiter::new(&Limits {
        memory_bytes: 2 * MIB,
        ..Limits::default()
    });
    assert!(limiter.memory_growing(0, MIB, None).unwrap());
    assert!(limiter.memory_growing(MIB, 2 * MIB, None).unwrap());
    assert!(!limiter.refused());
    // Allowed, then the allocation failed: the memory stayed at 1 MiB.
    limiter
        .memory_grow_failed(wasmtime::format_err!("mmap failed"))
        .unwrap();
    assert!(limiter.refused());
    limiter.clear_refused();
    assert!(limiter.memory_growing(MIB, 2 * MIB, None).unwrap());
    // Past what the memory's type can address: wasmtime reports that without asking first, so
    // the growth allowed before it stands.
    limiter
        .memory_grow_failed(wasmtime::format_err!(
            "memory growth exceeds memory type's limits"
        ))
        .unwrap();
    assert!(!limiter.memory_growing(2 * MIB, 2 * MIB + 1, None).unwrap());
    assert!(limiter.refused());
}

#[test]
fn a_trap_unloads_the_plugin_with_a_diagnostic() {
    let (mut plugin, logged) = load("trap", limits());
    let (plugin, error) = bounded(move || {
        let error = run_command(&mut plugin).unwrap_err();
        (plugin, error)
    });
    let CallError::Failed(diagnostic) = error else {
        panic!("{error:?}");
    };
    assert!(
        matches!(&diagnostic, Diagnostic { plugin, reason: Reason::Trap, .. } if plugin == "trap"),
        "{diagnostic:?}"
    );
    assert!(
        diagnostic.to_string().contains("plugin trap"),
        "{diagnostic}"
    );
    assert_eq!(plugin.state(), &PluginState::Unloaded(diagnostic));
    // The thread ran the guest, then ended, dropping the store and its log sink with it.
    assert_eq!(logged.messages.recv().unwrap().1, "trapping");
    assert_eq!(logged.exited.try_recv(), Ok(()));
    assert_eq!(logged.messages.try_recv(), Err(TryRecvError::Disconnected));
    assert!(!plugin.thread_running());
}

#[test]
fn an_unloaded_plugin_never_runs_again() {
    let (mut plugin, logged) = load("trap", limits());
    let replies = bounded(move || {
        let mut replies = vec![run_command(&mut plugin)];
        for _ in 0..3 {
            replies.push(run_command(&mut plugin));
            replies.push(plugin.call(Call::HandleEvents(Vec::new())));
        }
        replies
    });
    assert!(matches!(replies[0], Err(CallError::Failed(_))));
    assert!(
        replies[1..]
            .iter()
            .all(|replied| replied == &Err(CallError::Unloaded))
    );
    // Only the first call logged "trapping".
    assert_eq!(logged.messages.recv().unwrap().1, "trapping");
    assert_eq!(logged.messages.try_recv(), Err(TryRecvError::Disconnected));
}

#[test]
fn calls_run_on_the_plugin_thread() {
    // grow_memory logs from `run-command`, so its log sink sees the thread the call ran on.
    let (mut plugin, logged) = load("grow_memory", limits());
    let (plugin, replied, caller) = bounded(move || {
        let replied = run_command(&mut plugin);
        (plugin, replied, thread::current().id())
    });
    assert_eq!(replied, Ok(Reply::RunCommand(Ok(()))));
    let (ran_on, _) = logged.messages.recv().unwrap();
    assert_ne!(ran_on, caller);
    assert_ne!(ran_on, thread::current().id());
    assert!(plugin.thread_running());
    assert_eq!(logged.exited.try_recv(), Err(TryRecvError::Empty));

    bounded(move || drop(plugin));
    // Dropping the plugin joined its thread: it has ended, and dropped the store with it.
    assert_eq!(logged.exited.try_recv(), Ok(()));
    assert_eq!(logged.messages.try_recv(), Err(TryRecvError::Disconnected));
}

#[test]
fn wasi_starts_empty() {
    let mut wasi = wasi_context(&Grants::new(), &Workspace::default());
    let mut table = ResourceTable::new();

    let mut filesystem = WasiFilesystemCtxView {
        ctx: wasi.filesystem(),
        table: &mut table,
    };
    assert!(
        preopens::Host::get_directories(&mut filesystem)
            .unwrap()
            .is_empty()
    );

    let mut cli = WasiCliCtxView {
        ctx: wasi.cli(),
        table: &mut table,
    };
    assert_eq!(environment::Host::get_environment(&mut cli).unwrap(), []);
    assert_eq!(
        environment::Host::get_arguments(&mut cli).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(environment::Host::initial_cwd(&mut cli).unwrap(), None);
    assert!(
        terminal_stdin::Host::get_terminal_stdin(&mut cli)
            .unwrap()
            .is_none()
    );
    assert!(
        terminal_stdout::Host::get_terminal_stdout(&mut cli)
            .unwrap()
            .is_none()
    );
    assert!(
        terminal_stderr::Host::get_terminal_stderr(&mut cli)
            .unwrap()
            .is_none()
    );
    // Stdin is closed, not the host's.
    let stdin = stdin::Host::get_stdin(&mut cli).unwrap();
    let stdin = table.get_mut(&stdin).unwrap();
    assert!(matches!(stdin.read(1), Err(StreamError::Closed)));
}

#[test]
fn a_component_over_the_byte_cap_is_refused() {
    let engine = PluginEngine::shared().unwrap();
    let load = |bytes: &[u8]| Plugin::load(engine, bytes, "big", &Grants::new(), limits());
    // Not a component either way: the size check refuses it before compiling would.
    let at_cap = vec![0; MAX_READ_BYTES];
    assert!(matches!(load(&at_cap), Err(LoadError::Invalid { .. })));
    let over = vec![0; MAX_READ_BYTES + 1];
    let error = load(&over).err().unwrap();
    assert!(
        matches!(
            &error,
            LoadError::TooLarge { plugin, bytes, limit }
                if plugin == "big" && *bytes == MAX_READ_BYTES + 1 && *limit == MAX_READ_BYTES
        ),
        "{error:?}"
    );
}
