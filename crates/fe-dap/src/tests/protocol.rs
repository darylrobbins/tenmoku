// SPDX-License-Identifier: Apache-2.0

//! Every message type against an example shaped as the specification (1.71)
//! describes it: parsed into its typed arguments or body, written back, and
//! compared with the example, so no field is lost or renamed. Plus fields
//! no type names surviving the round trip.

use std::fmt::Debug;

use serde_json::{Value, json};

use crate::protocol::{
    Capabilities, Event, EventMessage, Message, Request, RequestMessage, ResponseMessage, event,
    request, reverse,
};

fn parse(example: &Value) -> Message {
    serde_json::from_value(example.clone()).unwrap_or_else(|err| panic!("{example}: {err}"))
}

fn written(message: &Message) -> Value {
    serde_json::to_value(message).unwrap()
}

/// `example`, a request `R`, through its typed arguments and back.
fn request_round_trip<R: Request>(example: Value) -> R::Arguments
where
    R::Arguments: Debug,
{
    let Message::Request(message) = parse(&example) else {
        panic!("not a request: {example}");
    };
    assert!(
        message.is::<R>(),
        "{} is not {}",
        message.command,
        R::COMMAND
    );
    let arguments = message.arguments::<R>().unwrap();
    let rebuilt = RequestMessage {
        arguments: Some(serde_json::to_value(&arguments).unwrap()),
        ..message.clone()
    };
    assert_eq!(
        written(&Message::Request(rebuilt)),
        example,
        "{arguments:#?}"
    );
    // And as built from scratch.
    let mut built = RequestMessage::new::<R>(message.seq, &arguments);
    built.extra = message.extra.clone();
    assert_eq!(written(&Message::Request(built)), example);
    arguments
}

/// `example`, a response to `R`, through its typed body and back.
fn response_round_trip<R: Request>(example: Value) -> R::Body
where
    R::Body: Debug,
{
    let Message::Response(message) = parse(&example) else {
        panic!("not a response: {example}");
    };
    assert_eq!(message.command, R::COMMAND);
    let body = message.body::<R>().unwrap();
    let rebuilt = ResponseMessage {
        body: Some(serde_json::to_value(&body).unwrap()),
        ..message
    };
    assert_eq!(written(&Message::Response(rebuilt)), example, "{body:#?}");
    body
}

/// `example`, event `E`, through its typed body and back.
fn event_round_trip<E: Event>(example: Value) -> E::Body
where
    E::Body: Debug,
{
    let Message::Event(message) = parse(&example) else {
        panic!("not an event: {example}");
    };
    assert!(message.is::<E>(), "{} is not {}", message.event, E::EVENT);
    let body = message.body::<E>().unwrap();
    let rebuilt = EventMessage {
        body: Some(serde_json::to_value(&body).unwrap()),
        ..message
    };
    assert_eq!(written(&Message::Event(rebuilt)), example, "{body:#?}");
    body
}

fn req(seq: i64, command: &str, arguments: Value) -> Value {
    json!({"seq": seq, "type": "request", "command": command, "arguments": arguments})
}

fn res(seq: i64, request_seq: i64, command: &str, body: Value) -> Value {
    json!({
        "seq": seq, "type": "response", "request_seq": request_seq,
        "success": true, "command": command, "body": body,
    })
}

fn ev(seq: i64, event: &str, body: Value) -> Value {
    json!({"seq": seq, "type": "event", "event": event, "body": body})
}

fn source() -> Value {
    json!({
        "name": "main.rs",
        "path": "/work/src/main.rs",
        "sourceReference": 0,
        "presentationHint": "normal",
        "origin": "internal module",
        "adapterData": {"opaque": [1, 2]},
        "checksums": [{"algorithm": "SHA256", "checksum": "e3b0c442"}],
    })
}

fn breakpoint() -> Value {
    json!({
        "id": 7,
        "verified": true,
        "message": "resolved",
        "source": source(),
        "line": 12,
        "column": 5,
        "endLine": 12,
        "endColumn": 20,
        "instructionReference": "0x1000",
        "offset": -2,
    })
}

#[test]
fn initialize_round_trips() {
    let arguments = request_round_trip::<request::Initialize>(req(
        1,
        "initialize",
        json!({
            "clientID": "tenmoku",
            "clientName": "Tenmoku",
            "adapterID": "debugpy",
            "locale": "en-US",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "pathFormat": "path",
            "supportsVariableType": true,
            "supportsVariablePaging": true,
            "supportsRunInTerminalRequest": true,
            "supportsMemoryReferences": false,
            "supportsProgressReporting": true,
            "supportsInvalidatedEvent": true,
            "supportsMemoryEvent": false,
            "supportsArgsCanBeInterpretedByShell": false,
            "supportsStartDebuggingRequest": false,
            "supportsANSIStyling": true,
        }),
    ));
    assert_eq!(arguments.adapter_id, "debugpy");
    assert_eq!(arguments.client_id.as_deref(), Some("tenmoku"));
    assert_eq!(arguments.supports_ansi_styling, Some(true));
    let capabilities = response_round_trip::<request::Initialize>(res(
        1,
        1,
        "initialize",
        // Too big for one `json!`.
        serde_json::from_str(
            r#"{
            "supportsConfigurationDoneRequest": true,
            "supportsFunctionBreakpoints": true,
            "supportsConditionalBreakpoints": true,
            "supportsHitConditionalBreakpoints": true,
            "supportsEvaluateForHovers": true,
            "exceptionBreakpointFilters": [
                {"filter": "raised", "label": "Raised Exceptions", "default": false,
                 "description": "Break whenever any exception is raised.",
                 "supportsCondition": true, "conditionDescription": "e.g. IndexError"},
                {"filter": "uncaught", "label": "Uncaught Exceptions", "default": true}
            ],
            "supportsStepBack": false,
            "supportsSetVariable": true,
            "supportsRestartFrame": false,
            "supportsGotoTargetsRequest": true,
            "supportsStepInTargetsRequest": true,
            "supportsCompletionsRequest": true,
            "completionTriggerCharacters": [".", "["],
            "supportsModulesRequest": true,
            "additionalModuleColumns": [{"attributeName": "version", "label": "Version"}],
            "supportedChecksumAlgorithms": ["MD5", "SHA256"],
            "supportsRestartRequest": false,
            "supportsExceptionOptions": true,
            "supportsValueFormattingOptions": true,
            "supportsExceptionInfoRequest": true,
            "supportTerminateDebuggee": true,
            "supportSuspendDebuggee": false,
            "supportsDelayedStackTraceLoading": true,
            "supportsLoadedSourcesRequest": false,
            "supportsLogPoints": true,
            "supportsTerminateThreadsRequest": false,
            "supportsSetExpression": true,
            "supportsTerminateRequest": true,
            "supportsDataBreakpoints": false,
            "supportsReadMemoryRequest": false,
            "supportsWriteMemoryRequest": false,
            "supportsDisassembleRequest": false,
            "supportsCancelRequest": true,
            "supportsBreakpointLocationsRequest": true,
            "supportsClipboardContext": true,
            "supportsSteppingGranularity": false,
            "supportsInstructionBreakpoints": false,
            "supportsExceptionFilterOptions": true,
            "supportsSingleThreadExecutionRequests": false,
            "supportsDataBreakpointBytes": false,
            "breakpointModes": [{"mode": "hw", "label": "Hardware", "appliesTo": ["source"]}],
            "supportsANSIStyling": false
        }"#,
        )
        .unwrap(),
    ));
    assert_eq!(capabilities.supports_configuration_done_request, Some(true));
    let filters = capabilities.exception_breakpoint_filters.unwrap();
    assert_eq!(filters[1].filter, "uncaught");
    assert_eq!(filters[1].default, Some(true));
    // Named nowhere here, kept in `extra`.
    assert!(capabilities.extra.contains_key("additionalModuleColumns"));
    assert!(capabilities.extra.contains_key("breakpointModes"));
}

#[test]
fn launch_round_trips() {
    let arguments = request_round_trip::<request::Launch>(req(
        2,
        "launch",
        json!({
            "noDebug": false,
            "__restart": {"attempt": 1},
            "program": "/work/app.py",
            "args": ["--verbose"],
            "cwd": "/work",
            "env": {"API_TOKEN": "${env:API_TOKEN}"},
            "justMyCode": true,
        }),
    ));
    assert_eq!(arguments["program"], "/work/app.py");
    response_round_trip::<request::Launch>(res(3, 2, "launch", json!({})));
}

#[test]
fn attach_round_trips() {
    let arguments = request_round_trip::<request::Attach>(req(
        2,
        "attach",
        json!({"processId": 4242, "connect": {"host": "127.0.0.1", "port": 5678}}),
    ));
    assert_eq!(arguments["processId"], 4242);
    response_round_trip::<request::Attach>(res(3, 2, "attach", json!({})));
}

#[test]
fn configuration_done_round_trips() {
    request_round_trip::<request::ConfigurationDone>(req(9, "configurationDone", json!({})));
    response_round_trip::<request::ConfigurationDone>(res(10, 9, "configurationDone", json!({})));
}

#[test]
fn set_breakpoints_round_trips() {
    let arguments = request_round_trip::<request::SetBreakpoints>(req(
        4,
        "setBreakpoints",
        json!({
            "source": source(),
            "breakpoints": [
                {"line": 12},
                {"line": 20, "column": 3, "condition": "i > 3", "hitCondition": "5",
                 "logMessage": "i is {i}", "mode": "hw"},
            ],
            "lines": [12, 20],
            "sourceModified": false,
        }),
    ));
    assert_eq!(arguments.source.path.as_deref(), Some("/work/src/main.rs"));
    assert_eq!(
        arguments.source.checksums.as_ref().unwrap()[0].algorithm,
        "SHA256"
    );
    let breakpoints = arguments.breakpoints.unwrap();
    assert_eq!(breakpoints[1].condition.as_deref(), Some("i > 3"));
    let body = response_round_trip::<request::SetBreakpoints>(res(
        5,
        4,
        "setBreakpoints",
        json!({"breakpoints": [
            breakpoint(),
            {"id": 8, "verified": false, "message": "no code", "reason": "failed", "line": 20},
        ]}),
    ));
    assert_eq!(body.breakpoints[0].id, Some(7));
    assert_eq!(body.breakpoints[1].reason.as_deref(), Some("failed"));
}

#[test]
fn set_function_breakpoints_round_trips() {
    let arguments = request_round_trip::<request::SetFunctionBreakpoints>(req(
        6,
        "setFunctionBreakpoints",
        json!({"breakpoints": [{"name": "main"}, {"name": "add", "condition": "a == 1", "hitCondition": ">2"}]}),
    ));
    assert_eq!(arguments.breakpoints[1].name, "add");
    response_round_trip::<request::SetFunctionBreakpoints>(res(
        7,
        6,
        "setFunctionBreakpoints",
        json!({"breakpoints": [{"id": 1, "verified": true, "line": 3}, {"id": 2, "verified": false}]}),
    ));
}

#[test]
fn set_exception_breakpoints_round_trips() {
    let arguments = request_round_trip::<request::SetExceptionBreakpoints>(req(
        8,
        "setExceptionBreakpoints",
        json!({
            "filters": ["uncaught"],
            "filterOptions": [{"filterId": "raised", "condition": "IndexError", "mode": "always"}],
            "exceptionOptions": [{"path": [{"names": ["Python Exceptions"]}], "breakMode": "always"}],
        }),
    ));
    assert_eq!(arguments.filters, vec!["uncaught"]);
    assert_eq!(arguments.filter_options.unwrap()[0].filter_id, "raised");
    response_round_trip::<request::SetExceptionBreakpoints>(res(
        9,
        8,
        "setExceptionBreakpoints",
        json!({"breakpoints": [{"verified": true}]}),
    ));
}

#[test]
fn threads_round_trips() {
    // `threads` takes no arguments: the specification's example has none.
    let example = json!({"seq": 11, "type": "request", "command": "threads"});
    let Message::Request(message) = parse(&example) else {
        panic!("not a request");
    };
    assert!(message.is::<request::Threads>());
    message.arguments::<request::Threads>().unwrap();
    assert_eq!(written(&Message::Request(message)), example);
    let body = response_round_trip::<request::Threads>(res(
        12,
        11,
        "threads",
        json!({"threads": [{"id": 1, "name": "MainThread"}, {"id": 2, "name": "worker"}]}),
    ));
    assert_eq!(body.threads[1].name, "worker");
}

#[test]
fn stack_trace_round_trips() {
    let arguments = request_round_trip::<request::StackTrace>(req(
        13,
        "stackTrace",
        json!({
            "threadId": 1, "startFrame": 0, "levels": 20,
            "format": {"hex": false, "parameters": true, "parameterTypes": true,
                       "parameterNames": true, "parameterValues": false, "line": true,
                       "module": false, "includeAll": false},
        }),
    ));
    assert_eq!(arguments.levels, Some(20));
    let body = response_round_trip::<request::StackTrace>(res(
        14,
        13,
        "stackTrace",
        json!({
            "stackFrames": [
                {"id": 1000, "name": "add", "source": source(), "line": 3, "column": 5,
                 "endLine": 3, "endColumn": 10, "canRestart": false,
                 "instructionPointerReference": "0x55d0", "moduleId": 4,
                 "presentationHint": "normal"},
                {"id": 1001, "name": "main", "line": 4, "column": 1, "moduleId": "core"},
            ],
            "totalFrames": 2,
        }),
    ));
    assert_eq!(body.stack_frames[0].name, "add");
    assert_eq!(body.stack_frames[1].module_id, Some(json!("core")));
    assert_eq!(body.total_frames, Some(2));
}

#[test]
fn scopes_round_trips() {
    let arguments =
        request_round_trip::<request::Scopes>(req(15, "scopes", json!({"frameId": 1000})));
    assert_eq!(arguments.frame_id, 1000);
    let body = response_round_trip::<request::Scopes>(res(
        16,
        15,
        "scopes",
        json!({"scopes": [
            {"name": "Locals", "presentationHint": "locals", "variablesReference": 5,
             "namedVariables": 2, "indexedVariables": 0, "expensive": false,
             "source": source(), "line": 1, "column": 1, "endLine": 9, "endColumn": 2},
            {"name": "Globals", "variablesReference": 6, "expensive": true},
        ]}),
    ));
    assert!(body.scopes[1].expensive);
}

#[test]
fn variables_round_trips() {
    let arguments = request_round_trip::<request::Variables>(req(
        17,
        "variables",
        json!({"variablesReference": 5, "filter": "named", "start": 0, "count": 100, "format": {"hex": true}}),
    ));
    assert_eq!(arguments.format.unwrap().hex, Some(true));
    let body = response_round_trip::<request::Variables>(res(
        18,
        17,
        "variables",
        json!({"variables": [
            {"name": "x", "value": "1", "type": "int", "variablesReference": 0,
             "presentationHint": {"kind": "data", "attributes": ["readOnly"], "visibility": "public", "lazy": false},
             "evaluateName": "x", "namedVariables": 0, "indexedVariables": 0,
             "memoryReference": "0x7ff0", "declarationLocationReference": 3,
             "valueLocationReference": 4},
            {"name": "point", "value": "Point(x=1, y=2)", "variablesReference": 9},
        ]}),
    ));
    assert_eq!(body.variables[0].r#type.as_deref(), Some("int"));
    assert_eq!(body.variables[1].variables_reference, 9);
}

#[test]
fn continue_round_trips() {
    let arguments = request_round_trip::<request::Continue>(req(
        19,
        "continue",
        json!({"threadId": 1, "singleThread": false}),
    ));
    assert_eq!(arguments.thread_id, 1);
    let body = response_round_trip::<request::Continue>(res(
        20,
        19,
        "continue",
        json!({"allThreadsContinued": false}),
    ));
    assert_eq!(body.all_threads_continued, Some(false));
}

#[test]
fn next_round_trips() {
    request_round_trip::<request::Next>(req(
        21,
        "next",
        json!({"threadId": 1, "singleThread": false, "granularity": "line"}),
    ));
    response_round_trip::<request::Next>(res(22, 21, "next", json!({})));
}

#[test]
fn step_in_round_trips() {
    let arguments = request_round_trip::<request::StepIn>(req(
        23,
        "stepIn",
        json!({"threadId": 1, "singleThread": false, "targetId": 3, "granularity": "statement"}),
    ));
    assert_eq!(arguments.target_id, Some(3));
    response_round_trip::<request::StepIn>(res(24, 23, "stepIn", json!({})));
}

#[test]
fn step_out_round_trips() {
    request_round_trip::<request::StepOut>(req(
        25,
        "stepOut",
        json!({"threadId": 2, "granularity": "instruction"}),
    ));
    response_round_trip::<request::StepOut>(res(26, 25, "stepOut", json!({})));
}

#[test]
fn pause_round_trips() {
    request_round_trip::<request::Pause>(req(27, "pause", json!({"threadId": 1})));
    response_round_trip::<request::Pause>(res(28, 27, "pause", json!({})));
}

#[test]
fn evaluate_round_trips() {
    let arguments = request_round_trip::<request::Evaluate>(req(
        29,
        "evaluate",
        json!({
            "expression": "x + 1", "frameId": 1000, "line": 3, "column": 4,
            "source": source(), "context": "watch", "format": {"hex": false},
        }),
    ));
    assert_eq!(arguments.context.as_deref(), Some("watch"));
    let body = response_round_trip::<request::Evaluate>(res(
        30,
        29,
        "evaluate",
        json!({
            "result": "2", "type": "int", "presentationHint": {"kind": "property"},
            "variablesReference": 0, "namedVariables": 0, "indexedVariables": 0,
            "memoryReference": "0x10", "valueLocationReference": 2,
        }),
    ));
    assert_eq!(body.result, "2");
}

#[test]
fn terminate_round_trips() {
    request_round_trip::<request::Terminate>(req(31, "terminate", json!({"restart": false})));
    response_round_trip::<request::Terminate>(res(32, 31, "terminate", json!({})));
}

#[test]
fn disconnect_round_trips() {
    let arguments = request_round_trip::<request::Disconnect>(req(
        33,
        "disconnect",
        json!({"restart": false, "terminateDebuggee": true, "suspendDebuggee": false}),
    ));
    assert_eq!(arguments.terminate_debuggee, Some(true));
    response_round_trip::<request::Disconnect>(res(34, 33, "disconnect", json!({})));
}

#[test]
fn restart_round_trips() {
    let arguments = request_round_trip::<request::Restart>(req(
        35,
        "restart",
        json!({"arguments": {"program": "/work/app.py", "noDebug": false}}),
    ));
    assert_eq!(arguments.arguments.unwrap()["program"], "/work/app.py");
    response_round_trip::<request::Restart>(res(36, 35, "restart", json!({})));
}

#[test]
fn source_round_trips() {
    request_round_trip::<request::SourceRequest>(req(
        37,
        "source",
        json!({"source": {"name": "<string>", "sourceReference": 12}, "sourceReference": 12}),
    ));
    let body = response_round_trip::<request::SourceRequest>(res(
        38,
        37,
        "source",
        json!({"content": "print('hi')\n", "mimeType": "text/x-python"}),
    ));
    assert_eq!(body.mime_type.as_deref(), Some("text/x-python"));
}

#[test]
fn cancel_round_trips() {
    let arguments = request_round_trip::<request::Cancel>(req(
        39,
        "cancel",
        json!({"requestId": 21, "progressId": "p1"}),
    ));
    assert_eq!(arguments.request_id, Some(21));
    response_round_trip::<request::Cancel>(res(40, 39, "cancel", json!({})));
}

#[test]
fn completions_round_trips() {
    request_round_trip::<request::Completions>(req(
        41,
        "completions",
        json!({"frameId": 1000, "text": "po", "column": 3, "line": 1}),
    ));
    let body = response_round_trip::<request::Completions>(res(
        42,
        41,
        "completions",
        json!({"targets": [
            {"label": "point", "text": "point", "sortText": "a", "detail": "Point",
             "type": "variable", "start": 0, "length": 2, "selectionStart": 5, "selectionLength": 0},
        ]}),
    ));
    assert_eq!(body.targets[0].r#type.as_deref(), Some("variable"));
}

#[test]
fn set_variable_round_trips() {
    request_round_trip::<request::SetVariable>(req(
        43,
        "setVariable",
        json!({"variablesReference": 5, "name": "x", "value": "42", "format": {"hex": false}}),
    ));
    response_round_trip::<request::SetVariable>(res(
        44,
        43,
        "setVariable",
        json!({
            "value": "42", "type": "int", "variablesReference": 0,
            "namedVariables": 0, "indexedVariables": 0, "memoryReference": "0x8",
            "valueLocationReference": 1,
        }),
    ));
}

#[test]
fn goto_round_trips() {
    request_round_trip::<request::Goto>(req(45, "goto", json!({"threadId": 1, "targetId": 4})));
    response_round_trip::<request::Goto>(res(46, 45, "goto", json!({})));
}

#[test]
fn goto_targets_round_trips() {
    request_round_trip::<request::GotoTargets>(req(
        47,
        "gotoTargets",
        json!({"source": source(), "line": 4, "column": 1}),
    ));
    response_round_trip::<request::GotoTargets>(res(
        48,
        47,
        "gotoTargets",
        json!({"targets": [{"id": 4, "label": "line 4", "line": 4, "column": 1,
                             "endLine": 4, "endColumn": 9, "instructionPointerReference": "0x20"}]}),
    ));
}

#[test]
fn restart_frame_round_trips() {
    request_round_trip::<request::RestartFrame>(req(49, "restartFrame", json!({"frameId": 1000})));
    response_round_trip::<request::RestartFrame>(res(50, 49, "restartFrame", json!({})));
}

#[test]
fn a_failed_response_round_trips_with_its_error() {
    let example = json!({
        "seq": 51, "type": "response", "request_seq": 2, "success": false,
        "command": "launch", "message": "Could not start",
        "body": {"error": {
            "id": 2001, "format": "Can't run {program}: {why}",
            "variables": {"program": "app.py", "why": "not found"},
            "sendTelemetry": false, "showUser": true,
            "url": "https://example.invalid/help", "urlLabel": "Help",
        }},
    });
    let Message::Response(message) = parse(&example) else {
        panic!("not a response");
    };
    assert!(!message.success);
    let error = message.error().unwrap();
    assert_eq!(error.id, 2001);
    assert_eq!(error.variables.unwrap()["why"], "not found");
    assert_eq!(written(&Message::Response(message)), example);

    // A cancelled request's failure, with no body.
    let cancelled = json!({
        "seq": 52, "type": "response", "request_seq": 21, "success": false,
        "command": "next", "message": "cancelled",
    });
    let Message::Response(message) = parse(&cancelled) else {
        panic!("not a response");
    };
    assert_eq!(message.message.as_deref(), Some("cancelled"));
    assert_eq!(message.error(), None);
    assert_eq!(written(&Message::Response(message)), cancelled);
}

#[test]
fn initialized_event_round_trips() {
    event_round_trip::<event::Initialized>(ev(1, "initialized", json!({})));
    // As the specification's example has it, with no body.
    let bare = json!({"seq": 1, "type": "event", "event": "initialized"});
    let Message::Event(message) = parse(&bare) else {
        panic!("not an event");
    };
    message.body::<event::Initialized>().unwrap();
    assert_eq!(written(&Message::Event(message)), bare);
}

#[test]
fn stopped_event_round_trips() {
    let body = event_round_trip::<event::Stopped>(ev(
        2,
        "stopped",
        json!({
            "reason": "breakpoint", "description": "Paused on breakpoint", "threadId": 1,
            "preserveFocusHint": false, "text": "bp 7", "allThreadsStopped": true,
            "hitBreakpointIds": [7],
        }),
    ));
    assert_eq!(body.reason, "breakpoint");
    assert_eq!(body.hit_breakpoint_ids, Some(vec![7]));
}

#[test]
fn continued_event_round_trips() {
    let body = event_round_trip::<event::Continued>(ev(
        3,
        "continued",
        json!({"threadId": 1, "allThreadsContinued": true}),
    ));
    assert_eq!(body.thread_id, 1);
}

#[test]
fn exited_event_round_trips() {
    let body = event_round_trip::<event::Exited>(ev(4, "exited", json!({"exitCode": 3})));
    assert_eq!(body.exit_code, 3);
}

#[test]
fn terminated_event_round_trips() {
    let body = event_round_trip::<event::Terminated>(ev(
        5,
        "terminated",
        json!({"restart": {"session": 2}}),
    ));
    assert_eq!(body.restart, Some(json!({"session": 2})));
}

#[test]
fn thread_event_round_trips() {
    let body = event_round_trip::<event::ThreadEvent>(ev(
        6,
        "thread",
        json!({"reason": "started", "threadId": 2}),
    ));
    assert_eq!(body.reason, "started");
}

#[test]
fn output_event_round_trips() {
    let body = event_round_trip::<event::Output>(ev(
        7,
        "output",
        json!({
            "category": "stdout", "output": "hello\n", "group": "start",
            "variablesReference": 0, "source": source(), "line": 2, "column": 1,
            "data": {"any": "thing"}, "locationReference": 0,
        }),
    ));
    assert_eq!(body.category.as_deref(), Some("stdout"));
}

#[test]
fn breakpoint_event_round_trips() {
    let body = event_round_trip::<event::BreakpointEvent>(ev(
        8,
        "breakpoint",
        json!({"reason": "changed", "breakpoint": breakpoint()}),
    ));
    assert!(body.breakpoint.verified);
}

#[test]
fn capabilities_event_round_trips() {
    let body = event_round_trip::<event::CapabilitiesEvent>(ev(
        9,
        "capabilities",
        json!({"capabilities": {"supportsRestartRequest": true}}),
    ));
    assert_eq!(body.capabilities.supports_restart_request, Some(true));
}

#[test]
fn invalidated_event_round_trips() {
    event_round_trip::<event::Invalidated>(ev(
        10,
        "invalidated",
        json!({"areas": ["variables", "stacks"], "threadId": 1, "stackFrameId": 1000}),
    ));
}

#[test]
fn process_event_round_trips() {
    let body = event_round_trip::<event::Process>(ev(
        11,
        "process",
        json!({"name": "/work/app.py", "systemProcessId": 4242, "isLocalProcess": true,
               "startMethod": "launch", "pointerSize": 64}),
    ));
    assert_eq!(body.system_process_id, Some(4242));
}

#[test]
fn progress_events_round_trip() {
    event_round_trip::<event::ProgressStart>(ev(
        12,
        "progressStart",
        json!({"progressId": "p1", "title": "Loading symbols", "requestId": 2,
               "cancellable": true, "message": "libc", "percentage": 12.5}),
    ));
    event_round_trip::<event::ProgressUpdate>(ev(
        13,
        "progressUpdate",
        json!({"progressId": "p1", "message": "libm", "percentage": 62.5}),
    ));
    event_round_trip::<event::ProgressEnd>(ev(
        14,
        "progressEnd",
        json!({"progressId": "p1", "message": "done"}),
    ));
}

#[test]
fn run_in_terminal_round_trips() {
    let arguments = request_round_trip::<reverse::RunInTerminal>(req(
        15,
        "runInTerminal",
        json!({
            "kind": "integrated", "title": "Python Debug Console", "cwd": "/work",
            "args": ["/usr/bin/python3", "app.py"],
            "env": {"PYTHONUNBUFFERED": "1", "REMOVED": null},
            "argsCanBeInterpretedByShell": false,
        }),
    ));
    let env = arguments.env.unwrap();
    assert_eq!(env["PYTHONUNBUFFERED"].as_deref(), Some("1"));
    assert_eq!(env["REMOVED"], None);
    let body = response_round_trip::<reverse::RunInTerminal>(res(
        16,
        15,
        "runInTerminal",
        json!({"processId": 999, "shellProcessId": 998}),
    ));
    assert_eq!(body.process_id, Some(999));
}

#[test]
fn start_debugging_round_trips() {
    let arguments = request_round_trip::<reverse::StartDebugging>(req(
        17,
        "startDebugging",
        json!({"configuration": {"type": "pwa-node", "__pendingTargetId": "abc"}, "request": "attach"}),
    ));
    assert_eq!(arguments.request, "attach");
    response_round_trip::<reverse::StartDebugging>(res(18, 17, "startDebugging", json!({})));
}

#[test]
fn unknown_fields_survive_a_round_trip() {
    // Unknown fields at every level: the envelope, the body, an object in
    // a list, an object inside that, and one in a typed nested struct.
    let example = json!({
        "seq": 20, "type": "response", "request_seq": 13, "success": true,
        "command": "stackTrace", "adapterTiming": 3,
        "body": {
            "totalFrames": 1,
            "futureField": {"nested": [1, 2, 3]},
            "stackFrames": [{
                "id": 1, "name": "main", "line": 1, "column": 1,
                "x-vendor": "kept",
                "source": {"path": "/a.rs", "vendorHash": "abc", "checksums": [
                    {"algorithm": "MD5", "checksum": "00", "salt": 1},
                ]},
            }],
        },
    });
    let Message::Response(message) = parse(&example) else {
        panic!("not a response");
    };
    assert_eq!(message.extra["adapterTiming"], 3);
    let body = message.body::<request::StackTrace>().unwrap();
    assert_eq!(body.extra["futureField"], json!({"nested": [1, 2, 3]}));
    let frame = &body.stack_frames[0];
    assert_eq!(frame.extra["x-vendor"], "kept");
    let source = frame.source.as_ref().unwrap();
    assert_eq!(source.extra["vendorHash"], "abc");
    assert_eq!(source.checksums.as_ref().unwrap()[0].extra["salt"], 1);
    response_round_trip::<request::StackTrace>(example.clone());

    // An event's and a request's too, and free-form launch arguments.
    event_round_trip::<event::Stopped>(json!({
        "seq": 21, "type": "event", "event": "stopped", "trace": "t1",
        "body": {"reason": "exception", "threadId": 1, "exceptionKind": {"name": "E"}},
    }));
    request_round_trip::<request::Next>(json!({
        "seq": 22, "type": "request", "command": "next", "clientTag": [true],
        "arguments": {"threadId": 1, "futureFlag": true},
    }));

    // A message type or event this crate doesn't know still parses.
    let module = json!({"seq": 23, "type": "event", "event": "module",
                        "body": {"reason": "new", "module": {"id": 1, "name": "libc"}}});
    assert_eq!(written(&parse(&module)), module);
}

#[test]
fn a_capabilities_event_merges_into_the_capabilities() {
    let mut capabilities: Capabilities = serde_json::from_value(json!({
        "supportsConfigurationDoneRequest": true,
        "supportsRestartRequest": false,
        "vendorThing": 1,
    }))
    .unwrap();
    let changes: Capabilities = serde_json::from_value(json!({
        "supportsRestartRequest": true,
        "vendorOther": 2,
    }))
    .unwrap();
    capabilities.merge(&changes);
    assert_eq!(capabilities.supports_configuration_done_request, Some(true));
    assert_eq!(capabilities.supports_restart_request, Some(true));
    assert_eq!(capabilities.extra["vendorThing"], 1);
    assert_eq!(capabilities.extra["vendorOther"], 2);
}

#[test]
fn a_message_of_unknown_type_is_refused() {
    let err = serde_json::from_value::<Message>(json!({"seq": 1, "type": "notification"}));
    assert!(err.is_err());
}

#[test]
fn a_null_where_any_value_is_allowed_survives_a_round_trip() {
    // The envelope's arguments and bodies.
    for example in [
        json!({"seq": 1, "type": "request", "command": "configurationDone", "arguments": null}),
        json!({"seq": 2, "type": "response", "request_seq": 1, "success": true,
               "command": "configurationDone", "body": null}),
        json!({"seq": 3, "type": "event", "event": "initialized", "body": null}),
    ] {
        let message = parse(&example);
        assert_eq!(written(&message), example);
    }
    let Message::Request(request) = parse(&json!({
        "seq": 1, "type": "request", "command": "configurationDone", "arguments": null,
    })) else {
        panic!("not a request");
    };
    assert_eq!(request.arguments, Some(Value::Null));
    request.arguments::<request::ConfigurationDone>().unwrap();

    // `adapterData`, `terminated`'s `restart` and `output`'s `data`.
    let source = request_round_trip::<request::SetBreakpoints>(req(
        4,
        "setBreakpoints",
        json!({"source": {"path": "/a.rs", "adapterData": null}}),
    ))
    .source;
    assert_eq!(source.adapter_data, Some(Value::Null));
    let terminated =
        event_round_trip::<event::Terminated>(ev(5, "terminated", json!({"restart": null})));
    assert_eq!(terminated.restart, Some(Value::Null));
    let output =
        event_round_trip::<event::Output>(ev(6, "output", json!({"output": "", "data": null})));
    assert_eq!(output.data, Some(Value::Null));
    // Absent stays absent.
    let terminated = event_round_trip::<event::Terminated>(ev(7, "terminated", json!({})));
    assert_eq!(terminated.restart, None);
}
