//! Cancelling launches in progress, and the manager's kill switch, which
//! force-stops its servers without the graceful shutdown (§3.1, §3.4). The
//! manager's clock is manual and never moves, so nothing here waits out a
//! timeout: what ends a server ends it at once.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fe_buffer::Buffer;
use futures::FutureExt;

use super::{Events, ROOT, WAIT, manager};
use crate::fake::{FakeLauncher, FakeServer, InitializeReply, Script};
use crate::{
    Clock, LaunchCancel, LocalLauncher, LoginPath, LspEvent, LspManager, ManualClock, ServerId,
    ServerIo, ServerLauncher, ServerSpec, ServerStatus, SystemClock,
};

/// Hands launches to the fake, but blocks those of a spec named `held`
/// until their cancel fires, then gives up with `Interrupted`. Not
/// cancelled within `WAIT / 2`, it panics.
struct Cooperating {
    fake: Arc<FakeLauncher>,
    entered: Mutex<Sender<()>>,
}

impl ServerLauncher for Cooperating {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        if spec.name != "held" {
            return self.fake.launch(spec, root, cancel);
        }
        let _ = self.entered.lock().unwrap().send(());
        // Well within a test's wait for the status, so a cancel that never
        // comes shows as the panic's `Stopped(Some(..))`.
        let cancelled = async_io::block_on(futures::future::select(
            Box::pin(cancel.cancelled()),
            SystemClock.sleep(WAIT / 2),
        ));
        assert!(
            matches!(cancelled, futures::future::Either::Left(_)),
            "the launch was never cancelled"
        );
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
    }
}

/// Hands launches to the fake once released, ignoring their cancel.
struct Ignoring {
    fake: Arc<FakeLauncher>,
    entered: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
}

impl ServerLauncher for Ignoring {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        let _ = self.entered.lock().unwrap().send(());
        let _ = self.release.lock().unwrap().recv_timeout(WAIT);
        self.fake.launch(spec, root, cancel)
    }
}

/// A manager over `launcher` on a clock that never moves.
fn still(launcher: Arc<dyn ServerLauncher>) -> (LspManager, Events) {
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new());
    manager(launcher, Some(clock))
}

fn cooperating(fake: &FakeServer) -> (Arc<Cooperating>, Receiver<()>) {
    let (entered, entering) = channel();
    let launcher = Arc::new(Cooperating {
        fake: fake.launcher(),
        entered: Mutex::new(entered),
    });
    (launcher, entering)
}

fn start(manager: &mut LspManager, events: &Events, name: &str) -> ServerId {
    let id = manager
        .start(ServerSpec::new(name, "fake-server"), Path::new(ROOT))
        .unwrap();
    assert_eq!(events.status(id), ServerStatus::Starting);
    id
}

/// What each of `servers` reported last, once each has reported
/// `Stopped`.
fn stopped(events: &Events, servers: &[ServerId]) -> HashMap<ServerId, ServerStatus> {
    let mut last = HashMap::new();
    events.until("every server stopped", |event| {
        if let LspEvent::Status { server, status, .. } = event
            && servers.contains(&server)
        {
            last.insert(server, status);
        }
        let all = servers
            .iter()
            .all(|server| matches!(last.get(server), Some(ServerStatus::Stopped(_))));
        all.then_some(())
    });
    last
}

fn sent(fake: &FakeServer, method: &str) -> bool {
    fake.records().iter().any(|r| r.method() == method)
}

#[test]
fn stop_cancels_a_blocked_launch() {
    for drop_it in [false, true] {
        let fake = FakeServer::new(Script::default());
        let (launcher, entering) = cooperating(&fake);
        let (mut manager, events) = still(launcher);
        let id = start(&mut manager, &events, "held");
        entering.recv_timeout(WAIT).unwrap();
        if drop_it {
            drop(manager);
        } else {
            manager.stop(id);
        }
        assert_eq!(events.status(id), ServerStatus::Stopped(None), "{drop_it}");
        assert!(!sent(&fake, "initialize"));
        assert_eq!(fake.launches(), 0);
    }
}

#[test]
fn kill_switch_skips_the_graceful_shutdown() {
    let fake = FakeServer::new(Script {
        answer_shutdown: false,
        ..Script::default()
    });
    let (mut manager, events) = still(fake.launcher());
    let id = start(&mut manager, &events, "fake");
    fake.wait_initialized();
    let switch = manager.kill_switch();
    drop(manager);
    // Dropping the manager only asks for a shutdown, which never answers.
    fake.wait_for("shutdown", |records| {
        records.iter().any(|r| r.method() == "shutdown")
    });
    assert_eq!(fake.kills(), 0);

    switch.kill();
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(fake.kills(), 1);
    assert!(!sent(&fake, "exit"));
}

#[test]
fn kill_switch_ends_launches_and_restarts() {
    let fake = FakeServer::new(Script::default());
    let (launcher, entering) = cooperating(&fake);
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = manager(launcher, Some(clock.clone()));
    // One crashes, and waits out its 1 s backoff, which never passes.
    let restarting = start(&mut manager, &events, "fake");
    fake.wait_initialized();
    fake.crash();
    assert_eq!(events.status(restarting), ServerStatus::Restarting(1));
    clock.wait_for_sleep(Duration::from_secs(1));
    // One is held in a launch that watches its cancel.
    let launching = start(&mut manager, &events, "held");
    entering.recv_timeout(WAIT).unwrap();
    let launches = fake.launches();

    let switch = manager.kill_switch();
    switch.kill();
    let last = stopped(&events, &[restarting, launching]);
    assert_eq!(last[&restarting], ServerStatus::Stopped(None));
    assert_eq!(last[&launching], ServerStatus::Stopped(None));
    assert_eq!(fake.launches(), launches);

    // It stays thrown: a server started afterwards launches nothing.
    let later = start(&mut manager, &events, "later");
    assert_eq!(events.status(later), ServerStatus::Stopped(None));
    assert_eq!(fake.launches(), launches);
}

#[test]
fn a_launch_ignoring_cancel_is_killed_when_it_returns() {
    let fake = FakeServer::new(Script::default());
    let (entered, entering) = channel();
    let (release, released) = channel();
    let launcher = Arc::new(Ignoring {
        fake: fake.launcher(),
        entered: Mutex::new(entered),
        release: Mutex::new(released),
    });
    let (mut manager, events) = still(launcher);
    let id = start(&mut manager, &events, "fake");
    entering.recv_timeout(WAIT).unwrap();

    manager.kill_switch().kill();
    release.send(()).unwrap();
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert_eq!(fake.launches(), 1);
    assert_eq!(fake.kills(), 1);
    assert!(!sent(&fake, "initialize"));
}

#[test]
fn local_launcher_honours_a_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("ran");
    #[cfg(unix)]
    let mut spec = ServerSpec::new("touch", "/usr/bin/touch");
    #[cfg(windows)]
    let mut spec = {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let mut spec = ServerSpec::new("mkdir", format!(r"{root}\System32\cmd.exe"));
        spec.args = vec!["/c".into(), "mkdir".into()];
        spec
    };
    spec.args.push(marker.to_string_lossy().into_owned());
    let cancel = LaunchCancel::new();
    assert!(cancel.cancelled().now_or_never().is_none());
    cancel.cancel();
    assert!(cancel.is_cancelled());
    assert!(cancel.cancelled().now_or_never().is_some());

    let with_path = LocalLauncher::with_login_path(Arc::new(LoginPath::new("/bin/sh")));
    let launchers: [&dyn ServerLauncher; 2] = [&LocalLauncher, &with_path];
    for launcher in launchers {
        match launcher.launch(&spec, dir.path(), &cancel) {
            Err(err) => assert_eq!(err.kind(), io::ErrorKind::Interrupted, "{err}"),
            Ok(_) => panic!("a cancelled launch spawned"),
        }
    }
    assert!(!marker.exists());

    // Not cancelled, the same spec runs.
    let mut io = LocalLauncher
        .launch(&spec, dir.path(), &LaunchCancel::new())
        .unwrap();
    async_io::block_on(io.process.exited());
    assert!(marker.exists());
}

#[test]
fn stop_during_initialize_is_prompt() {
    for how in ["stop", "drop", "kill"] {
        let fake = FakeServer::new(Script {
            initialize: InitializeReply::Never,
            ..Script::default()
        });
        let (mut manager, events) = still(fake.launcher());
        let id = start(&mut manager, &events, "fake");
        fake.wait_for("initialize", |records| {
            records.iter().any(|r| r.method() == "initialize")
        });
        match how {
            "stop" => manager.stop(id),
            "drop" => drop(manager),
            _ => manager.kill_switch().kill(),
        }
        assert_eq!(events.status(id), ServerStatus::Stopped(None), "{how}");
        assert_eq!(fake.kills(), 1, "{how}");
        assert!(!sent(&fake, "shutdown"), "{how}");
    }
}

#[test]
fn a_restart_cancels_the_old_instances_launch() {
    let fake = FakeServer::new(Script::default());
    let (launcher, entering) = cooperating(&fake);
    let (mut manager, events) = still(launcher);
    let mut spec = ServerSpec::new("held", "held-server");
    spec.languages = vec!["held".into()];
    manager.register_spec(spec);
    let mut buffer = Buffer::new("x\n");
    let old = manager
        .open(
            Some(Path::new(ROOT)),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &Path::new(ROOT).join("a.held"),
            "held",
        )
        .unwrap()
        .unwrap();
    assert_eq!(events.status(old), ServerStatus::Starting);
    entering.recv_timeout(WAIT).unwrap();

    // A routed server's restart retires the old instance.
    let new = manager.restart("held", Path::new(ROOT)).unwrap().unwrap();
    let last = stopped(&events, &[old]);
    assert_eq!(last[&old], ServerStatus::Stopped(None));
    entering.recv_timeout(WAIT).unwrap();
    drop(manager);
    assert_eq!(stopped(&events, &[new])[&new], ServerStatus::Stopped(None));
    assert_eq!(fake.launches(), 0);
}

/// Hands launches to the fake, with a stderr that's always ready, as when
/// something the server left behind keeps writing it.
struct EndlessStderr(Arc<FakeLauncher>);

impl ServerLauncher for EndlessStderr {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        let mut io = self.0.launch(spec, root, cancel)?;
        io.stderr = Some(Box::pin(futures::io::repeat(b'!')));
        Ok(io)
    }
}

/// Waits until every server thread has ended, the manager having gone:
/// the sink's last holders.
fn threads_ended(events: &Events) {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match events.0.recv_timeout(left) {
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("the server threads didn't end in {WAIT:?}")
            }
        }
    }
}

/// Lets the client's next stepped write through, once it's waiting.
fn let_through(fake: &FakeServer) {
    super::wait_until("a write", || fake.write_waiting());
    fake.allow_write();
}

#[test]
fn kill_switch_wins_over_an_exit_it_comes_with() {
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = still(fake.launcher());
    let id = start(&mut manager, &events, "fake");
    fake.wait_initialized();
    let switch = manager.kill_switch();
    fake.set_writes(crate::fake::Writes::Stepped);
    drop(manager);
    // `shutdown` (a header, then a body) is answered; the actor then waits
    // for the process, and the main loop writes `exit`. A stepped write
    // blocks the server's thread itself, which the actor shares (join3), so
    // while it's held the actor can't look at anything: the leader exits and
    // the switch is thrown, and both are ready when it next looks.
    let_through(&fake);
    let_through(&fake);
    super::wait_until("exit's write", || fake.write_waiting());
    assert!(sent(&fake, "shutdown") && !sent(&fake, "exit"));
    fake.exit_keeping_output(0);
    switch.kill();
    assert!(fake.write_waiting(), "the server thread moved on");
    assert_eq!(fake.kills(), 0);
    fake.set_writes(crate::fake::Writes::Open);
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    threads_ended(&events);
    assert_eq!(fake.kills(), 1);
}

#[test]
fn kill_switch_ends_a_crash_cleanup() {
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = still(Arc::new(EndlessStderr(fake.launcher())));
    let id = start(&mut manager, &events, "fake");
    fake.wait_initialized();
    // The crash's last words never end, and the clock never moves.
    fake.crash();
    events.assert_quiet();
    assert!(!manager.thread_finished(id));

    manager.kill_switch().kill();
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    super::wait_until("the thread's end", || manager.thread_finished(id));
    assert_eq!(fake.launches(), 1);
    assert!(fake.kills() >= 1);
}

#[test]
fn kill_switch_reaps_after_a_graceful_exit() {
    let fake = FakeServer::new(Script::default());
    let (mut manager, events) = still(Arc::new(EndlessStderr(fake.launcher())));
    let id = start(&mut manager, &events, "fake");
    fake.wait_initialized();
    let switch = manager.kill_switch();
    // It shuts down and exits, but its stderr stays ready: the thread reads
    // on, and what's left of the process may still be running.
    drop(manager);
    assert_eq!(events.status(id), ServerStatus::Stopped(None));
    assert!(sent(&fake, "exit"));
    assert_eq!(fake.kills(), 0);
    events.assert_quiet();

    switch.kill();
    threads_ended(&events);
    assert_eq!(fake.kills(), 1);
}

/// A server on a still clock whose definitions are never answered, so
/// abandoning more than the orphan budget recycles it: a graceful
/// shutdown and exit, then a restart, as after a crash.
fn recycling() -> super::Clocked {
    super::Clocked::new(
        ServerSpec::new("fake", "fake-server"),
        Script {
            replies: HashMap::from([(
                "textDocument/definition".to_owned(),
                crate::fake::Scripted::now(crate::fake::Reply::Never),
            )]),
            ..Script::default()
        },
    )
}

/// Recycles launch `launch` of `s`, and returns its next status that isn't
/// `Starting`.
fn recycle(s: &super::Clocked, launch: usize) -> ServerStatus {
    drop(super::cancel::sent_definitions(
        s,
        launch,
        crate::ORPHANS_MAX + 1,
    ));
    loop {
        match s.events.status(s.server) {
            ServerStatus::Starting => {}
            status => return status,
        }
    }
}

/// Throws `switch` from the sink, on the server's thread, when it reports
/// a status `when` picks.
fn throw_on(
    s: &super::Clocked,
    switch: crate::KillSwitch,
    when: impl Fn(&ServerStatus) -> bool + Send + 'static,
) {
    let server = s.server;
    s.events.probe(move |event| {
        if let LspEvent::Status {
            server: from,
            status,
            ..
        } = event
            && *from == server
            && when(status)
        {
            switch.kill();
        }
    });
}

#[test]
fn kill_switch_thrown_as_a_recycled_server_restarts_reaps_it() {
    let s = recycling();
    let restarted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = restarted.clone();
    throw_on(&s, s.manager.kill_switch(), move |status| match status {
        ServerStatus::Restarting(_) => {
            seen.store(true, std::sync::atomic::Ordering::SeqCst);
            false
        }
        ServerStatus::Starting => seen.load(std::sync::atomic::Ordering::SeqCst),
        _ => false,
    });
    assert_eq!(recycle(&s, 1), ServerStatus::Restarting(1));
    assert!(sent(&s.fake, "exit"));
    assert_eq!(s.fake.kills(), 0);

    // Its backoff ends, and the switch is thrown as it says `Starting`.
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.expect(ServerStatus::Starting);
    assert!(restarted.load(std::sync::atomic::Ordering::SeqCst));
    s.expect(ServerStatus::Stopped(None));
    super::wait_until("the thread's end", || s.manager.thread_finished(s.server));
    assert_eq!(s.fake.launches(), 1);
    assert_eq!(s.fake.kills(), 1);
}

#[test]
fn kill_switch_thrown_as_a_recycled_server_gives_up_reaps_it() {
    let s = recycling();
    for n in 1..=5u32 {
        s.fake.crash();
        assert_eq!(s.events.status(s.server), ServerStatus::Restarting(n));
        let backoff = Duration::from_secs(1 << (n - 1));
        s.clock.wait_for_sleep(backoff);
        s.clock.advance(backoff);
        s.expect(ServerStatus::Starting);
        s.fake.wait_initialized_at(n as usize + 1);
    }
    let crash_kills = s.fake.kills();
    throw_on(&s, s.manager.kill_switch(), |status| {
        matches!(status, ServerStatus::Stopped(Some(_)))
    });
    // The sixth crash in the window, a graceful recycle, gives up.
    assert!(matches!(recycle(&s, 6), ServerStatus::Stopped(Some(_))));
    super::wait_until("the thread's end", || s.manager.thread_finished(s.server));
    assert_eq!(s.fake.kills(), crash_kills + 1);
}

#[test]
fn kill_switch_spares_a_replaced_sessions_process() {
    let s = recycling();
    assert_eq!(recycle(&s, 1), ServerStatus::Restarting(1));
    assert_eq!(s.fake.kills(), 0);
    // The replacement launches and runs; the first exited gracefully, and
    // its process-group id may since have gone to another process.
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.expect(ServerStatus::Starting);
    s.fake.wait_initialized_at(2);

    s.manager.kill_switch().kill();
    s.expect(ServerStatus::Stopped(None));
    super::wait_until("the thread's end", || s.manager.thread_finished(s.server));
    // Only the replacement is killed.
    assert_eq!(s.fake.launches(), 2);
    assert_eq!(s.fake.kills(), 1);
}

/// Hands each launch to the fake its spec's name picks.
struct Named(HashMap<&'static str, Arc<dyn ServerLauncher>>);

impl ServerLauncher for Named {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        self.0[spec.name.as_str()].launch(spec, root, cancel)
    }
}

#[test]
fn kill_switch_counts_servers_whose_threads_have_not_ended() {
    let (a, b) = (
        FakeServer::new(Script::default()),
        FakeServer::new(Script::default()),
    );
    let launcher = Named(HashMap::from([
        (
            "a",
            Arc::new(EndlessStderr(a.launcher())) as Arc<dyn ServerLauncher>,
        ),
        (
            "b",
            Arc::new(EndlessStderr(b.launcher())) as Arc<dyn ServerLauncher>,
        ),
    ]));
    let (mut manager, events) = still(Arc::new(launcher));
    let switch = manager.kill_switch();
    assert_eq!(switch.unfinished(), 0);
    let first = start(&mut manager, &events, "a");
    let second = start(&mut manager, &events, "b");
    a.wait_initialized();
    b.wait_initialized();
    assert_eq!(switch.unfinished(), 2);

    // One has stopped but its endless stderr keeps its thread cleaning up,
    // the other still runs: neither is finished.
    manager.stop(first);
    assert_eq!(events.status(first), ServerStatus::Stopped(None));
    assert_eq!(switch.unfinished(), 2);

    // Both have reported `Stopped`, and neither thread has ended.
    drop(manager);
    assert_eq!(events.status(second), ServerStatus::Stopped(None));
    assert_eq!(switch.unfinished(), 2);

    switch.kill();
    super::wait_until("both threads' end", || switch.unfinished() == 0);
}
