// SPDX-License-Identifier: Apache-2.0

//! The registry's `Port` adapters (§3; card D3) against a real process:
//! the `fe-dap-fake` binary, started as a host (jdtls for java-debug)
//! starts its adapter, listening on the port it is given. An integration
//! test so cargo hands over the fake's path (`CARGO_BIN_EXE_fe-dap-fake`),
//! as `tests/launcher.rs` has it: a unit test found it beside its own
//! binary, which isn't where cargo puts it under a separate
//! `build.build-dir`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use fe_dap::ManualClock;
use fe_dap::fake::Program;
use fe_dap::launcher::{
    AdapterIo, AdapterLauncher, AdapterSpec, AdapterTransport, LaunchCancel, LocalLauncher,
};
use fe_dap::protocol::{
    DisconnectArguments, InitializeArguments, Message, RequestMessage, SourceBreakpoint, request,
};
use fe_dap::registry::{Adapter, Context, HostOs, Registry, Source, Status};
use fe_dap::session::{DapEvent, EndReason, Session, SessionOptions, SessionState};
use fe_dap::transport::{FrameReader, FrameWriter};
use fe_lsp::LoginPath;

/// The `fe-dap-fake` binary, which cargo builds for this crate's
/// integration tests and names here, wherever its target or build dir is.
fn fake_binary() -> PathBuf {
    env!("CARGO_BIN_EXE_fe-dap-fake").into()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// Options for the fake's default program over `adapter`: `launch` with a
/// breakpoint on its `let y = add` line.
fn options_with_breakpoint(adapter: AdapterSpec) -> SessionOptions {
    let program = Program::default();
    let line = program.line_of("let y = add");
    let serde_json::Value::Object(arguments) = serde_json::json!({ "program": "/fake/main" })
    else {
        unreachable!()
    };
    SessionOptions {
        adapter,
        cwd: "/fake".into(),
        arguments,
        breakpoints: [(
            program.path,
            vec![SourceBreakpoint {
                line,
                ..Default::default()
            }],
        )]
        .into(),
        ..Default::default()
    }
}

/// A launcher handing out one prepared [`AdapterIo`].
struct IoLauncher(std::sync::Mutex<Option<AdapterIo>>);

impl AdapterLauncher for IoLauncher {
    fn launch(
        &self,
        _: &AdapterSpec,
        _: &std::path::Path,
        _: &LaunchCancel,
    ) -> std::io::Result<AdapterIo> {
        self.0
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| std::io::Error::other("launched once"))
    }
}

/// A [`Session`] on one prepared [`AdapterIo`] and a [`ManualClock`] that
/// never moves, its states collected for the test to wait on.
struct Harness {
    session: Session,
    states: Receiver<SessionState>,
}

impl Harness {
    fn start_on(io: AdapterIo, options: SessionOptions) -> Self {
        let (sender, states) = channel();
        let launcher = Arc::new(IoLauncher(std::sync::Mutex::new(Some(io))));
        let session = Session::start(options, launcher, Arc::new(ManualClock::new()), {
            move |event| {
                if let DapEvent::State(state) = event {
                    let _ = sender.send(state);
                }
            }
        })
        .expect("the session thread starts");
        Self { session, states }
    }

    /// The first state from now on that is `wanted`. Panics after 10 s.
    fn wait_for(&mut self, what: &str, wanted: impl Fn(&SessionState) -> bool) -> SessionState {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.states.recv_timeout(left) {
                Ok(state) if wanted(&state) => return state,
                Ok(state) => seen.push(state),
                Err(RecvTimeoutError::Timeout) => panic!("never got to {what}: {seen:?}"),
                Err(RecvTimeoutError::Disconnected) => panic!("no {what}: {seen:?}"),
            }
        }
    }
}

#[test]
fn a_server_port_adapter_connects_to_the_port_its_host_gives() {
    // The host starts the adapter and learns its port, as jdtls gives
    // java-debug's: here fe-dap-fake, listening on a port picked for it.
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut host_adapter = std::process::Command::new(fake_binary())
        .args(["--port", &port.to_string()])
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let login = Arc::new(LoginPath::new("/nonexistent-shell"));
    let mut registry = Registry::new(login).with_os(HostOs::Linux);
    registry.register(AdapterSpec {
        name: "java-debug".into(),
        transport: AdapterTransport::Port(port),
        languages: strings(&["java"]),
        types: strings(&["java"]),
        ..AdapterSpec::default()
    });
    let context = Context {
        root: std::env::temp_dir(),
        language: None,
        virtual_env: None,
    };
    let Adapter {
        source,
        spec,
        status,
    } = registry.find("java-debug", &context).unwrap();
    // Nothing to find: the host started it.
    assert_eq!((source, status), (Source::Host, Status::Found));
    // The `java` type is the built-in java's by its name (D21b), which a
    // name beats a type for.
    let builtin = registry.find("java", &context).unwrap();
    assert_eq!(
        (builtin.source, builtin.spec.name.as_str()),
        (Source::Builtin, "java")
    );
    assert_eq!(spec.transport, AdapterTransport::Port(port));
    assert_eq!(spec.command, PathBuf::new());

    let io = LocalLauncher::new()
        .launch(&spec, &context.root, &LaunchCancel::new())
        .unwrap();
    assert_eq!(io.port, Some(port));
    let AdapterIo {
        input,
        output,
        stderr,
        stdout,
        mut process,
        ..
    } = io;
    // No process of fe-dap's: the adapter is the host's.
    assert!(stderr.is_none() && stdout.is_none());
    assert_eq!(process.id(), None);
    let mut writer = FrameWriter::new(input);
    let mut reader = FrameReader::new(output);
    async_io::block_on(async {
        let initialize = Message::Request(RequestMessage::new::<request::Initialize>(
            1,
            &InitializeArguments {
                adapter_id: "java".into(),
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
        // The adapter closed the connection: that alone is its end, with
        // no kill.
        process.exited().await;
        // A kill after it changes nothing.
        process.kill();
        process.exited().await;
    });
    let exit = host_adapter.wait().unwrap();
    assert!(exit.success(), "{exit:?}");
}

/// A session over a Port adapter ends as soon as the adapter closes its
/// connection after `disconnect`: no kill, and no exit grace, on a clock
/// that never moves.
#[test]
fn a_port_session_ends_when_the_adapter_closes_its_connection() {
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut host_adapter = std::process::Command::new(fake_binary())
        .args(["--port", &port.to_string()])
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let spec = AdapterSpec {
        name: "java-debug".into(),
        transport: AdapterTransport::Port(port),
        ..AdapterSpec::default()
    };
    let io = LocalLauncher::new()
        .launch(&spec, &std::env::temp_dir(), &LaunchCancel::new())
        .unwrap();
    let mut harness = Harness::start_on(io, options_with_breakpoint(spec));
    harness.wait_for("Stopped", |state| *state == SessionState::Stopped);
    harness.session.stop();
    let ended = match harness.wait_for("the end", SessionState::is_ended) {
        SessionState::Ended(ended) => ended,
        other => panic!("not the end: {other:?}"),
    };
    // Its clock never moved, so no grace timer ended it.
    assert_eq!(ended.reason, EndReason::Stopped);
    let exit = host_adapter.wait().unwrap();
    assert!(exit.success(), "{exit:?}");
}
