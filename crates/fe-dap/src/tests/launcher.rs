// SPDX-License-Identifier: Apache-2.0

//! `LocalLauncher` (§2) against real processes: the `fe-dap-fake` binary
//! over TCP and stdio, the 5 s connect limit on the injected clock, a
//! cancel, and the adapter's own process group.

use std::io;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::clock::ManualClock;
use crate::launcher::{
    AdapterIo, AdapterLauncher, AdapterSpec, AdapterTransport, CONNECT_RETRY, LaunchCancel,
    LocalLauncher, PORT_PLACEHOLDER,
};
use crate::protocol::{DisconnectArguments, InitializeArguments, Message, RequestMessage, request};
use crate::transport::{FrameReader, FrameWriter};

/// The `fe-dap-fake` binary, which cargo builds beside this test binary for
/// the crate's integration tests (cargo sets `CARGO_BIN_EXE_*` only for
/// those, so a unit test finds it by place).
fn fake_binary() -> PathBuf {
    for var in ["NEXTEST_BIN_EXE_fe-dap-fake", "CARGO_BIN_EXE_fe-dap-fake"] {
        if let Some(path) = std::env::var_os(var) {
            return path.into();
        }
    }
    let exe = std::env::current_exe().unwrap();
    // target/<profile>/deps/fe_dap-<hash> → target/<profile>/fe-dap-fake
    let profile = exe.parent().and_then(Path::parent).unwrap();
    let binary = profile.join(format!("fe-dap-fake{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary.is_file(),
        "{} isn't built: cargo build -p fe-dap --features test-support --bin fe-dap-fake",
        binary.display()
    );
    binary
}

fn tcp_spec(args: &[&str]) -> AdapterSpec {
    AdapterSpec {
        transport: AdapterTransport::Tcp,
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        ..AdapterSpec::new("fe-dap-fake", fake_binary())
    }
}

fn cwd() -> PathBuf {
    std::env::temp_dir()
}

/// Runs a launch over TCP and its `initialize` and `disconnect`, checking
/// the adapter exits cleanly.
fn talk(io: AdapterIo) {
    let AdapterIo {
        input,
        output,
        mut process,
        ..
    } = io;
    let mut writer = FrameWriter::new(input);
    let mut reader = FrameReader::new(output);
    async_io::block_on(async {
        let initialize = Message::Request(RequestMessage::new::<request::Initialize>(
            1,
            &InitializeArguments {
                adapter_id: "fake".into(),
                ..Default::default()
            },
        ));
        writer.write(&initialize).await.unwrap();
        let Some(Message::Response(response)) = reader.read().await.unwrap() else {
            panic!("no initialize response");
        };
        assert!(response.success);
        assert_eq!(response.request_seq, 1);
        let capabilities = response.body::<request::Initialize>().unwrap();
        assert_eq!(capabilities.supports_configuration_done_request, Some(true));
        let disconnect = Message::Request(RequestMessage::new::<request::Disconnect>(
            2,
            &DisconnectArguments::default(),
        ));
        writer.write(&disconnect).await.unwrap();
        loop {
            match reader.read().await.unwrap() {
                Some(Message::Response(response)) if response.command == "disconnect" => break,
                Some(_) => {}
                None => panic!("no disconnect response"),
            }
        }
        assert_eq!(reader.read().await.unwrap(), None);
        let exit = process.exited().await;
        assert_eq!(exit.code, Some(0), "{exit:?}");
    });
}

#[test]
fn local_launcher_substitutes_port_and_connects_to_fe_dap_fake() {
    let spec = tcp_spec(&["--port", PORT_PLACEHOLDER]);
    let io = LocalLauncher::new()
        .launch(&spec, &cwd(), &LaunchCancel::new())
        .unwrap();
    let port = io.port.expect("no port");
    assert_ne!(port, 0);
    // stdout isn't the transport over TCP, so it's handed on.
    assert!(io.stdout.is_some());
    talk(io);
}

#[cfg(unix)]
#[test]
fn local_launcher_substitutes_port_inside_an_argument() {
    // As delve's `--listen 127.0.0.1:${port}` has it, inside a longer
    // argument: `port=${port}`, which a shell strips for the fake.
    let spec = AdapterSpec {
        command: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "exec \"$0\" --port \"${1#port=}\"".into(),
            fake_binary().display().to_string(),
            format!("port={PORT_PLACEHOLDER}"),
        ],
        ..tcp_spec(&[])
    };
    let io = LocalLauncher::new()
        .launch(&spec, &cwd(), &LaunchCancel::new())
        .unwrap();
    talk(io);
}

#[test]
fn local_launcher_runs_a_stdio_adapter() {
    let spec = AdapterSpec {
        args: vec!["--stdio".into()],
        ..AdapterSpec::new("fe-dap-fake", fake_binary())
    };
    let io = LocalLauncher::new()
        .launch(&spec, &cwd(), &LaunchCancel::new())
        .unwrap();
    assert_eq!(io.port, None);
    assert!(io.stdout.is_none());
    talk(io);
}

#[test]
fn a_tcp_spec_without_a_port_placeholder_is_refused() {
    let err = LocalLauncher::new()
        .launch(&tcp_spec(&["--port", "4711"]), &cwd(), &LaunchCancel::new())
        .err()
        .expect("launched");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn a_bare_command_not_on_the_path_is_not_found() {
    let spec = AdapterSpec::new("missing", "fe-dap-no-such-adapter");
    let err = LocalLauncher::new()
        .with_path(Some(fake_binary().parent().unwrap().as_os_str().to_owned()))
        .launch(&spec, &cwd(), &LaunchCancel::new())
        .err()
        .expect("launched");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    // Found on the PATH given.
    let spec = AdapterSpec {
        args: vec!["--stdio".into()],
        ..AdapterSpec::new("fe-dap-fake", "fe-dap-fake")
    };
    let io = LocalLauncher::new()
        .with_path(Some(
            std::env::join_paths([
                Path::new("relative/ignored"),
                fake_binary().parent().unwrap(),
            ])
            .unwrap(),
        ))
        .launch(&spec, &cwd(), &LaunchCancel::new())
        .unwrap();
    talk(io);
}

/// A pid file in a fresh directory, and the pid the fake writes to it.
struct PidFile(tempfile::TempDir);

impl PidFile {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }

    fn path(&self) -> String {
        self.0.path().join("pid").display().to_string()
    }

    fn wait(&self) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(text) = std::fs::read_to_string(self.path())
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "the fake never wrote its pid");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Whether process `pid` is gone: no such process, or (on Linux) a zombie
/// waiting for a parent that isn't us.
#[cfg(unix)]
fn gone(pid: u32) -> bool {
    let Some(pid) = rustix::process::Pid::from_raw(pid as i32) else {
        return true;
    };
    if rustix::process::test_kill_process(pid).is_err() {
        return true;
    }
    std::fs::read_to_string(format!("/proc/{}/stat", pid.as_raw_nonzero()))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(')')
                .map(|(_, rest)| rest.trim_start().starts_with('Z'))
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn wait_gone(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gone(pid) {
        assert!(Instant::now() < deadline, "process {pid} is still running");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A launch of a TCP adapter that never listens, on `clock`, on a thread.
fn pending_launch(
    clock: Arc<ManualClock>,
    cancel: LaunchCancel,
    pid_file: &PidFile,
) -> std::thread::JoinHandle<io::Result<AdapterIo>> {
    let spec = tcp_spec(&[
        "--pid-file",
        &pid_file.path(),
        "--never-listen",
        "--port",
        PORT_PLACEHOLDER,
    ]);
    let launcher = LocalLauncher::new().with_clock(clock);
    std::thread::spawn(move || launcher.launch(&spec, &cwd(), &cancel))
}

#[test]
fn connecting_gives_up_after_5_s_on_the_clock() {
    let clock = Arc::new(ManualClock::new());
    let pid_file = PidFile::new();
    let launch = pending_launch(clock.clone(), LaunchCancel::new(), &pid_file);
    let pid = pid_file.wait();
    // It retries on the clock, and real time alone doesn't end it.
    clock.wait_for_sleep(CONNECT_RETRY);
    std::thread::sleep(Duration::from_millis(200));
    assert!(!launch.is_finished());
    // Short of 5 s it is still trying...
    clock.advance(Duration::from_millis(4900));
    clock.wait_for_sleep(CONNECT_RETRY);
    assert!(!launch.is_finished());
    // ...and at 5 s it gives up, and stops the adapter.
    clock.advance(CONNECT_RETRY);
    let err = launch.join().unwrap().err().expect("connected");
    assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
    #[cfg(unix)]
    wait_gone(pid);
    let _ = pid;
}

#[test]
fn cancel_stops_a_pending_launch() {
    let clock = Arc::new(ManualClock::new());
    let pid_file = PidFile::new();
    let cancel = LaunchCancel::new();
    let launch = pending_launch(clock.clone(), cancel.clone(), &pid_file);
    let pid = pid_file.wait();
    clock.wait_for_sleep(CONNECT_RETRY);
    let start = Instant::now();
    cancel.cancel();
    // With no time passing on the clock.
    let err = launch.join().unwrap().err().expect("connected");
    assert_eq!(err.kind(), io::ErrorKind::Interrupted, "{err}");
    assert!(start.elapsed() < Duration::from_secs(5));
    #[cfg(unix)]
    wait_gone(pid);
    let _ = pid;
    // A launch cancelled before it starts starts nothing.
    let err = LocalLauncher::new()
        .launch(&tcp_spec(&["--port", PORT_PLACEHOLDER]), &cwd(), &cancel)
        .err()
        .expect("launched");
    assert_eq!(err.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn an_adapter_exiting_before_it_listens_fails_the_launch() {
    // The fake refuses an unknown argument and exits at once.
    let err = LocalLauncher::new()
        .launch(
            &tcp_spec(&["--bogus", PORT_PLACEHOLDER]),
            &cwd(),
            &LaunchCancel::new(),
        )
        .err()
        .expect("connected");
    assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused, "{err}");
}

#[cfg(unix)]
#[test]
fn the_adapter_runs_in_its_own_process_group() {
    use futures::AsyncBufReadExt as _;

    // The "adapter" starts a child of its own and prints its pid.
    let spec = AdapterSpec {
        args: vec!["-c".into(), "sleep 300 & echo $!; wait".into()],
        ..AdapterSpec::new("group", "/bin/sh")
    };
    let mut io = LocalLauncher::new()
        .launch(&spec, &cwd(), &LaunchCancel::new())
        .unwrap();
    let pid = io.process.id().unwrap();
    let raw = |pid: u32| rustix::process::Pid::from_raw(pid as i32).unwrap();
    let group = rustix::process::getpgid(Some(raw(pid))).unwrap();
    assert_eq!(group, raw(pid), "the adapter doesn't lead its own group");
    assert_ne!(
        group,
        rustix::process::getpgrp(),
        "the adapter shares our group"
    );

    let mut line = String::new();
    let mut output = futures::io::BufReader::new(&mut io.output);
    async_io::block_on(output.read_line(&mut line)).unwrap();
    let child: u32 = line.trim().parse().unwrap();
    assert_eq!(rustix::process::getpgid(Some(raw(child))).unwrap(), group);

    // Killing the adapter kills its group: its child too.
    io.process.kill();
    let exit = async_io::block_on(io.process.exited());
    assert_eq!(exit.signal, Some(9), "{exit:?}");
    wait_gone(child);
}

/// `connect_with` on a thread, on `clock`, with `attempt`.
fn connect_on_thread<F>(
    clock: Arc<ManualClock>,
    attempt: impl FnMut() -> F + Send + 'static,
) -> std::thread::JoinHandle<io::Result<()>>
where
    F: std::future::Future<Output = io::Result<()>> + Send,
{
    std::thread::spawn(move || {
        let spec = AdapterSpec::new("pending", "unused");
        async_io::block_on(crate::launcher::connect_with(
            &*clock,
            &LaunchCancel::new(),
            &spec,
            4711,
            || false,
            attempt,
        ))
    })
}

#[test]
fn a_handshake_that_never_finishes_times_out_at_5_s() {
    let clock = Arc::new(ManualClock::new());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = attempts.clone();
    let connect = connect_on_thread(clock.clone(), move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        futures::future::pending::<io::Result<()>>()
    });
    clock.wait_for_sleep(crate::launcher::CONNECT_TIMEOUT);
    clock.advance(Duration::from_millis(4900));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!connect.is_finished());
    clock.advance(Duration::from_millis(100));
    let err = connect.join().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn no_attempt_starts_once_5_s_have_passed() {
    let clock = Arc::new(ManualClock::new());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = attempts.clone();
    // The first attempt is refused; any later one would connect.
    let connect = connect_on_thread(clock.clone(), move || {
        let first = counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
        async move {
            if first {
                Err(io::ErrorKind::ConnectionRefused.into())
            } else {
                Ok(())
            }
        }
    });
    clock.wait_for_sleep(CONNECT_RETRY);
    // The clock passes 5 s during the retry's sleep.
    clock.advance(Duration::from_secs(6));
    let err = connect.join().unwrap().unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}");
    assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[test]
fn a_tcp_adapter_writing_before_it_listens_is_drained() {
    use futures::AsyncReadExt as _;

    // Far past a pipe's 64 KiB on both streams, before it listens.
    const SIZE: usize = 300_000;
    let spec = AdapterSpec {
        command: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!(
                "head -c {SIZE} /dev/zero; head -c {SIZE} /dev/zero >&2; exec \"$0\" --port \"$1\""
            ),
            fake_binary().display().to_string(),
            PORT_PLACEHOLDER.into(),
        ],
        ..tcp_spec(&[])
    };
    let mut io = LocalLauncher::new()
        .launch(&spec, &cwd(), &LaunchCancel::new())
        .unwrap();
    // What it wrote comes back on the streams, from the start.
    let mut stdout = io.stdout.take().unwrap();
    let mut stderr = io.stderr.take().unwrap();
    let mut out = vec![1; SIZE];
    let mut err = vec![1; SIZE];
    async_io::block_on(async {
        stdout.read_exact(&mut out).await.unwrap();
        stderr.read_exact(&mut err).await.unwrap();
    });
    assert!(out.iter().all(|&byte| byte == 0));
    assert!(err.iter().all(|&byte| byte == 0));
    talk(io);
}

#[test]
fn a_connection_made_as_the_deadline_passes_is_refused() {
    use std::future::Future as _;
    use std::task::{Context, Poll};

    let spec = AdapterSpec::new("late", "unused");
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    // One attempt, which completes when `connect` is sent to.
    let attempt_once = |receiver: futures::channel::oneshot::Receiver<()>| {
        let mut receiver = Some(receiver);
        move || {
            let receiver = receiver.take().expect("a second attempt");
            async move { receiver.await.map_err(|_| io::Error::other("dropped")) }
        }
    };

    // Pending, then the connection and the deadline in the same poll.
    let clock = ManualClock::new();
    let cancel = LaunchCancel::new();
    let (connect, connected) = futures::channel::oneshot::channel();
    let mut loop_ = pin!(crate::launcher::connect_with(
        &clock,
        &cancel,
        &spec,
        4711,
        || false,
        attempt_once(connected),
    ));
    assert!(loop_.as_mut().poll(&mut cx).is_pending());
    clock.advance(Duration::from_secs(6));
    connect.send(()).unwrap();
    match loop_.as_mut().poll(&mut cx) {
        Poll::Ready(Err(err)) => assert_eq!(err.kind(), io::ErrorKind::TimedOut, "{err}"),
        other => panic!("not timed out: {other:?}"),
    }

    // The same with no time passed connects.
    let clock = ManualClock::new();
    let (connect, connected) = futures::channel::oneshot::channel();
    let mut loop_ = pin!(crate::launcher::connect_with(
        &clock,
        &cancel,
        &spec,
        4711,
        || false,
        attempt_once(connected),
    ));
    assert!(loop_.as_mut().poll(&mut cx).is_pending());
    connect.send(()).unwrap();
    assert!(matches!(loop_.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));

    // And with a cancel in the same poll, it is cancelled.
    let clock = ManualClock::new();
    let cancel = LaunchCancel::new();
    let (connect, connected) = futures::channel::oneshot::channel();
    let mut loop_ = pin!(crate::launcher::connect_with(
        &clock,
        &cancel,
        &spec,
        4711,
        || false,
        attempt_once(connected),
    ));
    assert!(loop_.as_mut().poll(&mut cx).is_pending());
    cancel.cancel();
    connect.send(()).unwrap();
    match loop_.as_mut().poll(&mut cx) {
        Poll::Ready(Err(err)) => assert_eq!(err.kind(), io::ErrorKind::Interrupted, "{err}"),
        other => panic!("not cancelled: {other:?}"),
    }
}

/// A pipe that is always readable, forever.
struct AlwaysReady;

impl futures::AsyncRead for AlwaysReady {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<io::Result<usize>> {
        buf.fill(b'x');
        std::task::Poll::Ready(Ok(buf.len()))
    }
}

/// A connection loop that never connects, run on a thread while draining
/// two always-readable pipes; its result and what was kept of each.
fn draining_forever(
    clock: Arc<ManualClock>,
    cancel: LaunchCancel,
) -> std::sync::mpsc::Receiver<(io::Result<()>, usize, usize)> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let spec = AdapterSpec::new("chatty", "unused");
        let connect = crate::launcher::connect_with(
            &*clock,
            &cancel,
            &spec,
            4711,
            || false,
            futures::future::pending::<io::Result<()>>,
        );
        let (result, out, err) = async_io::block_on(crate::launcher::while_draining(
            connect,
            Some(&mut AlwaysReady),
            Some(&mut AlwaysReady),
            "chatty",
        ));
        let _ = sender.send((result, out.len(), err.len()));
    });
    receiver
}

#[test]
fn draining_an_always_readable_pipe_lets_the_deadline_and_cancel_through() {
    use crate::launcher::{CONNECT_TIMEOUT, STARTUP_OUTPUT_LIMIT};

    let limit = Duration::from_secs(10);
    // A cancel gets through, and both pipes were drained, within the limit.
    let cancel = LaunchCancel::new();
    let done = draining_forever(Arc::new(ManualClock::new()), cancel.clone());
    std::thread::sleep(Duration::from_millis(50));
    cancel.cancel();
    let (result, out, err) = done
        .recv_timeout(limit)
        .expect("the cancel never got through");
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert!(out > 0 && out <= STARTUP_OUTPUT_LIMIT, "{out}");
    assert!(err > 0 && err <= STARTUP_OUTPUT_LIMIT, "{err}");

    // So does the deadline.
    let clock = Arc::new(ManualClock::new());
    let done = draining_forever(clock.clone(), LaunchCancel::new());
    clock.wait_for_sleep(CONNECT_TIMEOUT);
    clock.advance(CONNECT_TIMEOUT);
    let (result, _, _) = done
        .recv_timeout(limit)
        .expect("the deadline never got through");
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
}
