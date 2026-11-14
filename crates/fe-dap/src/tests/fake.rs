// SPDX-License-Identifier: Apache-2.0

//! The scripted fake adapter (§2): it answers a startup over an in-memory
//! pipe, and each of §2's knobs changes what it sends. Each knob's test is
//! named after it, and compares the knob's messages with the default's.

use std::pin::{Pin, pin};
use std::time::Duration;

use futures::future::Either;
use futures::{AsyncRead, AsyncWrite};
use serde_json::json;

use crate::fake::{
    FakeAdapter, Fault, FaultKind, Line, Program, Script, Startup, THREAD_ID, Trigger, connect,
};
use crate::launcher::{AdapterProcess, ExitInfo};
use crate::protocol::{
    CancelArguments, ContinueArguments, DisconnectArguments, Empty, EvaluateArguments,
    InitializeArguments, Message, Object, PauseArguments, Request, RequestMessage, ResponseMessage,
    RunInTerminalBody, ScopesArguments, SetBreakpointsArguments, SetExceptionBreakpointsArguments,
    Source, SourceBreakpoint, StackTraceArguments, StepArguments, TerminateArguments,
    VariablesArguments, request, reverse,
};
use crate::transport::{FrameReader, FrameWriter};

/// A bare client: it writes requests, and reads the fake's messages when
/// a test wants them through the pipe (the fake's own record serves the
/// rest).
struct Client {
    writer: FrameWriter<Pin<Box<dyn AsyncWrite + Send>>>,
    reader: FrameReader<Pin<Box<dyn AsyncRead + Send>>>,
    process: Box<dyn AdapterProcess>,
    seq: i64,
    fake: FakeAdapter,
}

fn client(script: Script) -> Client {
    let (io, fake) = connect(script);
    Client {
        writer: FrameWriter::new(io.input),
        reader: FrameReader::new(io.output),
        process: io.process,
        seq: 0,
        fake,
    }
}

impl Client {
    fn send<R: Request>(&mut self, arguments: &R::Arguments) -> i64 {
        self.seq += 1;
        let message = Message::Request(RequestMessage::new::<R>(self.seq, arguments));
        async_io::block_on(self.writer.write(&message)).unwrap();
        self.seq
    }

    fn reply(&mut self, response: ResponseMessage) {
        self.seq += 1;
        let response = ResponseMessage {
            seq: self.seq,
            ..response
        };
        async_io::block_on(self.writer.write(&Message::Response(response))).unwrap();
    }

    /// The next message through the pipe; `None` at its end. Panics after
    /// 10 s.
    fn read(&mut self) -> Option<Message> {
        async_io::block_on(async {
            let timeout = async_io::Timer::after(Duration::from_secs(10));
            match futures::future::select(pin!(self.reader.read()), timeout).await {
                Either::Left((read, _)) => read.unwrap(),
                Either::Right(_) => panic!("nothing came through the pipe in 10 s"),
            }
        })
    }

    /// Reads through the pipe up to and including `label`'s message.
    fn read_until(&mut self, wanted: &str) -> Vec<String> {
        let mut seen = Vec::new();
        loop {
            let message = self
                .read()
                .unwrap_or_else(|| panic!("the pipe ended before {wanted}: {seen:?}"));
            seen.push(label(&message));
            if seen.last().unwrap() == wanted {
                return seen;
            }
        }
    }

    /// Waits until the fake has sent `wanted`, by label.
    fn wait_sent(&self, wanted: &str) {
        self.fake.wait_until(wanted, |_, sent| {
            sent.iter().any(|message| label(message) == wanted)
        });
    }

    /// Waits until the fake has sent `count` messages labelled `wanted`.
    fn wait_sent_times(&self, wanted: &str, count: usize) {
        self.fake.wait_until(wanted, |_, sent| {
            sent.iter()
                .filter(|message| label(message) == wanted)
                .count()
                >= count
        });
    }

    /// Waits for the fake's response to request `seq`.
    fn response(&self, seq: i64) -> ResponseMessage {
        let found = |sent: &[Message]| {
            sent.iter().find_map(|message| match message {
                Message::Response(response) if response.request_seq == seq => {
                    Some(response.clone())
                }
                _ => None,
            })
        };
        self.fake
            .wait_until("a response", |_, sent| found(sent).is_some());
        found(&self.fake.sent()).unwrap()
    }

    /// Whether the fake has answered request `seq`.
    fn answered(&self, seq: i64) -> bool {
        self.fake.sent().iter().any(
            |message| matches!(message, Message::Response(response) if response.request_seq == seq),
        )
    }

    /// The labels of what the fake has sent.
    fn sent(&self) -> Vec<String> {
        self.fake.sent().iter().map(label).collect()
    }

    /// The labels of what it sent after its first `skip` messages.
    fn sent_after(&self, skip: usize) -> Vec<String> {
        self.sent().into_iter().skip(skip).collect()
    }

    /// A request taking no arguments the fake needs, as a marker: once it
    /// is answered, everything sent before it has been too.
    fn sync(&mut self) {
        let seq = self.send::<request::Threads>(&Empty::default());
        self.response(seq);
    }

    /// Sends the startup up to (but not including) `configurationDone`;
    /// returns `setBreakpoints`' seq.
    fn begin(&mut self, breakpoint_lines: &[i64]) -> i64 {
        self.send::<request::Initialize>(&InitializeArguments {
            adapter_id: "fake".into(),
            lines_start_at1: Some(true),
            columns_start_at1: Some(true),
            ..Default::default()
        });
        self.send::<request::Launch>(&Object::new());
        let set = self.send::<request::SetBreakpoints>(&SetBreakpointsArguments {
            source: Source {
                path: Some(Program::default().path),
                ..Default::default()
            },
            breakpoints: Some(
                breakpoint_lines
                    .iter()
                    .map(|&line| SourceBreakpoint {
                        line,
                        ..Default::default()
                    })
                    .collect(),
            ),
            ..Default::default()
        });
        self.send::<request::SetExceptionBreakpoints>(&SetExceptionBreakpointsArguments {
            filters: vec!["uncaught".into()],
            ..Default::default()
        });
        set
    }

    /// The whole startup, with breakpoints on `breakpoint_lines`.
    fn start(&mut self, breakpoint_lines: &[i64]) {
        self.begin(breakpoint_lines);
        self.send::<request::ConfigurationDone>(&Empty::default());
    }

    fn next(&mut self) -> i64 {
        self.send::<request::Next>(&StepArguments {
            thread_id: THREAD_ID,
            ..Default::default()
        })
    }

    fn continue_(&mut self) -> i64 {
        self.send::<request::Continue>(&ContinueArguments {
            thread_id: THREAD_ID,
            ..Default::default()
        })
    }

    /// The top frame's line, by `stackTrace`.
    fn top_line(&mut self) -> i64 {
        let seq = self.send::<request::StackTrace>(&StackTraceArguments {
            thread_id: THREAD_ID,
            ..Default::default()
        });
        let response = self.response(seq);
        assert!(response.success, "{response:?}");
        response.body::<request::StackTrace>().unwrap().stack_frames[0].line
    }
}

/// A message, briefly: `response:next`, `response:next!` (failed),
/// `event:stopped(step)`, `request:runInTerminal`.
fn label(message: &Message) -> String {
    match message {
        Message::Request(request) => format!("request:{}", request.command),
        Message::Response(response) => format!(
            "response:{}{}",
            response.command,
            if response.success { "" } else { "!" }
        ),
        Message::Event(event) => {
            let reason = event
                .body
                .as_ref()
                .and_then(|body| body.get("reason"))
                .and_then(|reason| reason.as_str());
            match reason {
                Some(reason) => format!("event:{}({reason})", event.event),
                None => format!("event:{}", event.event),
            }
        }
    }
}

fn line(text: &str) -> i64 {
    Program::default().line_of(text)
}

/// Line 2, where most knob tests stop first.
fn point() -> i64 {
    line("let point")
}

#[test]
fn the_fake_answers_initialize_and_launch_over_a_pipe() {
    let mut client = client(Script::default());
    let initialize = client.send::<request::Initialize>(&InitializeArguments {
        adapter_id: "fake".into(),
        ..Default::default()
    });
    let Some(Message::Response(response)) = client.read() else {
        panic!("no initialize response");
    };
    assert_eq!(response.request_seq, initialize);
    assert!(response.success);
    let capabilities = response.body::<request::Initialize>().unwrap();
    assert_eq!(capabilities.supports_configuration_done_request, Some(true));
    assert_eq!(
        client.read_until("event:initialized"),
        vec!["event:initialized"]
    );

    let launch = client.send::<request::Launch>(&Object::from_iter([(
        "program".to_owned(),
        json!("/fake/target/debug/main"),
    )]));
    client.send::<request::SetBreakpoints>(&SetBreakpointsArguments {
        source: Source {
            path: Some(Program::default().path),
            ..Default::default()
        },
        breakpoints: Some(vec![SourceBreakpoint {
            line: point(),
            ..Default::default()
        }]),
        ..Default::default()
    });
    client.send::<request::SetExceptionBreakpoints>(&SetExceptionBreakpointsArguments::default());
    client.send::<request::ConfigurationDone>(&Empty::default());
    assert_eq!(
        client.read_until("event:stopped(breakpoint)"),
        vec![
            "response:setBreakpoints",
            "response:setExceptionBreakpoints",
            "response:configurationDone",
            "response:launch",
            "event:thread(started)",
            "event:stopped(breakpoint)",
        ]
    );
    assert!(client.answered(launch));
    // The launch arguments reached the fake as sent.
    let received = client.fake.received();
    let Some(Message::Request(sent_launch)) =
        received.iter().find(|m| label(m) == "request:launch")
    else {
        panic!("the fake never saw launch");
    };
    assert_eq!(
        sent_launch.arguments::<request::Launch>().unwrap()["program"],
        "/fake/target/debug/main"
    );

    // The stop's data, down to a child variable.
    let trace = client.send::<request::StackTrace>(&StackTraceArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    let Some(Message::Response(response)) = client.read() else {
        panic!("no stackTrace response");
    };
    assert_eq!(response.request_seq, trace);
    let frames = response.body::<request::StackTrace>().unwrap().stack_frames;
    assert_eq!(frames.len(), 1);
    assert_eq!((frames[0].name.as_str(), frames[0].line), ("main", point()));
    client.send::<request::Scopes>(&ScopesArguments {
        frame_id: frames[0].id,
        ..Default::default()
    });
    let Some(Message::Response(response)) = client.read() else {
        panic!("no scopes response");
    };
    let scopes = response.body::<request::Scopes>().unwrap().scopes;
    assert_eq!(
        scopes
            .iter()
            .map(|s| (s.name.as_str(), s.expensive))
            .collect::<Vec<_>>(),
        [("Locals", false), ("Globals", true)]
    );
    client.send::<request::Variables>(&VariablesArguments {
        variables_reference: scopes[0].variables_reference,
        ..Default::default()
    });
    let Some(Message::Response(response)) = client.read() else {
        panic!("no variables response");
    };
    let variables = response.body::<request::Variables>().unwrap().variables;
    assert_eq!(
        variables
            .iter()
            .map(|v| (v.name.as_str(), v.value.as_str()))
            .collect::<Vec<_>>(),
        [("x", "1"), ("point", "Point { x: 1, y: 2 }")]
    );
    assert_ne!(variables[1].variables_reference, 0);
    client.send::<request::Variables>(&VariablesArguments {
        variables_reference: variables[1].variables_reference,
        ..Default::default()
    });
    let Some(Message::Response(response)) = client.read() else {
        panic!("no variables response");
    };
    let children = response.body::<request::Variables>().unwrap().variables;
    assert_eq!(
        children.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
        ["x", "y"]
    );

    // Running to the end: the program's end, then a clean disconnect.
    client.continue_();
    assert_eq!(
        client.read_until("event:terminated"),
        vec![
            "response:continue",
            "event:continued",
            "event:thread(exited)",
            "event:exited",
            "event:terminated",
        ]
    );
    client.send::<request::Disconnect>(&DisconnectArguments::default());
    assert_eq!(
        client.read_until("response:disconnect"),
        vec!["response:disconnect"]
    );
    assert!(client.read().is_none(), "the fake's output didn't end");
    let exit = async_io::block_on(client.process.exited());
    assert_eq!(
        exit,
        ExitInfo {
            code: Some(0),
            signal: None
        }
    );
}

/// The startup, and what the fake sent for it, under `script`.
fn startup(script: Script, configuration_done: bool) -> Vec<String> {
    let mut client = client(script);
    if configuration_done {
        client.start(&[point()]);
    } else {
        client.begin(&[point()]);
    }
    client.wait_sent("event:stopped(breakpoint)");
    client.sent()
}

#[test]
fn startup_orderings_with_configuration_done() {
    assert_eq!(
        startup(Script::default(), true),
        [
            "response:initialize",
            "event:initialized",
            "response:setBreakpoints",
            "response:setExceptionBreakpoints",
            "response:configurationDone",
            "response:launch",
            "event:thread(started)",
            "event:stopped(breakpoint)",
        ]
    );
    let launch_first = Script {
        startup: Startup::LaunchResponseFirst,
        ..Script::default()
    };
    assert_eq!(
        startup(launch_first, true),
        [
            "response:initialize",
            "response:launch",
            "event:initialized",
            "response:setBreakpoints",
            "response:setExceptionBreakpoints",
            "response:configurationDone",
            "event:thread(started)",
            "event:stopped(breakpoint)",
        ]
    );
}

#[test]
fn startup_orderings_without_configuration_done() {
    let script = Script::default().without_configuration_done();
    assert_eq!(
        script.capabilities.supports_configuration_done_request,
        None
    );
    // Configuration ends with `setExceptionBreakpoints`, its last request.
    assert_eq!(
        startup(script.clone(), false),
        [
            "response:initialize",
            "event:initialized",
            "response:setBreakpoints",
            "response:setExceptionBreakpoints",
            "response:launch",
            "event:thread(started)",
            "event:stopped(breakpoint)",
        ]
    );
    let launch_first = Script {
        startup: Startup::LaunchResponseFirst,
        ..script
    };
    assert_eq!(
        startup(launch_first, false),
        [
            "response:initialize",
            "response:launch",
            "event:initialized",
            "response:setBreakpoints",
            "response:setExceptionBreakpoints",
            "event:thread(started)",
            "event:stopped(breakpoint)",
        ]
    );
}

#[test]
fn startup_initialized_on_release_holds_initialized_for_the_test() {
    let mut client = client(Script {
        startup: Startup::InitializedOnRelease,
        ..Script::default()
    });
    client.send::<request::Initialize>(&InitializeArguments {
        adapter_id: "fake".into(),
        ..Default::default()
    });
    client.send::<request::Launch>(&Object::new());
    client.wait_sent("response:launch");
    assert_eq!(client.sent(), ["response:initialize", "response:launch"]);
    client.fake.release();
    client.wait_sent("event:initialized");
    assert_eq!(
        client.sent(),
        [
            "response:initialize",
            "response:launch",
            "event:initialized"
        ]
    );
}

/// What a `next` from the first stop sends, under `script`.
fn step(script: Script) -> Vec<String> {
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let before = client.sent().len();
    client.next();
    client.wait_sent("event:stopped(step)");
    client.wait_sent("response:next");
    assert_eq!(client.top_line(), point() + 1);
    client
        .sent_after(before)
        .into_iter()
        .filter(|label| label != "response:stackTrace")
        .collect()
}

#[test]
fn resumes_answered_with_no_continued_event() {
    assert_eq!(
        step(Script::default()),
        ["response:next", "event:continued", "event:stopped(step)"]
    );
    let script = Script {
        continued_events: false,
        ..Script::default()
    };
    assert_eq!(step(script), ["response:next", "event:stopped(step)"]);
}

#[test]
fn a_stopped_event_sent_before_a_step_response() {
    let script = Script {
        stopped_before_step_response: true,
        ..Script::default()
    };
    assert_eq!(step(script), ["event:stopped(step)", "response:next"]);
}

#[test]
fn a_resume_that_fails() {
    let script = Script {
        faults: vec![Fault::new(
            "next",
            FaultKind::Fail("can't step here".into()),
        )],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let before = client.sent().len();
    let next = client.next();
    let response = client.response(next);
    assert!(!response.success);
    assert_eq!(response.message.as_deref(), Some("can't step here"));
    assert_eq!(response.error().unwrap().format, "can't step here");
    // Nothing ran: still stopped where it was, and no events.
    assert_eq!(client.top_line(), point());
    assert_eq!(
        client.sent_after(before),
        ["response:next!", "response:stackTrace"]
    );
    // Only the first `next` fails.
    client.next();
    client.wait_sent("event:stopped(step)");
    assert_eq!(client.top_line(), point() + 1);
}

#[test]
fn a_resume_that_is_never_answered() {
    let script = Script {
        faults: vec![Fault::new("next", FaultKind::Unanswered)],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let before = client.sent().len();
    let next = client.next();
    client.sync();
    assert!(!client.answered(next));
    assert_eq!(client.sent_after(before), ["response:threads"]);
    // Still stopped where it was.
    assert_eq!(client.top_line(), point());
}

#[test]
fn a_resume_failure_delayed_until_after_another_resume() {
    let script = Script {
        faults: vec![Fault::new(
            "next",
            FaultKind::FailAfter {
                trigger: Trigger::NextResume,
                message: "cancelled".into(),
            },
        )],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point(), line("println")]);
    client.wait_sent("event:stopped(breakpoint)");
    let before = client.sent().len();
    let next = client.next();
    client.sync();
    assert!(!client.answered(next), "the failure came early");
    client.continue_();
    let failure = client.response(next);
    assert!(!failure.success);
    assert_eq!(failure.message.as_deref(), Some("cancelled"));
    assert_eq!(
        client.sent_after(before),
        [
            "response:threads",
            "response:continue",
            "event:continued",
            "event:stopped(breakpoint)",
            "response:next!",
        ]
    );
}

#[test]
fn a_resume_failure_delayed_until_after_a_terminate() {
    let script = Script {
        faults: vec![Fault::new(
            "next",
            FaultKind::FailAfter {
                trigger: Trigger::Terminate,
                message: "cancelled".into(),
            },
        )],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let before = client.sent().len();
    let next = client.next();
    // Another resume doesn't let this one go.
    client.continue_();
    client.wait_sent("event:terminated");
    assert!(
        !client.answered(next),
        "the failure came before the terminate"
    );
    client.send::<request::Terminate>(&TerminateArguments::default());
    assert!(!client.response(next).success);
    assert_eq!(
        client.sent_after(before),
        [
            "response:continue",
            "event:continued",
            "event:thread(exited)",
            "event:exited",
            "event:terminated",
            "response:terminate",
            "event:terminated",
            "response:next!",
        ]
    );
}

#[test]
fn a_variables_reply_delayed_past_a_step() {
    let script = Script {
        faults: vec![Fault::new(
            "variables",
            FaultKind::ReplyAfter(Trigger::NextResume),
        )],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let trace = client.send::<request::StackTrace>(&StackTraceArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    let frame = client
        .response(trace)
        .body::<request::StackTrace>()
        .unwrap()
        .stack_frames[0]
        .id;
    let scopes = client.send::<request::Scopes>(&ScopesArguments {
        frame_id: frame,
        ..Default::default()
    });
    let locals = client
        .response(scopes)
        .body::<request::Scopes>()
        .unwrap()
        .scopes[0]
        .variables_reference;
    let before = client.sent().len();
    let variables = client.send::<request::Variables>(&VariablesArguments {
        variables_reference: locals,
        ..Default::default()
    });
    client.sync();
    assert!(
        !client.answered(variables),
        "the reply came before the step"
    );
    client.next();
    let reply = client.response(variables);
    assert_eq!(
        client.sent_after(before),
        [
            "response:threads",
            "response:next",
            "event:continued",
            "event:stopped(step)",
            "response:variables",
        ]
    );
    // It is the old stop's reply, which the reference now no longer
    // names.
    assert!(reply.success);
    assert_eq!(
        reply.body::<request::Variables>().unwrap().variables[0].name,
        "x"
    );
    let again = client.send::<request::Variables>(&VariablesArguments {
        variables_reference: locals,
        ..Default::default()
    });
    assert!(!client.response(again).success);
}

/// A program that spins at its second line until paused.
fn spinning() -> Script {
    let mut program = Program::default();
    program.lines[1] = Line {
        spin: true,
        ..program.lines[1].clone()
    };
    Script {
        program,
        ..Script::default()
    }
}

#[test]
fn a_program_that_runs_with_no_thread_events() {
    let run = |script: Script| {
        let mut client = client(script);
        client.start(&[]);
        client.wait_sent("response:launch");
        client.sync();
        let threads = client.send::<request::Threads>(&Empty::default());
        let threads = client
            .response(threads)
            .body::<request::Threads>()
            .unwrap()
            .threads;
        // Paused where it spins.
        client.send::<request::Pause>(&PauseArguments {
            thread_id: threads[0].id,
            ..Default::default()
        });
        client.wait_sent("event:stopped(pause)");
        (client.sent(), threads)
    };
    let (sent, threads) = run(spinning());
    assert!(
        sent.contains(&"event:thread(started)".to_owned()),
        "{sent:?}"
    );
    assert_eq!(threads[0].id, THREAD_ID);
    let quiet = Script {
        thread_events: false,
        ..spinning()
    };
    let (sent, threads) = run(quiet);
    assert!(
        !sent.iter().any(|label| label.starts_with("event:thread")),
        "{sent:?}"
    );
    // `threads` still names it: the baseline is all a client has.
    assert_eq!(threads[0].id, THREAD_ID);
}

#[test]
fn unverified_then_verified_breakpoints() {
    let run = |script: Script| {
        let mut client = client(script);
        let set = client.begin(&[point()]);
        let breakpoints = client
            .response(set)
            .body::<request::SetBreakpoints>()
            .unwrap()
            .breakpoints;
        client.send::<request::ConfigurationDone>(&Empty::default());
        client.wait_sent("event:stopped(breakpoint)");
        (client.fake.sent(), breakpoints)
    };
    let (sent, breakpoints) = run(Script::default());
    assert!(breakpoints[0].verified);
    assert!(
        !sent
            .iter()
            .any(|message| label(message) == "event:breakpoint(changed)")
    );

    let later = Script {
        verify_breakpoints_later: true,
        ..Script::default()
    };
    let (sent, breakpoints) = run(later);
    assert!(!breakpoints[0].verified);
    assert_eq!(breakpoints[0].reason.as_deref(), Some("pending"));
    let changed = sent
        .iter()
        .find_map(|message| match message {
            Message::Event(event) if event.event == "breakpoint" => Some(
                event
                    .body::<crate::protocol::event::BreakpointEvent>()
                    .unwrap(),
            ),
            _ => None,
        })
        .expect("no breakpoint event");
    assert_eq!(changed.reason, "changed");
    assert!(changed.breakpoint.verified);
    assert_eq!(changed.breakpoint.id, breakpoints[0].id);
    assert_eq!(changed.breakpoint.line, Some(point()));
}

#[test]
fn the_adapter_exiting_mid_request() {
    let script = Script {
        faults: vec![Fault::new("stackTrace", FaultKind::Exit(3))],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.read_until("event:stopped(breakpoint)");
    let before = client.sent().len();
    let trace = client.send::<request::StackTrace>(&StackTraceArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    // The pipe ends with no answer, and the "process" exits with the code.
    assert!(client.read().is_none());
    assert!(!client.answered(trace));
    assert_eq!(client.sent().len(), before);
    let exit = async_io::block_on(client.process.exited());
    assert_eq!(
        exit,
        ExitInfo {
            code: Some(3),
            signal: None
        }
    );
    // Without the fault, the same request is answered.
    let mut client = self::client(Script::default());
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    assert_eq!(client.top_line(), point());
}

// Beyond §2's list: what the session card (D2) also drives.

#[test]
fn a_spinning_program_runs_until_paused() {
    let mut client = client(spinning());
    client.start(&[]);
    client.wait_sent("event:thread(started)");
    client.sync();
    assert!(
        !client
            .sent()
            .iter()
            .any(|label| label.starts_with("event:stopped"))
    );
    let trace = client.send::<request::StackTrace>(&StackTraceArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    assert_eq!(
        client.response(trace).message.as_deref(),
        Some("notStopped")
    );
    client.send::<request::Pause>(&PauseArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    client.wait_sent("event:stopped(pause)");
    assert_eq!(client.top_line(), point());
}

#[test]
fn step_in_and_out_move_between_functions() {
    let mut client = client(Script::default());
    client.start(&[line("add(x, 2)")]);
    client.wait_sent("event:stopped(breakpoint)");
    client.send::<request::StepIn>(&crate::protocol::StepInArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    client.wait_sent_times("event:stopped(step)", 1);
    let trace = client.send::<request::StackTrace>(&StackTraceArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    let frames = client
        .response(trace)
        .body::<request::StackTrace>()
        .unwrap()
        .stack_frames;
    assert_eq!(
        frames
            .iter()
            .map(|f| (f.name.as_str(), f.line))
            .collect::<Vec<_>>(),
        [("add", line("a + b")), ("main", line("add(x, 2)"))]
    );
    client.send::<request::StepOut>(&StepArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    client.wait_sent_times("event:stopped(step)", 2);
    assert_eq!(client.top_line(), line("println"));
}

#[test]
fn an_unknown_expression_fails_and_a_local_evaluates() {
    let mut client = client(Script::default());
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let known = client.send::<request::Evaluate>(&EvaluateArguments {
        expression: "x".into(),
        context: Some("watch".into()),
        ..Default::default()
    });
    assert_eq!(
        client
            .response(known)
            .body::<request::Evaluate>()
            .unwrap()
            .result,
        "1"
    );
    let unknown = client.send::<request::Evaluate>(&EvaluateArguments {
        expression: "nope".into(),
        ..Default::default()
    });
    assert!(!client.response(unknown).success);
}

#[test]
fn a_held_resume_succeeds_late_on_cancel() {
    let script = Script {
        faults: vec![Fault::new("next", FaultKind::ReplyAfter(Trigger::Cancel))],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let next = client.next();
    client.sync();
    assert!(!client.answered(next));
    client.send::<request::Cancel>(&CancelArguments {
        request_id: Some(next),
        ..Default::default()
    });
    assert!(client.response(next).success);
    client.wait_sent("event:stopped(step)");
    assert_eq!(client.top_line(), point() + 1);
}

#[test]
fn a_manually_held_reply_goes_on_release() {
    let script = Script {
        faults: vec![Fault::new("threads", FaultKind::ReplyAfter(Trigger::Manual)).nth(2)],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    client.sync();
    let held = client.send::<request::Threads>(&Empty::default());
    let after = client.send::<request::Threads>(&Empty::default());
    client.response(after);
    assert!(!client.answered(held));
    client.fake.release();
    assert!(client.response(held).success);
    // The release interrupted the fake's read; it reads on as before.
    assert_eq!(client.top_line(), point());
    assert_eq!(client.fake.exit(), None);
}

#[test]
fn restart_sends_initialized_again() {
    let mut client = client(Script::default());
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let before = client.sent().len();
    client.send::<request::Restart>(&Default::default());
    client.wait_sent_times("event:initialized", 2);
    client.send::<request::ConfigurationDone>(&Empty::default());
    client.wait_sent_times("event:stopped(breakpoint)", 2);
    assert_eq!(
        client.sent_after(before),
        [
            "response:restart",
            "event:initialized",
            "response:configurationDone",
            "event:thread(started)",
            "event:stopped(breakpoint)",
        ]
    );
}

#[test]
fn run_in_terminal_comes_before_the_launch_response() {
    let script = Script {
        run_in_terminal: true,
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("request:runInTerminal");
    client.sync();
    assert!(!client.sent().contains(&"response:launch".to_owned()));
    let sent = client.fake.sent();
    let Some(Message::Request(terminal)) =
        sent.iter().find(|m| label(m) == "request:runInTerminal")
    else {
        panic!("no runInTerminal");
    };
    let arguments = terminal.arguments::<reverse::RunInTerminal>().unwrap();
    assert_eq!(arguments.args[0], Program::default().path);
    let reply = terminal.respond::<reverse::RunInTerminal>(
        0,
        &RunInTerminalBody {
            process_id: Some(4242),
            ..Default::default()
        },
    );
    client.reply(reply);
    client.wait_sent("event:stopped(breakpoint)");
    assert!(client.sent().contains(&"response:launch".to_owned()));
}

#[test]
fn start_debugging_is_asked_after_launch() {
    let script = Script {
        start_debugging: true,
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let sent = client.sent();
    let launch = sent.iter().position(|l| l == "response:launch").unwrap();
    let child = sent
        .iter()
        .position(|l| l == "request:startDebugging")
        .unwrap();
    assert!(launch < child, "{sent:?}");
}

#[test]
fn a_lingering_adapter_outlives_disconnect_until_killed() {
    let script = Script {
        lingers: true,
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let disconnect = client.send::<request::Disconnect>(&DisconnectArguments {
        terminate_debuggee: Some(true),
        ..Default::default()
    });
    client.response(disconnect);
    async_io::block_on(client.writer.close()).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(client.fake.exit(), None);
    client.process.kill();
    let exit = async_io::block_on(client.process.exited());
    assert_eq!(exit.signal, Some(9));
    assert!(client.fake.killed());
}

#[test]
fn a_held_disconnect_keeps_the_fake_alive_until_released() {
    let script = Script {
        faults: vec![Fault::new(
            "disconnect",
            FaultKind::ReplyAfter(Trigger::Manual),
        )],
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.read_until("event:stopped(breakpoint)");
    let disconnect = client.send::<request::Disconnect>(&DisconnectArguments::default());
    // Still serving, unanswered.
    assert_eq!(client.top_line(), point());
    assert_eq!(client.fake.exit(), None);
    assert!(!client.answered(disconnect));
    client.fake.release();
    // The response, then the end of the stream, then the exit.
    let mut seen = Vec::new();
    while let Some(message) = client.read() {
        seen.push(label(&message));
    }
    assert_eq!(seen, ["response:stackTrace", "response:disconnect"]);
    let exit = async_io::block_on(client.process.exited());
    assert_eq!(exit.code, Some(0));
}

/// The first stop's frame, and a reference to its locals and to its
/// globals.
fn first_stop_scopes(client: &mut Client) -> (i64, i64) {
    let trace = client.send::<request::StackTrace>(&StackTraceArguments {
        thread_id: THREAD_ID,
        ..Default::default()
    });
    let frame = client
        .response(trace)
        .body::<request::StackTrace>()
        .unwrap()
        .stack_frames[0]
        .id;
    let scopes = client.send::<request::Scopes>(&ScopesArguments {
        frame_id: frame,
        ..Default::default()
    });
    let scopes = client
        .response(scopes)
        .body::<request::Scopes>()
        .unwrap()
        .scopes;
    (scopes[0].variables_reference, scopes[1].variables_reference)
}

fn variables(client: &mut Client, reference: i64) -> Vec<(String, String, i64)> {
    let seq = client.send::<request::Variables>(&VariablesArguments {
        variables_reference: reference,
        ..Default::default()
    });
    let response = client.response(seq);
    assert!(response.success, "{response:?}");
    response
        .body::<request::Variables>()
        .unwrap()
        .variables
        .into_iter()
        .map(|v| (v.name, v.value, v.variables_reference))
        .collect()
}

fn set_variable(client: &mut Client, reference: i64, name: &str, value: &str) -> ResponseMessage {
    let seq = client.send::<request::SetVariable>(&crate::protocol::SetVariableArguments {
        variables_reference: reference,
        name: name.into(),
        value: value.into(),
        ..Default::default()
    });
    client.response(seq)
}

#[test]
fn set_variable_changes_only_the_member_its_reference_names() {
    let mut client = client(Script::default());
    client.start(&[point()]);
    client.wait_sent("event:stopped(breakpoint)");
    let (locals, globals) = first_stop_scopes(&mut client);
    let point_children = variables(&mut client, locals)[1].2;
    // `point.x`, a child, shares its name with the local `x`.
    assert!(set_variable(&mut client, point_children, "x", "10").success);
    assert_eq!(
        variables(&mut client, locals)[0].1,
        "1",
        "the local x changed"
    );
    assert_eq!(variables(&mut client, point_children)[0].1, "10");
    // A name the container lacks fails, and changes nothing elsewhere.
    assert!(!set_variable(&mut client, globals, "x", "99").success);
    assert_eq!(variables(&mut client, locals)[0].1, "1");
    // A global, and the child, stay changed at the next stop.
    assert!(set_variable(&mut client, globals, "COUNT", "7").success);
    client.next();
    client.wait_sent("event:stopped(step)");
    let (locals, globals) = first_stop_scopes(&mut client);
    assert_eq!(variables(&mut client, globals)[0].1, "7");
    let point_children = variables(&mut client, locals)[1].2;
    assert_eq!(variables(&mut client, point_children)[0].1, "10");
    let evaluate = client.send::<request::Evaluate>(&EvaluateArguments {
        expression: "COUNT".into(),
        ..Default::default()
    });
    assert_eq!(
        client
            .response(evaluate)
            .body::<request::Evaluate>()
            .unwrap()
            .result,
        "7"
    );
}

#[test]
fn replaced_breakpoints_are_not_verified_later() {
    let script = Script {
        verify_breakpoints_later: true,
        ..Script::default()
    };
    let mut client = client(script);
    client.begin(&[point()]);
    let set = |client: &mut Client, lines: &[i64]| {
        let seq = client.send::<request::SetBreakpoints>(&SetBreakpointsArguments {
            source: Source {
                path: Some(Program::default().path),
                ..Default::default()
            },
            breakpoints: Some(
                lines
                    .iter()
                    .map(|&line| SourceBreakpoint {
                        line,
                        ..Default::default()
                    })
                    .collect(),
            ),
            ..Default::default()
        });
        client
            .response(seq)
            .body::<request::SetBreakpoints>()
            .unwrap()
            .breakpoints
    };
    // Replaced, then cleared, then set again: only the last is current.
    set(&mut client, &[line("add(x, 2)")]);
    set(&mut client, &[]);
    let current = set(&mut client, &[line("println")]);
    client.send::<request::ConfigurationDone>(&Empty::default());
    client.wait_sent("event:stopped(breakpoint)");
    let verified: Vec<_> = client
        .fake
        .sent()
        .iter()
        .filter_map(|message| match message {
            Message::Event(event) if event.event == "breakpoint" => Some(
                event
                    .body::<crate::protocol::event::BreakpointEvent>()
                    .unwrap()
                    .breakpoint
                    .id,
            ),
            _ => None,
        })
        .collect();
    assert_eq!(verified, [current[0].id]);
    assert_eq!(client.top_line(), line("println"));
}

#[test]
fn terminate_while_run_in_terminal_is_pending_starts_nothing() {
    let script = Script {
        run_in_terminal: true,
        ..Script::default()
    };
    let mut client = client(script);
    client.start(&[point()]);
    client.wait_sent("request:runInTerminal");
    let terminate = client.send::<request::Terminate>(&TerminateArguments::default());
    client.response(terminate);
    let sent = client.fake.sent();
    let Some(Message::Request(terminal)) =
        sent.iter().find(|m| label(m) == "request:runInTerminal")
    else {
        panic!("no runInTerminal");
    };
    let reply = terminal.respond::<reverse::RunInTerminal>(0, &RunInTerminalBody::default());
    client.reply(reply);
    client.sync();
    let sent = client.sent();
    assert!(sent.contains(&"response:launch!".to_owned()), "{sent:?}");
    assert!(sent.contains(&"event:terminated".to_owned()), "{sent:?}");
    assert!(
        !sent
            .iter()
            .any(|l| l.starts_with("event:thread") || l.starts_with("event:stopped")),
        "{sent:?}"
    );
}

#[test]
fn configuration_done_after_terminate_starts_nothing() {
    let script = Script {
        startup: Startup::LaunchResponseFirst,
        ..Script::default()
    };
    let mut client = client(script);
    client.begin(&[point()]);
    client.wait_sent("response:launch");
    client.send::<request::Terminate>(&TerminateArguments::default());
    client.send::<request::ConfigurationDone>(&Empty::default());
    client.sync();
    let sent = client.sent();
    assert!(
        sent.contains(&"response:configurationDone".to_owned()),
        "{sent:?}"
    );
    assert!(
        !sent
            .iter()
            .any(|l| l.starts_with("event:thread") || l.starts_with("event:stopped")),
        "{sent:?}"
    );
}

/// A writer that stalls: `write` (and `flush`) never finish when
/// `writes_stall`, and `close` never does.
struct Stalled {
    writes_stall: bool,
}

impl AsyncWrite for Stalled {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.writes_stall {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(Ok(buf.len()))
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.writes_stall {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(Ok(()))
        }
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Pending
    }
}

/// The fake on a stalled writer, and the client's writing end of its input.
fn stalled(writes_stall: bool) -> (FakeAdapter, FrameWriter<crate::fake::PipeWriter>) {
    let (fake_reads, client_writes) = crate::fake::pipe();
    let fake = crate::fake::serve(Script::default(), fake_reads, Stalled { writes_stall });
    (fake, FrameWriter::new(client_writes))
}

fn send_initialize(writer: &mut FrameWriter<crate::fake::PipeWriter>) {
    let initialize = Message::Request(RequestMessage::new::<request::Initialize>(
        1,
        &InitializeArguments {
            adapter_id: "fake".into(),
            ..Default::default()
        },
    ));
    async_io::block_on(writer.write(&initialize)).unwrap();
}

#[test]
fn a_kill_ends_the_fake_while_its_write_stalls() {
    let (fake, mut writer) = stalled(true);
    send_initialize(&mut writer);
    // The response is made (and recorded), but its write never finishes.
    fake.wait_until("the initialize response", |_, sent| !sent.is_empty());
    assert_eq!(fake.exit(), None);
    fake.kill();
    let exit = fake.wait_exit();
    assert_eq!(exit.signal, Some(9));
}

#[test]
fn a_kill_ends_the_fake_while_its_close_stalls() {
    let (fake, mut writer) = stalled(false);
    send_initialize(&mut writer);
    fake.wait_until("the initialize response", |_, sent| !sent.is_empty());
    // Its input ends, so it closes its output, which never finishes.
    async_io::block_on(writer.close()).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(fake.exit(), None, "the close didn't stall");
    fake.kill();
    assert_eq!(fake.wait_exit().signal, Some(9));
}

#[test]
fn a_killed_fake_never_closes_its_writer() {
    // Killed while idle: it ends at once, though close would stall.
    let (fake, mut writer) = stalled(false);
    send_initialize(&mut writer);
    fake.wait_until("the initialize response", |_, sent| !sent.is_empty());
    fake.kill();
    assert_eq!(fake.wait_exit().signal, Some(9));
}
