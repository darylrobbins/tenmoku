// SPDX-License-Identifier: Apache-2.0

//! fe-dap's wire layer against real adapters (docs/decisions/r1b9dw3g0g-debugger.md
//! §11 D16): debugpy and an LLDB adapter each take a fixture through one
//! breakpoint, a step, variables and an evaluate, and the tests record the
//! message orders the adapter sends (§2's ordering claims). `#[ignore]`d:
//! `scripts/dap-real.sh` runs them.
//!
//! The driver speaks the protocol directly (framing, launcher, typed
//! messages) so it sees every message in the order it arrives.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use fe_dap::launcher::{
    AdapterIo, AdapterLauncher, AdapterProcess, AdapterSpec, AdapterTransport, LaunchCancel,
    LocalLauncher, PORT_PLACEHOLDER,
};
use fe_dap::protocol::{
    ContinueArguments, DisconnectArguments, Empty, EvaluateArguments, EventMessage,
    InitializeArguments, Message, Object, Request, RequestMessage, ResponseMessage,
    ScopesArguments, SetBreakpointsArguments, Source, SourceBreakpoint, StackTraceArguments,
    StepArguments, StoppedBody, Variable, VariablesArguments, event, request,
};
use fe_dap::transport::{FrameReader, FrameWriter};

const WAIT: Duration = Duration::from_secs(60);

fn require() -> bool {
    std::env::var_os("FE_DAP_REQUIRE_ADAPTERS").is_some_and(|v| v == "1")
}

/// Skips (passes) with `reason`, or fails when adapters are required: for a
/// missing adapter.
fn skip(reason: &str) {
    assert!(!require(), "required adapter unavailable: {reason}");
    eprintln!("skipped: {reason}");
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real")
}

/// One framed client talking to an adapter, logging the order of what it
/// sees.
struct Client {
    writer: FrameWriter<Pin<Box<dyn futures::AsyncWrite + Send>>>,
    rx: mpsc::Receiver<Message>,
    inbox: VecDeque<Message>,
    process: Box<dyn AdapterProcess>,
    seq: i64,
    /// `req:<command>`, `resp:<command>` and `evt:<event>`, in order.
    log: Vec<String>,
    _stdout: Option<Pin<Box<dyn futures::AsyncRead + Send>>>,
}

impl Client {
    fn new(io: AdapterIo) -> Self {
        let AdapterIo {
            input,
            output,
            process,
            stdout,
            stderr,
            ..
        } = io;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = FrameReader::new(output);
            while let Ok(Some(message)) = async_io::block_on(reader.read()) {
                if tx.send(message).is_err() {
                    break;
                }
            }
        });
        // Keep stderr drained so a chatty adapter never blocks.
        if let Some(mut stderr) = stderr {
            std::thread::spawn(move || {
                use futures::AsyncReadExt;
                let mut buf = [0u8; 4096];
                while let Ok(n) = async_io::block_on(stderr.read(&mut buf)) {
                    if n == 0 {
                        break;
                    }
                }
            });
        }
        Self {
            writer: FrameWriter::new(input),
            rx,
            inbox: VecDeque::new(),
            process,
            seq: 0,
            log: Vec::new(),
            _stdout: stdout,
        }
    }

    fn send<R: Request>(&mut self, args: &R::Arguments) -> i64 {
        self.seq += 1;
        let message = Message::Request(RequestMessage::new::<R>(self.seq, args));
        async_io::block_on(self.writer.write(&message)).expect("write failed");
        self.log.push(format!("req:{}", R::COMMAND));
        self.seq
    }

    /// Reads one message from the adapter into the log and inbox.
    fn pump(&mut self) {
        let message = self.rx.recv_timeout(WAIT).unwrap_or_else(|_| {
            panic!(
                "adapter silent for {WAIT:?}; log: {:?}. On macOS, a native adapter hangs \
                     on launch while Developer Mode is off (`DevToolsSecurity -status`)",
                self.log
            )
        });
        match &message {
            Message::Response(r) => self.log.push(format!("resp:{}", r.command)),
            Message::Event(e) => self.log.push(format!("evt:{}", e.event)),
            Message::Request(r) => self.log.push(format!("reverse:{}", r.command)),
        }
        self.inbox.push_back(message);
    }

    fn take(&mut self, mut pred: impl FnMut(&Message) -> bool) -> Message {
        loop {
            if let Some(i) = self.inbox.iter().position(&mut pred) {
                return self.inbox.remove(i).unwrap();
            }
            self.pump();
        }
    }

    fn response(&mut self, seq: i64) -> ResponseMessage {
        let Message::Response(r) =
            self.take(|m| matches!(m, Message::Response(r) if r.request_seq == seq))
        else {
            unreachable!()
        };
        r
    }

    fn ok<R: Request>(&mut self, args: &R::Arguments) -> R::Body {
        let seq = self.send::<R>(args);
        let response = self.response(seq);
        assert!(response.success, "{} failed: {response:?}", R::COMMAND);
        response.body::<R>().unwrap()
    }

    fn event(&mut self, name: &'static str) -> EventMessage {
        let Message::Event(e) = self.take(|m| matches!(m, Message::Event(e) if e.event == name))
        else {
            unreachable!()
        };
        e
    }

    fn stopped(&mut self) -> StoppedBody {
        self.event("stopped").body::<event::Stopped>().unwrap()
    }

    fn position(&self, entry: &str, nth: usize) -> Option<usize> {
        self.log
            .iter()
            .enumerate()
            .filter(|(_, e)| *e == entry)
            .nth(nth)
            .map(|(i, _)| i)
    }

    fn finish(mut self) {
        let _ = self.send::<request::Disconnect>(&DisconnectArguments {
            terminate_debuggee: Some(true),
            ..Default::default()
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Message::Response(r)) if r.command == "disconnect" => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                _ => {}
            }
        }
        let exited = async_io::block_on(async {
            let either = futures::future::select(
                self.process.exited(),
                Box::pin(async_io::Timer::after(Duration::from_secs(10))),
            )
            .await;
            matches!(either, futures::future::Either::Left(_))
        });
        if !exited {
            self.process.kill();
        }
    }
}

struct Plan<'a> {
    adapter: &'a str,
    source: PathBuf,
    breakpoint_line: i64,
    launch: Object,
    locals_at_break: &'a [(&'a str, &'a str)],
    locals_after_step: &'a [(&'a str, &'a str)],
    expression: &'a str,
    expected: &'a str,
}

fn locals(client: &mut Client, thread: i64) -> (i64, i64, Vec<Variable>) {
    let stack = client.ok::<request::StackTrace>(&StackTraceArguments {
        thread_id: thread,
        ..Default::default()
    });
    let frame = stack.stack_frames.first().expect("no frames");
    let (frame_id, line) = (frame.id, frame.line);
    let scopes = client.ok::<request::Scopes>(&ScopesArguments {
        frame_id,
        ..Default::default()
    });
    let scope = scopes
        .scopes
        .iter()
        .find(|s| s.name.to_lowercase().contains("local"))
        .or(scopes.scopes.first())
        .expect("no scopes");
    let variables = client
        .ok::<request::Variables>(&VariablesArguments {
            variables_reference: scope.variables_reference,
            ..Default::default()
        })
        .variables;
    (frame_id, line, variables)
}

fn expect_vars(vars: &[Variable], want: &[(&str, &str)]) {
    for (name, value) in want {
        let v = vars
            .iter()
            .find(|v| v.name == *name)
            .unwrap_or_else(|| panic!("no variable {name}: {vars:?}"));
        assert_eq!(v.value.trim(), *value, "{name}");
    }
}

fn record(adapter: &str, client: &Client, step_command: &str) {
    let launch_resp = client.position("resp:launch", 0);
    let cfg_resp = client.position("resp:configurationDone", 0);
    let init_evt = client.position("evt:initialized", 0);
    let step_resp = client.position(&format!("resp:{step_command}"), 0);
    // The step's stop is the second `stopped` (the first is the breakpoint).
    let step_stop = client.position("evt:stopped", 1);
    let rel = |a: Option<usize>, b: Option<usize>, an: &str, bn: &str| match (a, b) {
        (Some(a), Some(b)) if a < b => format!("{an} before {bn}"),
        (Some(_), Some(_)) => format!("{an} after {bn}"),
        _ => format!("{an} / {bn}: not observed"),
    };
    let text = [
        format!("[{adapter}]"),
        format!(
            "  {}",
            rel(
                launch_resp,
                cfg_resp,
                "launch response",
                "configurationDone response"
            )
        ),
        format!(
            "  {}",
            rel(
                init_evt,
                launch_resp,
                "initialized event",
                "launch response"
            )
        ),
        format!(
            "  {}",
            rel(
                step_stop,
                step_resp,
                "stopped event",
                &format!("{step_command} response")
            )
        ),
        format!("  full order: {}", client.log.join(" ")),
        String::new(),
    ]
    .join("\n");
    eprintln!("observation:\n{text}");
    if let Some(path) = std::env::var_os("FE_DAP_REAL_RECORD") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open FE_DAP_REAL_RECORD");
        writeln!(file, "{text}").unwrap();
    }
}

/// breakpoint → step over → variables → evaluate, in the order every DAP
/// client does it: `initialize`, `launch` (not awaited: some adapters answer
/// it only after `configurationDone`), `setBreakpoints` on `initialized`,
/// `configurationDone`.
fn run(io: AdapterIo, plan: Plan<'_>) {
    let mut c = Client::new(io);
    c.ok::<request::Initialize>(&InitializeArguments {
        adapter_id: plan.adapter.into(),
        client_id: Some("fe-dap-real-test".into()),
        lines_start_at1: Some(true),
        columns_start_at1: Some(true),
        path_format: Some("path".into()),
        supports_variable_type: Some(true),
        ..Default::default()
    });
    let launch = c.send::<request::Launch>(&plan.launch);
    c.event("initialized");
    let set = c.ok::<request::SetBreakpoints>(&SetBreakpointsArguments {
        source: Source {
            path: Some(plan.source.display().to_string()),
            ..Default::default()
        },
        breakpoints: Some(vec![SourceBreakpoint {
            line: plan.breakpoint_line,
            ..Default::default()
        }]),
        ..Default::default()
    });
    assert_eq!(set.breakpoints.len(), 1, "{set:?}");
    c.ok::<request::ConfigurationDone>(&Empty::default());
    let launch_response = c.response(launch);
    assert!(
        launch_response.success,
        "launch failed: {launch_response:?}"
    );

    let stop = c.stopped();
    assert_eq!(stop.reason, "breakpoint", "{stop:?}");
    let thread = stop.thread_id.expect("stopped without a thread");
    let (_, line, vars) = locals(&mut c, thread);
    assert_eq!(line, plan.breakpoint_line);
    expect_vars(&vars, plan.locals_at_break);

    let step = c.send::<request::Next>(&StepArguments {
        thread_id: thread,
        ..Default::default()
    });
    let step_response = c.response(step);
    assert!(step_response.success, "{step_response:?}");
    let stop = c.stopped();
    assert_eq!(stop.reason, "step", "{stop:?}");
    let (frame, line_after, vars) = locals(&mut c, thread);
    assert_eq!(line_after, plan.breakpoint_line + 1);
    expect_vars(&vars, plan.locals_after_step);

    let result = c.ok::<request::Evaluate>(&EvaluateArguments {
        expression: plan.expression.into(),
        frame_id: Some(frame),
        context: Some("repl".into()),
        ..Default::default()
    });
    // lldb's repl formats a result as `(int) $0 = 4`: the value is what
    // follows ` = `. debugpy's is the bare value.
    let value = result.result.trim();
    let value = if plan.adapter == "debugpy" {
        value
    } else {
        value.rsplit_once(" = ").map_or(value, |(_, v)| v)
    };
    assert_eq!(value, plan.expected, "{}", plan.expression);

    record(plan.adapter, &c, "next");
    let _ = c.send::<request::Continue>(&ContinueArguments {
        thread_id: thread,
        ..Default::default()
    });
    c.finish();
}

fn spec(
    name: &str,
    command: impl Into<PathBuf>,
    args: &[&str],
    t: AdapterTransport,
) -> AdapterSpec {
    AdapterSpec {
        transport: t,
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        ..AdapterSpec::new(name, command)
    }
}

fn launch(spec: &AdapterSpec, cwd: &Path) -> AdapterIo {
    LocalLauncher::new()
        .launch(spec, cwd, &LaunchCancel::new())
        .unwrap_or_else(|err| panic!("launching {}: {err}", spec.name))
}

fn obj(value: serde_json::Value) -> Object {
    value.as_object().cloned().unwrap()
}

fn python_with_debugpy() -> Option<PathBuf> {
    let python: PathBuf = std::env::var_os("FE_DAP_PYTHON").map_or("python3".into(), Into::into);
    let ok = Command::new(&python)
        .args(["-I", "-c", "import debugpy"])
        .output()
        .is_ok_and(|o| o.status.success());
    ok.then_some(python)
}

#[test]
#[ignore = "real adapter: scripts/dap-real.sh"]
fn debugpy_breakpoint_step_variables_evaluate() {
    let Some(python) = python_with_debugpy() else {
        return skip("debugpy not importable (run scripts/dap-real.sh)");
    };
    let program = fixtures().join("hello.py");
    let io = launch(
        &spec(
            "debugpy",
            python,
            &["-I", "-m", "debugpy.adapter"],
            AdapterTransport::Stdio,
        ),
        &fixtures(),
    );
    run(
        io,
        Plan {
            adapter: "debugpy",
            source: program.clone(),
            breakpoint_line: 2,
            launch: obj(serde_json::json!({
                "type": "python",
                "request": "launch",
                "program": program,
                "console": "internalConsole",
                "justMyCode": false,
                "stopOnEntry": false,
            })),
            locals_at_break: &[("a", "1"), ("b", "2")],
            locals_after_step: &[("total", "3")],
            expression: "a + b + total",
            expected: "6",
        },
    );
}

/// `Some(reason)` where Linux's Yama `ptrace_scope` forbids launching a
/// debuggee under ptrace (2 admin-only, 3 none).
fn ptrace_forbidden() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let text = std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope").ok()?;
    let value = text.trim();
    let scope: i32 = value.parse().ok()?;
    (scope >= 2).then(|| {
        format!("kernel.yama.ptrace_scope is {value}, which forbids launching under ptrace")
    })
}

/// lldb-dap (stdio), else codelldb (TCP).
fn lldb_adapter() -> Option<AdapterSpec> {
    let lldb_dap = |p: PathBuf| spec("lldb-dap", p, &[], AdapterTransport::Stdio);
    let codelldb = |p: PathBuf| {
        spec(
            "codelldb",
            p,
            &["--port", PORT_PLACEHOLDER],
            AdapterTransport::Tcp,
        )
    };
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(p) = std::env::var_os("FE_DAP_LLDB_DAP")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        return Some(lldb_dap(p));
    }
    if let Some(p) = path_dirs
        .iter()
        .map(|d| d.join("lldb-dap"))
        .find(|p| p.is_file())
    {
        return Some(lldb_dap(p));
    }
    if let Ok(out) = Command::new("xcrun").args(["-f", "lldb-dap"]).output()
        && out.status.success()
    {
        let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        if p.is_file() {
            return Some(lldb_dap(p));
        }
    }
    if let Some(p) = std::env::var_os("FE_DAP_CODELLDB")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        return Some(codelldb(p));
    }
    path_dirs
        .iter()
        .map(|d| d.join("codelldb"))
        .find(|p| p.is_file())
        .map(codelldb)
}

#[test]
#[ignore = "real adapter: scripts/dap-real.sh"]
fn lldb_breakpoint_step_variables_evaluate() {
    if let Some(reason) = ptrace_forbidden() {
        // Not a missing adapter: the kernel forbids it, so it skips even
        // under FE_DAP_REQUIRE_ADAPTERS.
        return eprintln!("skipped: {reason}");
    }
    let Some(adapter) = lldb_adapter() else {
        return skip("neither lldb-dap nor codelldb found (set FE_DAP_LLDB_DAP)");
    };
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.rs");
    std::fs::copy(fixtures().join("rust/main.rs"), &source).unwrap();
    let binary = dir.path().join("fixture");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let status = Command::new(rustc)
        .args(["-C", "opt-level=0", "-C", "debuginfo=2", "-o"])
        .arg(&binary)
        .arg(&source)
        .status()
        .expect("running rustc");
    assert!(status.success(), "building the Rust fixture failed");

    let name = adapter.name.clone();
    let io = launch(&adapter, dir.path());
    run(
        io,
        Plan {
            adapter: &name,
            source: source.clone(),
            breakpoint_line: 3,
            launch: obj(serde_json::json!({
                "type": "lldb",
                "request": "launch",
                "program": binary,
                "cwd": dir.path(),
                "stopOnEntry": false,
            })),
            locals_at_break: &[("a", "1"), ("b", "2")],
            locals_after_step: &[("total", "3")],
            expression: "total + 1",
            expected: "4",
        },
    );
}
