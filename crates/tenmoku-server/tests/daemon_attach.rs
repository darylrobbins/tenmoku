//! The daemon and `--stdio` attach (D § 3): one daemon per host, user and
//! build, found through a flock'd lock beside its socket; sessions that
//! outlive their link by the grace; teardown of their process groups; and
//! the daemon's exit after its last session.

#![cfg(unix)]

mod common;

use common::{Host, client_config, exists, handshake, lock_held, run_daemon, socket_link};
use fe_remote_proto::{BUILD, Link, SessionToken};
use std::io::{BufRead as _, BufReader, Read as _};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::process::Command;
use std::time::Duration;
use tenmoku_server::{Config, GRACE, LINGER, Paths, TERM_WAIT, TornDown};

const MS: Duration = Duration::from_millis(1);

/// Kills the daemon a test's attach started, whatever the test's outcome.
struct KillDaemon(Option<u32>);

impl Drop for KillDaemon {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            // SAFETY: kill has no memory preconditions.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
}

fn alive(pid: u32) -> bool {
    // SAFETY: kill has no memory preconditions; signal 0 only checks.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[test]
fn stdio_starts_the_daemon_once_and_a_second_attach_reuses_it() {
    let host = Host::new();
    let paths = host.paths();
    assert_eq!(paths.dir, host.runtime.join("tenmoku"));
    assert!(!exists(&paths.socket));

    let (first, first_welcome) = handshake(
        Link::spawn(&mut host.stdio()).unwrap(),
        client_config(),
        None,
    )
    .unwrap();
    let pid = paths
        .daemon_pid()
        .expect("the daemon wrote its pid in the lock");
    let _kill = KillDaemon(Some(pid));
    let mode = std::fs::metadata(&paths.dir).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    assert!(lock_held(&paths));
    // Detached: the leader of a session of its own, with no terminal.
    // SAFETY: getsid has no memory preconditions.
    assert_eq!(
        unsafe { libc::getsid(pid as libc::pid_t) },
        pid as libc::pid_t
    );

    let (second, second_welcome) = handshake(
        Link::spawn(&mut host.stdio()).unwrap(),
        client_config(),
        None,
    )
    .unwrap();
    assert_eq!(paths.daemon_pid(), Some(pid));
    assert!(alive(pid));
    assert_ne!(first_welcome.session, second_welcome.session);

    // Both attaches end; the daemon stays for their sessions' grace.
    first.close();
    second.close();
    assert!(alive(pid));
    assert!(lock_held(&paths));
}

#[test]
fn without_a_runtime_dir_the_daemon_listens_under_the_cache() {
    let host = Host::new();
    let paths = Paths::new(BUILD, None, Some(&host.home)).unwrap();
    assert_eq!(
        paths.dir,
        host.home
            .join(".cache/tenmoku-server")
            .join(BUILD)
            .join("run")
    );
    let mut stdio = host.stdio();
    stdio.env_remove("XDG_RUNTIME_DIR");
    let (client, _) = handshake(Link::spawn(&mut stdio).unwrap(), client_config(), None).unwrap();
    let _kill = KillDaemon(paths.daemon_pid());
    assert!(exists(&paths.socket));
    assert!(lock_held(&paths));
    let mode = std::fs::metadata(&paths.dir).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    client.close();
}

#[test]
fn dropping_stdio_keeps_the_session_for_its_grace() {
    let host = Host::new();
    let config = Config::default();
    assert_eq!(config.grace, Duration::from_secs(10 * 60));
    assert_eq!(GRACE, config.grace);
    let running = run_daemon(&host, config);
    let daemon = &running.daemon;

    let link = Link::spawn(&mut host.stdio()).unwrap();
    let stdio_pid = link.child.as_ref().unwrap().id();
    let (client, welcome) = handshake(link, client_config(), None).unwrap();
    let token = welcome.session;
    assert!(daemon.wait_for(|s| s.session(token).is_some_and(|s| s.attached)));

    // SAFETY: kill has no memory preconditions.
    unsafe { libc::kill(stdio_pid as libc::pid_t, libc::SIGKILL) };
    client.wait_lost();
    assert!(daemon.wait_for(|s| s.session(token).is_some_and(|s| !s.attached)));

    running.clock.advance(GRACE - MS);
    assert!(daemon.settle());
    let held = daemon.snapshot();
    assert!(held.session(token).is_some(), "{held:?}");
    assert!(held.torn_down.is_empty());
    assert!(!held.exited);
    assert!(lock_held(&host.paths()));

    running.clock.advance(MS);
    assert!(daemon.settle());
    let ended = daemon.snapshot();
    assert!(ended.session(token).is_none(), "{ended:?}");
    assert_eq!(ended.torn_down.len(), 1);
    assert_eq!(ended.torn_down[0].session, token);
}

/// A session of `running`'s daemon with no link: its client attached
/// straight to the socket and hung up.
fn detached_session(running: &common::Running, paths: &Paths) -> SessionToken {
    let (client, welcome) = handshake(socket_link(paths), client_config(), None).unwrap();
    let token = welcome.session;
    assert!(
        running
            .daemon
            .wait_for(|s| s.session(token).is_some_and(|s| s.attached))
    );
    drop(client);
    assert!(
        running
            .daemon
            .wait_for(|s| s.session(token).is_some_and(|s| !s.attached))
    );
    token
}

/// Spawns `script` under `sh` as one of `session`'s process groups, and
/// waits for it to say it's ready. Its stdout is the returned reader, which
/// ends once every process in the group has gone.
fn group(
    running: &common::Running,
    session: SessionToken,
    script: &str,
) -> (u32, BufReader<std::io::PipeReader>) {
    let (reader, writer) = std::io::pipe().unwrap();
    let pid = running
        .daemon
        .spawn_group(
            session,
            Command::new("sh").args(["-c", script]).stdout(writer),
        )
        .unwrap();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert_eq!(line, "ready\n");
    (pid, reader)
}

#[test]
fn grace_end_tears_down_the_session() {
    let host = Host::new();
    let grace = Duration::from_secs(60);
    let running = run_daemon(
        &host,
        Config {
            grace,
            ..Config::default()
        },
    );
    let daemon = &running.daemon;
    let (client, welcome) = handshake(socket_link(&host.paths()), client_config(), None).unwrap();
    let token = welcome.session;
    // Each group has a background child, so the signal must reach the
    // whole group for the pipe to close.
    let (quits, mut quits_out) = group(&running, token, "sleep 1000 & echo ready; wait");
    let (stays, mut stays_out) = group(
        &running,
        token,
        "trap '' TERM; sleep 1000 & echo ready; wait",
    );
    drop(client);
    assert!(daemon.wait_for(|s| s.session(token).is_some_and(|s| !s.attached)));

    running.clock.advance(grace);
    assert!(daemon.settle());
    let ending = daemon.snapshot();
    assert!(ending.session(token).is_none());
    assert_eq!(ending.ending, [token]);
    // TERM ends the group that doesn't ignore it.
    quits_out.read_to_end(&mut Vec::new()).unwrap();

    running.clock.advance(TERM_WAIT - MS);
    assert!(daemon.settle());
    let waiting = daemon.snapshot();
    assert_eq!(waiting.ending, [token]);
    assert!(waiting.reaping.is_empty() && waiting.torn_down.is_empty());
    assert!(alive(stays));

    running.clock.advance(MS);
    stays_out.read_to_end(&mut Vec::new()).unwrap();
    let ended = torn_down(daemon, token);
    assert_eq!(exit_signal(&ended, quits), Some(libc::SIGTERM));
    assert_eq!(exit_signal(&ended, stays), Some(libc::SIGKILL));
}

/// Waits for `session`'s teardown to finish, and gives it.
fn torn_down(daemon: &tenmoku_server::Daemon, session: SessionToken) -> TornDown {
    assert!(
        daemon.wait_for(|s| s.ending.is_empty() && s.reaping.is_empty() && !s.torn_down.is_empty())
    );
    let ended = daemon.snapshot();
    let [torn] = &ended.torn_down[..] else {
        panic!("{ended:?}");
    };
    assert_eq!(torn.session, session);
    torn.clone()
}

fn exit_signal(torn: &TornDown, leader: u32) -> Option<i32> {
    let (_, status) = torn.exits.iter().find(|(p, _)| *p == leader).unwrap();
    status.signal()
}

#[test]
fn a_late_teardown_still_waits_between_term_and_kill() {
    let host = Host::new();
    let grace = Duration::from_secs(60);
    let running = run_daemon(
        &host,
        Config {
            grace,
            ..Config::default()
        },
    );
    let daemon = &running.daemon;
    let (client, welcome) = handshake(socket_link(&host.paths()), client_config(), None).unwrap();
    let token = welcome.session;
    let (stays, mut stays_out) = group(
        &running,
        token,
        "trap '' TERM; sleep 1000 & echo ready; wait",
    );
    drop(client);
    assert!(daemon.wait_for(|s| s.session(token).is_some_and(|s| !s.attached)));

    // The first look at the clock comes well past the grace and the wait
    // after it: the TERM still gets its two seconds.
    running
        .clock
        .advance(grace + TERM_WAIT + Duration::from_secs(5));
    assert!(daemon.settle());
    let termed = daemon.snapshot();
    assert_eq!(termed.ending, [token]);
    assert!(termed.reaping.is_empty() && termed.torn_down.is_empty());
    running.clock.advance(TERM_WAIT - MS);
    assert!(daemon.settle());
    let waiting = daemon.snapshot();
    assert_eq!(waiting.ending, [token]);
    assert!(waiting.reaping.is_empty() && waiting.torn_down.is_empty());
    assert!(alive(stays));

    running.clock.advance(MS);
    stays_out.read_to_end(&mut Vec::new()).unwrap();
    assert_eq!(
        exit_signal(&torn_down(daemon, token), stays),
        Some(libc::SIGKILL)
    );
}

#[test]
fn a_group_cant_join_a_session_past_its_grace() {
    let host = Host::new();
    let grace = Duration::from_secs(60);
    // No reaper runs: only the spawn itself can see the grace is over.
    let (daemon, clock) = common::claim_daemon(
        &host,
        Config {
            grace,
            ..Config::default()
        },
    );
    let (client, welcome, served) = common::serve_client(&daemon, None);
    let token = welcome.session;
    client.close();
    assert_eq!(served.join().unwrap().unwrap(), token);

    clock.advance(grace);
    let err = daemon
        .spawn_group(token, Command::new("sh").args(["-c", "exit 0"]))
        .unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    let held = daemon.snapshot();
    assert!(held.session(token).is_none(), "{held:?}");
}

#[test]
fn the_daemon_exits_its_linger_after_its_last_session() {
    let host = Host::new();
    let paths = host.paths();
    let grace = Duration::from_secs(60);
    let config = Config {
        grace,
        ..Config::default()
    };
    assert_eq!(config.linger, Duration::from_secs(60));
    assert_eq!(LINGER, config.linger);
    let running = run_daemon(&host, config);
    let daemon = &running.daemon;
    let token = detached_session(&running, &paths);

    running.clock.advance(grace);
    assert!(daemon.settle());
    assert!(daemon.snapshot().session(token).is_none());

    running.clock.advance(LINGER - MS);
    assert!(daemon.settle());
    assert!(!daemon.snapshot().exited);
    assert!(exists(&paths.socket));
    assert!(lock_held(&paths));

    running.clock.advance(MS);
    assert!(daemon.wait_for(|s| s.exited));
    running.thread.join().unwrap();
    assert!(!exists(&paths.socket));
    assert!(!lock_held(&paths));
}

#[test]
fn a_new_session_in_the_linger_keeps_the_daemon() {
    let host = Host::new();
    let paths = host.paths();
    let running = run_daemon(
        &host,
        Config {
            grace: Duration::from_secs(60),
            ..Config::default()
        },
    );
    let daemon = &running.daemon;
    detached_session(&running, &paths);
    running.clock.advance(Duration::from_secs(60) + LINGER - MS);
    assert!(daemon.settle());
    assert!(daemon.snapshot().sessions.is_empty());

    let (_client, welcome) = handshake(socket_link(&paths), client_config(), None).unwrap();
    running.clock.advance(LINGER);
    assert!(daemon.settle());
    let held = daemon.snapshot();
    assert!(held.session(welcome.session).is_some());
    assert!(!held.exited);
}

#[test]
fn a_second_daemon_backs_off_while_the_lock_is_held() {
    let host = Host::new();
    let _running = run_daemon(&host, Config::default());
    let status = Command::new(common::BIN)
        .arg("daemon")
        .env("XDG_RUNTIME_DIR", &host.runtime)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(host.paths().daemon_pid(), Some(std::process::id()));
}

#[test]
fn a_reaper_thread_that_cant_start_holds_up_no_other_deadline() {
    let host = Host::new();
    let paths = host.paths();
    let grace = Duration::from_secs(60);
    let running = run_daemon(
        &host,
        Config {
            grace,
            ..Config::default()
        },
    );
    let daemon = &running.daemon;
    let script = "trap '' TERM; sleep 1000 & echo ready; wait";
    let (first_client, first) = handshake(socket_link(&paths), client_config(), None).unwrap();
    let (_, mut first_out) = group(&running, first.session, script);
    let (second_client, second) = handshake(socket_link(&paths), client_config(), None).unwrap();
    let (second_leader, mut second_out) = group(&running, second.session, script);
    drop(first_client);
    assert!(daemon.wait_for(|s| s.session(first.session).is_some_and(|s| !s.attached)));
    running.clock.advance(Duration::from_secs(10));
    drop(second_client);
    assert!(daemon.wait_for(|s| s.session(second.session).is_some_and(|s| !s.attached)));

    // The first session's KILL goes, but no thread can reap its leader.
    tenmoku_server::testing::fail_reaper_threads(1);
    running.clock.advance(grace - Duration::from_secs(10));
    assert!(daemon.settle());
    running.clock.advance(TERM_WAIT);
    assert!(daemon.settle());
    first_out.read_to_end(&mut Vec::new()).unwrap();
    let after_first = daemon.snapshot();
    assert!(after_first.reaping.is_empty(), "{after_first:?}");
    let [unreaped] = &after_first.torn_down[..] else {
        panic!("{after_first:?}");
    };
    assert_eq!(unreaped.session, first.session);
    assert!(unreaped.exits.is_empty());

    // The second session's teardown, and the linger, still come.
    running.clock.advance(Duration::from_secs(10));
    assert!(daemon.settle());
    assert_eq!(daemon.snapshot().ending, [second.session]);
    running.clock.advance(TERM_WAIT);
    second_out.read_to_end(&mut Vec::new()).unwrap();
    assert!(daemon.wait_for(|s| s.torn_down.len() == 2 && s.reaping.is_empty()));
    let ended = daemon.snapshot();
    assert_eq!(ended.torn_down[1].session, second.session);
    assert_eq!(
        exit_signal(&ended.torn_down[1], second_leader),
        Some(libc::SIGKILL)
    );
    running.clock.advance(LINGER);
    assert!(daemon.wait_for(|s| s.exited));
}
