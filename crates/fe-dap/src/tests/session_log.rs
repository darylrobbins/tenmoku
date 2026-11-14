// SPDX-License-Identifier: Apache-2.0

//! The protocol log (docs/decisions/r1b9dw3g0g-debugger.md §2 "Output and
//! logging", §8, D2 C8): by default metadata only, with no argument,
//! variable, evaluation or output value and no error text; with payloads
//! on, every `${env:NAME}` expansion shows as its reference wherever it
//! appears, and `env` values are hidden.

use std::time::Duration;

use kiln_l10n::tr;
use serde_json::json;

use super::session_harness::{
    Harness, Reads, TestProcess, advance_past, exited_process, frame, initialize_response, object,
    options, options_with_breakpoint, start, start_on, wait_client_response, wait_requests,
};
use crate::fake::{Fault, FaultKind, Script, THREAD_ID};
use crate::log::{
    Direction, EnvExpansion, LogMode, MAX_STDERR_LINE, ProtocolLog, REDACTED, Redactor,
};
use crate::protocol::{
    EvaluateArguments, EventMessage, Message, OutputBody, ScopesArguments, SourceArguments,
    StackTraceArguments, VariablesArguments, request,
};
use crate::session::{
    DapEvent, EXIT_DRAIN, EndReason, SessionError, SessionOptions, SessionState, TerminalStarted,
};

const TOKEN: &str = "s3cr3t-token-value";

/// Every log line, joined.
fn log_text(harness: &Harness) -> String {
    harness.log_lines().join("\n")
}

/// Answers the session's `runInTerminal` with a process id.
fn answer_terminal(harness: &mut Harness) {
    harness.wait("runInTerminal", |events| {
        events
            .iter()
            .any(|event| matches!(event, DapEvent::RunInTerminal { .. }))
    });
    let at = harness
        .events
        .iter()
        .position(|event| matches!(event, DapEvent::RunInTerminal { .. }))
        .unwrap();
    if let DapEvent::RunInTerminal { reply, .. } = harness.events.remove(at) {
        reply.started(TerminalStarted {
            process_id: Some(99),
            shell_process_id: None,
        });
    }
}

#[test]
fn the_default_log_holds_no_values_and_no_error_text() {
    let mut script = Script {
        run_in_terminal: true,
        output: vec![OutputBody {
            output: "OUTPUT_SECRET printed\n".into(),
            category: Some("stdout".into()),
            ..Default::default()
        }],
        faults: vec![Fault::new(
            "source",
            FaultKind::Fail("ERROR_SECRET: couldn't read /etc/ERROR_SECRET".into()),
        )],
        ..Script::default()
    };
    script.program.functions[0].locals[0].value = "VAR_SECRET_VALUE".into();
    let mut launch = options_with_breakpoint();
    launch.arguments = object(json!({
        "program": "/bin/ARG_SECRET_PROGRAM",
        "args": ["ARG_SECRET_ARG"],
        "env": { "TOKEN": "ENV_SECRET_VALUE" },
    }));
    let mut harness = start(script, launch);
    answer_terminal(&mut harness);
    harness.wait_state(SessionState::Stopped);
    let session = &harness.session;

    // Values of every kind: frames, variables, an evaluation, a failed
    // evaluation and a failed request with an error message.
    let frames = async_io::block_on(
        session.request::<request::StackTrace>(&StackTraceArguments {
            thread_id: THREAD_ID,
            ..Default::default()
        }),
    )
    .unwrap();
    let frame = frames.stack_frames[0].id;
    let scopes = async_io::block_on(session.request::<request::Scopes>(&ScopesArguments {
        frame_id: frame,
        ..Default::default()
    }))
    .unwrap();
    let variables =
        async_io::block_on(session.request::<request::Variables>(&VariablesArguments {
            variables_reference: scopes.scopes[0].variables_reference,
            ..Default::default()
        }))
        .unwrap();
    assert_eq!(variables.variables[0].value, "VAR_SECRET_VALUE");
    let evaluated = async_io::block_on(session.request::<request::Evaluate>(&EvaluateArguments {
        expression: "x".into(),
        frame_id: Some(frame),
        context: Some("watch".into()),
        ..Default::default()
    }))
    .unwrap();
    assert_eq!(evaluated.result, "VAR_SECRET_VALUE");
    let undefined = async_io::block_on(session.request::<request::Evaluate>(&EvaluateArguments {
        expression: "EXPR_SECRET".into(),
        frame_id: Some(frame),
        context: Some("repl".into()),
        ..Default::default()
    }));
    assert!(
        undefined
            .unwrap_err()
            .message()
            .unwrap()
            .contains("EXPR_SECRET")
    );
    let failed = async_io::block_on(session.request::<request::SourceRequest>(&SourceArguments {
        source_reference: 1,
        ..Default::default()
    }));
    assert!(failed.is_err());
    harness.session.stop();
    harness.wait_ended();
    assert!(harness.events.iter().any(
        |event| matches!(event, DapEvent::Output(body) if body.output.contains("OUTPUT_SECRET"))
    ));

    let text = log_text(&harness);
    assert!(!text.contains("SECRET"), "{text}");
    assert!(!text.contains("couldn't"), "{text}");
    assert!(!text.contains("isn't defined"), "{text}");
    // What it does hold: names, numbers and outcomes.
    harness.log_index(&["request", "command=launch", "arguments=<3 keys>"]);
    harness.log_index(&[
        "request",
        "command=runInTerminal",
        "arguments=[args, cwd, kind, title]",
    ]);
    harness.log_index(&[
        "response",
        "command=variables",
        "success=true",
        "body=[variables]",
    ]);
    harness.log_index(&["response", "command=source", "success=false", "error.id=1"]);
    harness.log_index(&["event", "event=output", "body=[category, output]"]);
    assert!(
        harness
            .session
            .log()
            .iter()
            .all(|entry| entry.direction != Direction::Note
                || !entry.text.contains(&*tr!("dap-log-payloads-warning"))),
        "the metadata log has the payload warning"
    );
}

fn failing_launch(log: LogMode) -> SessionOptions {
    let mut launch = options();
    launch.arguments = object(json!({
        "program": "/bin/app",
        "args": ["--token", TOKEN],
    }));
    launch.expansions = vec![EnvExpansion::new("API_TOKEN", TOKEN)];
    launch.log = log;
    launch
}

fn failing_script() -> Script {
    Script {
        faults: vec![Fault::new(
            "launch",
            FaultKind::Fail(format!("couldn't run /bin/app --token {TOKEN}")),
        )],
        ..Script::default()
    }
}

#[test]
fn a_failed_launch_echoing_an_expanded_token_leaves_only_the_failure_and_its_error_id() {
    let mut harness = start(failing_script(), failing_launch(LogMode::Metadata));
    let ended = harness.wait_ended();
    // The user still sees the message, on screen.
    assert_eq!(
        ended.reason,
        EndReason::Failed(SessionError::StartFailed(Some(format!(
            "couldn't run /bin/app --token {TOKEN}"
        ))))
    );
    let text = log_text(&harness);
    assert!(!text.contains(TOKEN), "{text}");
    assert!(!text.contains("couldn't"), "{text}");
    assert!(!text.contains("/bin/app"), "{text}");
    let lines = harness.log_lines();
    let response = &lines[harness.log_index(&["response", "command=launch"])];
    let failure = response
        .strip_prefix("response seq=")
        .and_then(|rest| rest.split_once(" command=launch "))
        .map(|(_, rest)| rest)
        .unwrap();
    // Only the outcome, the error id and the timing.
    let fields: Vec<&str> = failure.split(' ').collect();
    assert_eq!(fields.len(), 3, "{response}");
    assert_eq!(fields[..2], ["success=false", "error.id=1"], "{response}");
    assert!(
        fields[2].starts_with("took=") && fields[2].ends_with("ms"),
        "{response}"
    );
}

#[test]
fn with_payloads_the_token_shows_as_its_reference() {
    // The failed launch: in the request, and in the adapter's error.
    let mut harness = start(failing_script(), failing_launch(LogMode::Payloads));
    harness.wait_ended();
    let text = log_text(&harness);
    assert!(!text.contains(TOKEN), "{text}");
    let lines = harness.log_lines();
    assert_eq!(lines[0], tr!("dap-log-payloads-warning").to_string());
    let request = &lines[harness.log_index(&["request", "command=launch"])];
    assert!(
        request.contains(r#""args":["--token","${env:API_TOKEN}"]"#),
        "{request}"
    );
    let response = &lines[harness.log_index(&["response", "command=launch"])];
    assert!(
        response.contains(r#""message":"couldn't run /bin/app --token ${env:API_TOKEN}""#),
        "{response}"
    );
    assert!(
        response.contains(r#""format":"couldn't run /bin/app --token ${env:API_TOKEN}""#),
        "{response}"
    );

    // A session that runs: the adapter echoes the token in its
    // `runInTerminal` argv and in its output, and `env` values are hidden.
    let script = Script {
        run_in_terminal: true,
        output: vec![OutputBody {
            output: format!("connecting with {TOKEN}\n"),
            category: Some("console".into()),
            ..Default::default()
        }],
        ..Script::default()
    };
    let mut launch = failing_launch(LogMode::Payloads);
    launch.arguments = object(json!({
        "program": "/bin/app",
        "args": ["--token", TOKEN],
        "env": { "API_TOKEN": TOKEN, "HOME": "/home/someone" },
    }));
    let mut harness = start(script, launch);
    answer_terminal(&mut harness);
    let fake = harness.fake(0);
    wait_client_response(&fake, "runInTerminal");
    harness.wait_ended();
    let text = log_text(&harness);
    assert!(!text.contains(TOKEN), "{text}");
    assert!(!text.contains("/home/someone"), "{text}");
    let lines = harness.log_lines();
    let request = &lines[harness.log_index(&["request", "command=launch"])];
    assert!(
        request.contains(&format!(
            r#""env":{{"API_TOKEN":"{REDACTED}","HOME":"{REDACTED}"}}"#
        )),
        "{request}"
    );
    let echo = &lines[harness.log_index(&["request", "command=runInTerminal"])];
    assert!(
        echo.contains(r#""args":["/fake/src/main.rs","--token","${env:API_TOKEN}"]"#),
        "{echo}"
    );
    let output = &lines[harness.log_index(&["event=output"])];
    assert!(
        output.contains(r#""output":"connecting with ${env:API_TOKEN}\n""#),
        "{output}"
    );
    // The program's output itself still reaches the host as it came.
    assert!(
        harness
            .events
            .iter()
            .any(|event| matches!(event, DapEvent::Output(body) if body.output.contains(TOKEN)))
    );
    wait_requests(&fake, "disconnect", 1);
}

#[test]
fn redaction_puts_each_reference_back_once() {
    // A value inside another expansion's name, and one value inside
    // another: one pass, longest first.
    let redactor = Redactor::new(&[
        EnvExpansion::new("API_TOKEN", "API"),
        EnvExpansion::new("LONG", "API-KEY"),
        EnvExpansion::new("EMPTY", ""),
    ]);
    assert_eq!(
        redactor.text("use API and API-KEY"),
        "use ${env:API_TOKEN} and ${env:LONG}"
    );
    let mut value = json!({
        "env": { "A": "x", "B": null },
        "nested": [{ "env": { "C": "API" } }, "API"],
        "API": 1,
    });
    redactor.value(&mut value);
    assert_eq!(
        value,
        json!({
            "env": { "A": REDACTED, "B": null },
            "nested": [{ "env": { "C": REDACTED } }, "${env:API_TOKEN}"],
            "${env:API_TOKEN}": 1,
        })
    );
}

/// Every application log record, as text, for the whole test process.
static RECORDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        RECORDS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(record.args().to_string());
    }

    fn flush(&self) {}
}

static CAPTURE: Capture = Capture;

fn capture_logs() {
    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Trace);
}

fn records() -> Vec<String> {
    RECORDS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[test]
fn adapter_text_never_reaches_the_application_log() {
    capture_logs();
    let expansions = vec![EnvExpansion::new("API_TOKEN", TOKEN)];

    // A failed request whose message echoes the token.
    let script = Script {
        faults: vec![Fault::new(
            "threads",
            FaultKind::Fail(format!("no threads for {TOKEN}")),
        )],
        ..Script::default()
    };
    let mut launch = options();
    launch.expansions = expansions.clone();
    let mut harness = start(script, launch);
    wait_requests(&harness.fake(0), "threads", 1);
    harness.wait_ended();

    // Malformed messages whose parse errors would quote (escaped) a value
    // with a quote, a backslash or a line break in it.
    for secret in [r#"pa"ss"#, r"back\slash", "new\nline"] {
        let expansions = vec![EnvExpansion::new("SECRET", secret)];
        let quoted = serde_json::to_string(secret).unwrap();
        let malformed = format!(r#"{{"seq":{quoted},"type":"event","event":"output"}}"#);
        let mut output = frame(&initialize_response(json!({})));
        output.extend_from_slice(format!("Content-Length: {}\r\n\r\n", malformed.len()).as_bytes());
        output.extend_from_slice(malformed.as_bytes());
        let (_sent, input) = crate::fake::pipe();
        let (process, _) = TestProcess::new();
        let io = crate::launcher::AdapterIo {
            input: Box::pin(input),
            output: Box::pin(futures::io::Cursor::new(output)),
            stderr: None,
            stdout: None,
            port: None,
            process,
        };
        let mut launch = options();
        launch.expansions = expansions;
        launch.log = LogMode::Payloads;
        let mut harness = start_on(io, launch);
        let ended = harness.wait_ended();
        // A named error, holding nothing the adapter sent.
        assert_eq!(
            ended.reason,
            EndReason::Failed(SessionError::Transport(
                tr!("dap-session-bad-message").to_string()
            ))
        );
        let text = log_text(&harness);
        let escaped = format!("{secret:?}");
        for form in [secret, quoted.as_str(), escaped.as_str()] {
            assert!(!text.contains(form), "{form} in {text}");
        }
        for record in records() {
            for form in [secret, quoted.as_str(), escaped.as_str()] {
                assert!(
                    !record.contains(form),
                    "{form} in the application log: {record}"
                );
            }
        }
    }

    let logged = records();
    assert!(!logged.is_empty());
    for record in &logged {
        assert!(!record.contains(TOKEN), "in the application log: {record}");
    }
}

fn stderr_log(expansions: &[EnvExpansion], chunks: &[&[u8]]) -> Vec<String> {
    let mut log = ProtocolLog::new(LogMode::Payloads, expansions);
    for chunk in chunks {
        log.stderr(Duration::ZERO, chunk);
    }
    log.stderr_end(Duration::ZERO);
    log.entries()
        .into_iter()
        .filter(|entry| entry.direction == Direction::Stderr)
        .map(|entry| entry.text)
        .collect()
}

/// `bytes` in pieces of `size`.
fn pieces(bytes: &[u8], size: usize) -> Vec<&[u8]> {
    bytes.chunks(size).collect()
}

#[test]
fn stderr_is_redacted_as_a_stream_before_it_is_split() {
    let token = [EnvExpansion::new("API_TOKEN", TOKEN)];

    // Across the line-length cut, fed a few bytes at a time.
    let mut long = vec![b'a'; MAX_STDERR_LINE - 6];
    long.extend_from_slice(TOKEN.as_bytes());
    long.push(b'\n');
    let lines = stderr_log(&token, &pieces(&long, 7));
    assert!(
        lines.iter().all(|line| !line.contains("s3cr3t")),
        "{lines:?}"
    );
    assert_eq!(
        lines.concat(),
        format!("{}${{env:API_TOKEN}}", "a".repeat(MAX_STDERR_LINE - 6))
    );

    // A value with a line break in it, a byte at a time.
    let multi = [EnvExpansion::new("MULTI", "line-one\nline-two")];
    let text = b"before line-one\nline-two after\n";
    assert_eq!(
        stderr_log(&multi, &pieces(text, 1)),
        ["before ${env:MULTI} after"]
    );

    // A value ending in whitespace.
    let spaced = [EnvExpansion::new("PASSWORD", "pass word  ")];
    assert_eq!(
        stderr_log(&spaced, &[b"x pass word  \r\n"]),
        ["x ${env:PASSWORD}"]
    );

    // The last fragment, with no line break after it.
    let tail = format!("tail {TOKEN}");
    assert_eq!(
        stderr_log(&token, &pieces(tail.as_bytes(), 3)),
        ["tail ${env:API_TOKEN}"]
    );

    // Only with payloads on.
    let mut log = ProtocolLog::new(LogMode::Metadata, &token);
    log.stderr(Duration::ZERO, b"anything\n");
    log.stderr_end(Duration::ZERO);
    assert!(log.entries().is_empty());
}

#[test]
fn the_default_log_counts_keys_the_protocol_does_not_define() {
    let mut harness = start(Script::default(), options_with_breakpoint());
    harness.wait_state(SessionState::Stopped);
    let fake = harness.fake(0);
    // An adapter's own event, and a protocol event with an extra field,
    // both keyed by secrets.
    fake.send(Message::Event(EventMessage {
        event: "customProgress".into(),
        body: Some(json!({ "SECRET_DYNAMIC_KEY": 1 })),
        ..Default::default()
    }));
    fake.send(Message::Event(EventMessage {
        event: "output".into(),
        body: Some(json!({
            "category": "console",
            "output": "hello",
            "SECRET_EXTRA_KEY": true,
        })),
        ..Default::default()
    }));
    // A command the protocol doesn't define here, with dynamic arguments.
    let custom = harness
        .session
        .raw_request::<serde_json::Value>("customCommand", json!({ "SECRET_ARGUMENT_KEY": 1 }));
    assert!(async_io::block_on(custom).is_err());
    harness.session.stop();
    harness.wait_ended();
    let text = log_text(&harness);
    assert!(!text.contains("SECRET"), "{text}");
    harness.log_index(&["event=customProgress", "body=<1 keys>"]);
    harness.log_index(&["event=output", "body=[category, output, +1]"]);
    harness.log_index(&["command=customCommand", "arguments=<1 keys>"]);
}

fn stderr_entries(harness: &Harness) -> Vec<String> {
    harness
        .session
        .log()
        .into_iter()
        .filter(|entry| entry.direction == Direction::Stderr)
        .map(|entry| entry.text)
        .collect()
}

/// A session in payload mode on an adapter that has already exited, whose
/// stderr comes in `chunks` (and then ends, unless `open`).
fn exited_with_stderr(chunks: Vec<Vec<u8>>, open: bool) -> Harness {
    let (_output, output_writer) = crate::fake::pipe();
    drop(output_writer);
    let (_sent, input) = crate::fake::pipe();
    let io = crate::launcher::AdapterIo {
        input: Box::pin(input),
        output: Box::pin(_output),
        stderr: Some(Box::pin(Reads::new(chunks, open))),
        stdout: None,
        port: None,
        process: exited_process(),
    };
    let mut launch = options();
    launch.expansions = vec![EnvExpansion::new("API_TOKEN", TOKEN)];
    launch.log = LogMode::Payloads;
    start_on(io, launch)
}

#[test]
fn stderr_left_at_the_exit_is_read_through_the_redactor() {
    // A secret split across two reads, the second after the exit is seen.
    let (start, rest) = TOKEN.split_at(6);
    let mut harness = exited_with_stderr(
        vec![
            format!("boom {start}").into_bytes(),
            format!("{rest}\n").into_bytes(),
        ],
        false,
    );
    harness.wait_ended();
    assert_eq!(stderr_entries(&harness), ["boom ${env:API_TOKEN}"]);

    // A stream that stays open with a secret's start held: given up after
    // the drain, the start hidden.
    let mut harness = exited_with_stderr(vec![format!("boom {start}").into_bytes()], true);
    advance_past(&harness.clock, EXIT_DRAIN);
    harness.wait_ended();
    assert_eq!(stderr_entries(&harness), [format!("boom {REDACTED}")]);
    assert!(!log_text(&harness).contains(start));
}
