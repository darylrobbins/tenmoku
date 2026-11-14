// SPDX-License-Identifier: Apache-2.0

//! `LocalLauncher` (§2) against real processes: the `fe-dap-fake` binary
//! over TCP and stdio, the 5 s connect limit on the injected clock, a
//! cancel, the adapter's own process group, and the output it writes
//! before it listens. An integration test so cargo hands over the fake's
//! path (`CARGO_BIN_EXE_fe-dap-fake`), as fe-acp's launch tests have
//! theirs: a unit test found it beside its own binary, which isn't where
//! cargo puts it under a separate `build.build-dir`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fe_dap::ManualClock;
use fe_dap::launcher::{
    AdapterIo, AdapterLauncher, AdapterSpec, AdapterTransport, CONNECT_RETRY, LaunchCancel,
    LocalLauncher, PORT_PLACEHOLDER,
};
use fe_dap::protocol::{
    DisconnectArguments, InitializeArguments, Message, RequestMessage, request,
};
use fe_dap::transport::{FrameReader, FrameWriter};

/// The `fe-dap-fake` binary, which cargo builds for this crate's
/// integration tests and names here, wherever its target or build dir is.
fn fake_binary() -> PathBuf {
    env!("CARGO_BIN_EXE_fe-dap-fake").into()
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
