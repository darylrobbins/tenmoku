// SPDX-License-Identifier: Apache-2.0

//! A session's protocol log (docs/decisions/r1b9dw3g0g-debugger.md §2
//! "Output and logging", §8), for diagnosing adapters.
//!
//! - **By default it records metadata only:** each message's `seq`, type,
//!   `command` or `event`, `request_seq`, `success` and timing; for a
//!   failure only that it failed and its numeric `body.error.id`; the
//!   *names* of a protocol-defined body's top-level keys, and for the
//!   free-form `launch` and `attach` arguments only how many keys they
//!   have. No free text and no values: no error message, no launch
//!   argument, no `runInTerminal` argv or env, no variable, evaluation or
//!   output text, and nothing from the adapter's stderr.
//! - **Payloads are opt-in** ([`LogMode::Payloads`]): whole messages, error
//!   messages included, and the adapter's stderr, under a first line saying
//!   what that may still hold.
//! - **Redaction applies in every mode** to every string the log records:
//!   the values of `env` maps become `<redacted>`, and every string a
//!   `${env:NAME}` expanded to becomes `${env:NAME}` again, wherever it
//!   appears ([`Redactor`]).
//!
//! The log is a ring of the last [`LOG_ENTRIES`] entries. Nothing claims it
//! is safe to share. Its lines are protocol text (field names and JSON);
//! the prose in it goes through the catalogue.

use std::collections::VecDeque;
use std::fmt;
use std::time::Duration;

use kiln_l10n::tr;
use serde_json::Value;

use crate::protocol::{Message, ResponseMessage};

/// How many entries a session's log keeps.
pub const LOG_ENTRIES: usize = 2000;

/// What the log records of each message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogMode {
    /// Metadata only: no free text and no values.
    #[default]
    Metadata,
    /// Whole messages, redacted. The user setting `debug.log_payloads`,
    /// never a project one.
    Payloads,
}

/// A string a `${env:NAME}` in the configuration expanded to, so the log
/// can put the reference back wherever the value shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvExpansion {
    /// `NAME`, without `${env:` and `}`.
    pub name: String,
    pub value: String,
}

impl EnvExpansion {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Which way an entry went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Client to adapter.
    Sent,
    /// Adapter to client.
    Received,
    /// The session's own remark: a warning, or the payload log's first
    /// line.
    Note,
    /// A line of the adapter's stderr (payload mode only).
    Stderr,
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Since the session started, on its clock.
    pub at: Duration,
    pub direction: Direction,
    /// The line, already redacted.
    pub text: String,
}

impl fmt::Display for LogEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let arrow = match self.direction {
            Direction::Sent => "→",
            Direction::Received => "←",
            Direction::Note => "·",
            Direction::Stderr => "!",
        };
        write!(
            f,
            "[{}.{:03}] {arrow} {}",
            self.at.as_secs(),
            self.at.subsec_millis(),
            self.text
        )
    }
}

/// Puts `${env:NAME}` back for every expanded value, and hides `env` map
/// values.
#[derive(Clone, Debug, Default)]
pub struct Redactor {
    /// Longest value first, so a value inside another loses to it.
    expansions: Vec<EnvExpansion>,
}

/// What an `env` map's values become.
pub const REDACTED: &str = "<redacted>";

impl Redactor {
    pub fn new(expansions: &[EnvExpansion]) -> Self {
        let mut expansions: Vec<EnvExpansion> = expansions
            .iter()
            .filter(|expansion| !expansion.value.is_empty())
            .cloned()
            .collect();
        expansions.sort_by_key(|expansion| std::cmp::Reverse(expansion.value.len()));
        Self { expansions }
    }

    /// `text` with each expanded value replaced by its reference, in one
    /// pass, so a reference put back is never itself rewritten.
    pub fn text(&self, text: &str) -> String {
        if self.expansions.is_empty() {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        'scan: while !rest.is_empty() {
            for expansion in &self.expansions {
                if let Some(after) = rest.strip_prefix(expansion.value.as_str()) {
                    out.push_str("${env:");
                    out.push_str(&expansion.name);
                    out.push('}');
                    rest = after;
                    continue 'scan;
                }
            }
            let Some(c) = rest.chars().next() else {
                break;
            };
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
        out
    }

    /// Redacts a stream's bytes: `buffer` is what has come and not yet
    /// been redacted. Returns the redacted bytes that can go, and how much
    /// of `buffer` they used; the rest (a tail that may be the start of an
    /// expanded value) waits for more. Matching is on the raw bytes, before
    /// any line splitting or decoding, so a value is found across chunk
    /// boundaries and line breaks, whitespace and all.
    pub fn stream(&self, buffer: &[u8]) -> (Vec<u8>, usize) {
        let mut out = Vec::with_capacity(buffer.len());
        let mut at = 0;
        'scan: while at < buffer.len() {
            let rest = &buffer[at..];
            if self.expansions.iter().any(|expansion| {
                let value = expansion.value.as_bytes();
                value.len() > rest.len() && value.starts_with(rest)
            }) {
                break;
            }
            for expansion in &self.expansions {
                if rest.starts_with(expansion.value.as_bytes()) {
                    out.extend_from_slice(b"${env:");
                    out.extend_from_slice(expansion.name.as_bytes());
                    out.push(b'}');
                    at += expansion.value.len();
                    continue 'scan;
                }
            }
            out.push(rest[0]);
            at += 1;
        }
        (out, at)
    }

    /// `value` redacted in place: every `env` object's values hidden, then
    /// every string (keys included) rewritten by [`text`](Self::text).
    pub fn value(&self, value: &mut Value) {
        match value {
            Value::String(text) => *text = self.text(text),
            Value::Array(items) => items.iter_mut().for_each(|item| self.value(item)),
            Value::Object(map) => {
                let entries = std::mem::take(map);
                for (key, mut item) in entries {
                    if key == "env"
                        && let Value::Object(env) = &mut item
                    {
                        for (_, setting) in env.iter_mut() {
                            if !setting.is_null() {
                                *setting = Value::String(REDACTED.to_owned());
                            }
                        }
                    }
                    self.value(&mut item);
                    map.insert(self.text(&key), item);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
}

/// The most of a stderr line one entry holds; a longer line goes on in the
/// next.
pub const MAX_STDERR_LINE: usize = 4096;

/// A session's log: a ring of entries in one mode.
#[derive(Debug)]
pub struct ProtocolLog {
    mode: LogMode,
    redactor: Redactor,
    entries: VecDeque<LogEntry>,
    /// The adapter's stderr not yet redacted: a tail that may start an
    /// expanded value.
    stderr_raw: Vec<u8>,
    /// Redacted stderr of a line not yet ended.
    stderr_line: Vec<u8>,
}

impl ProtocolLog {
    pub fn new(mode: LogMode, expansions: &[EnvExpansion]) -> Self {
        let mut log = Self {
            mode,
            redactor: Redactor::new(expansions),
            entries: VecDeque::new(),
            stderr_raw: Vec::new(),
            stderr_line: Vec::new(),
        };
        if mode == LogMode::Payloads {
            log.note(Duration::ZERO, &tr!("dap-log-payloads-warning"));
        }
        log
    }

    pub fn mode(&self) -> LogMode {
        self.mode
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries.iter().cloned().collect()
    }

    fn push(&mut self, at: Duration, direction: Direction, text: String) {
        // The payload log's warning stays first, however long it runs.
        let keep_first = usize::from(self.mode == LogMode::Payloads);
        if self.entries.len() >= LOG_ENTRIES {
            self.entries.remove(keep_first);
        }
        self.entries.push_back(LogEntry {
            at,
            direction,
            text,
        });
    }

    /// The session's own remark, redacted.
    pub fn note(&mut self, at: Duration, text: &str) {
        let text = self.redactor.text(text);
        self.push(at, Direction::Note, text);
    }

    /// Bytes of the adapter's stderr: kept, a line per entry, only with
    /// payloads on. They are redacted as a stream before they are split
    /// into lines or decoded.
    pub fn stderr(&mut self, at: Duration, bytes: &[u8]) {
        if self.mode != LogMode::Payloads {
            return;
        }
        self.stderr_raw.extend_from_slice(bytes);
        let (redacted, used) = self.redactor.stream(&self.stderr_raw);
        self.stderr_raw.drain(..used);
        self.stderr_lines(at, &redacted);
    }

    /// The adapter's stderr has ended: what is left goes in, except a tail
    /// that could be the start of an expanded value, never completed, which
    /// shows as [`REDACTED`].
    pub fn stderr_end(&mut self, at: Duration) {
        if self.mode != LogMode::Payloads {
            return;
        }
        let raw = std::mem::take(&mut self.stderr_raw);
        let (mut redacted, used) = self.redactor.stream(&raw);
        if used < raw.len() {
            redacted.extend_from_slice(REDACTED.as_bytes());
        }
        self.stderr_lines(at, &redacted);
        if !self.stderr_line.is_empty() {
            self.stderr_entry(at);
        }
    }

    fn stderr_lines(&mut self, at: Duration, redacted: &[u8]) {
        for &byte in redacted {
            if byte == b'\n' {
                self.stderr_entry(at);
                continue;
            }
            if self.stderr_line.len() >= MAX_STDERR_LINE {
                self.stderr_entry(at);
            }
            self.stderr_line.push(byte);
        }
    }

    fn stderr_entry(&mut self, at: Duration) {
        let mut line = std::mem::take(&mut self.stderr_line);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        let text = String::from_utf8_lossy(&line).into_owned();
        self.push(at, Direction::Stderr, text);
    }

    /// A message the client sent, or (`received`) the adapter did. `took`
    /// is a response's time since its request went.
    pub fn message(
        &mut self,
        at: Duration,
        message: &Message,
        received: bool,
        took: Option<Duration>,
    ) {
        let direction = if received {
            Direction::Received
        } else {
            Direction::Sent
        };
        let mut text = self.metadata(message, took);
        if self.mode == LogMode::Payloads {
            let mut whole = serde_json::to_value(message).unwrap_or(Value::Null);
            self.redactor.value(&mut whole);
            text.push(' ');
            text.push_str(&whole.to_string());
        }
        self.push(at, direction, text);
    }

    /// The metadata line: names and numbers only.
    fn metadata(&self, message: &Message, took: Option<Duration>) -> String {
        let name = |name: &str| self.redactor.text(name);
        match message {
            Message::Request(request) => format!(
                "request seq={} command={} arguments={}",
                request.seq,
                name(&request.command),
                self.keys(
                    request.arguments.as_ref(),
                    argument_fields(&request.command)
                )
            ),
            Message::Response(response) => {
                let mut text = format!(
                    "response seq={} request_seq={} command={} success={}",
                    response.seq,
                    response.request_seq,
                    name(&response.command),
                    response.success
                );
                if response.success {
                    text.push_str(" body=");
                    text.push_str(
                        &self.keys(response.body.as_ref(), body_fields(&response.command)),
                    );
                } else if let Some(id) = error_id(response) {
                    text.push_str(&format!(" error.id={id}"));
                }
                if let Some(took) = took {
                    text.push_str(&format!(" took={}ms", took.as_millis()));
                }
                text
            }
            Message::Event(event) => format!(
                "event seq={} event={} body={}",
                event.seq,
                name(&event.event),
                self.keys(event.body.as_ref(), event_fields(&event.event))
            ),
        }
    }

    /// An object's top-level key names, for those `known` names as the
    /// protocol's fields for this message, sorted, then how many others:
    /// `[a, b]`, `[a, +2]`. Keys the protocol doesn't define (an adapter's
    /// own fields, free-form arguments) can be anything, a secret included,
    /// so they are only counted; with nothing known, `<N keys>`. `-` for no
    /// body.
    fn keys(&self, body: Option<&Value>, known: Option<&[&str]>) -> String {
        match body {
            Some(Value::Object(map)) => {
                let Some(known) = known else {
                    return format!("<{} keys>", map.len());
                };
                // Sorted, so a line reads the same whatever order the
                // message (or serde_json's `preserve_order`) gave.
                let mut names: Vec<&str> = map
                    .keys()
                    .map(String::as_str)
                    .filter(|key| known.contains(key))
                    .collect();
                names.sort_unstable();
                let others = map.len() - names.len();
                let mut text = names.join(", ");
                if others > 0 {
                    if !text.is_empty() {
                        text.push_str(", ");
                    }
                    text.push_str(&format!("+{others}"));
                }
                format!("[{text}]")
            }
            Some(Value::Null) | None => "-".to_owned(),
            Some(_) => "<value>".to_owned(),
        }
    }
}

/// A failed response's numeric `body.error.id`, if it has one.
fn error_id(response: &ResponseMessage) -> Option<i64> {
    response.body.as_ref()?.get("error")?.get("id")?.as_i64()
}

/// The protocol-defined top-level fields of request `command`'s arguments
/// (the client's and the adapter's reverse requests), sorted; `None` for a
/// command the protocol doesn't define here, or free-form arguments.
fn argument_fields(command: &str) -> Option<&'static [&'static str]> {
    Some(match command {
        "initialize" => &[
            "adapterID",
            "clientID",
            "clientName",
            "columnsStartAt1",
            "linesStartAt1",
            "locale",
            "pathFormat",
            "supportsANSIStyling",
            "supportsArgsCanBeInterpretedByShell",
            "supportsInvalidatedEvent",
            "supportsMemoryEvent",
            "supportsMemoryReferences",
            "supportsProgressReporting",
            "supportsRunInTerminalRequest",
            "supportsStartDebuggingRequest",
            "supportsVariablePaging",
            "supportsVariableType",
        ],
        "configurationDone" => &[],
        "setBreakpoints" => &["breakpoints", "lines", "source", "sourceModified"],
        "setFunctionBreakpoints" => &["breakpoints"],
        "setExceptionBreakpoints" => &["exceptionOptions", "filterOptions", "filters"],
        "threads" => &[],
        "stackTrace" => &["format", "levels", "startFrame", "threadId"],
        "scopes" => &["frameId"],
        "variables" => &["count", "filter", "format", "start", "variablesReference"],
        "continue" => &["singleThread", "threadId"],
        "next" => &["granularity", "singleThread", "threadId"],
        "stepIn" => &["granularity", "singleThread", "targetId", "threadId"],
        "stepOut" => &["granularity", "singleThread", "threadId"],
        "pause" => &["threadId"],
        "evaluate" => &[
            "column",
            "context",
            "expression",
            "format",
            "frameId",
            "line",
            "source",
        ],
        "terminate" => &["restart"],
        "disconnect" => &["restart", "suspendDebuggee", "terminateDebuggee"],
        "restart" => &["arguments"],
        "source" => &["source", "sourceReference"],
        "cancel" => &["progressId", "requestId"],
        "completions" => &["column", "frameId", "line", "text"],
        "setVariable" => &["format", "name", "value", "variablesReference"],
        "goto" => &["targetId", "threadId"],
        "gotoTargets" => &["column", "line", "source"],
        "restartFrame" => &["frameId"],
        "runInTerminal" => &[
            "args",
            "argsCanBeInterpretedByShell",
            "cwd",
            "env",
            "kind",
            "title",
        ],
        "startDebugging" => &["configuration", "request"],
        _ => return None,
    })
}

/// The protocol-defined top-level fields of a successful response's body
/// to `command`, sorted; `None` for a command not defined here.
fn body_fields(command: &str) -> Option<&'static [&'static str]> {
    Some(match command {
        "initialize" => &[
            "completionTriggerCharacters",
            "exceptionBreakpointFilters",
            "supportSuspendDebuggee",
            "supportTerminateDebuggee",
            "supportsANSIStyling",
            "supportsBreakpointLocationsRequest",
            "supportsCancelRequest",
            "supportsClipboardContext",
            "supportsCompletionsRequest",
            "supportsConditionalBreakpoints",
            "supportsConfigurationDoneRequest",
            "supportsDataBreakpointBytes",
            "supportsDataBreakpoints",
            "supportsDelayedStackTraceLoading",
            "supportsDisassembleRequest",
            "supportsEvaluateForHovers",
            "supportsExceptionFilterOptions",
            "supportsExceptionInfoRequest",
            "supportsExceptionOptions",
            "supportsFunctionBreakpoints",
            "supportsGotoTargetsRequest",
            "supportsHitConditionalBreakpoints",
            "supportsInstructionBreakpoints",
            "supportsLoadedSourcesRequest",
            "supportsLogPoints",
            "supportsModulesRequest",
            "supportsReadMemoryRequest",
            "supportsRestartFrame",
            "supportsRestartRequest",
            "supportsSetExpression",
            "supportsSetVariable",
            "supportsSingleThreadExecutionRequests",
            "supportsStepBack",
            "supportsStepInTargetsRequest",
            "supportsSteppingGranularity",
            "supportsTerminateRequest",
            "supportsTerminateThreadsRequest",
            "supportsValueFormattingOptions",
            "supportsWriteMemoryRequest",
        ],
        "launch" => &[],
        "attach" => &[],
        "configurationDone" => &[],
        "setBreakpoints" => &["breakpoints"],
        "setFunctionBreakpoints" => &["breakpoints"],
        "setExceptionBreakpoints" => &["breakpoints"],
        "threads" => &["threads"],
        "stackTrace" => &["stackFrames", "totalFrames"],
        "scopes" => &["scopes"],
        "variables" => &["variables"],
        "continue" => &["allThreadsContinued"],
        "next" => &[],
        "stepIn" => &[],
        "stepOut" => &[],
        "pause" => &[],
        "evaluate" => &[
            "indexedVariables",
            "memoryReference",
            "namedVariables",
            "presentationHint",
            "result",
            "type",
            "valueLocationReference",
            "variablesReference",
        ],
        "terminate" => &[],
        "disconnect" => &[],
        "restart" => &[],
        "source" => &["content", "mimeType"],
        "cancel" => &[],
        "completions" => &["targets"],
        "setVariable" => &[
            "indexedVariables",
            "memoryReference",
            "namedVariables",
            "type",
            "value",
            "valueLocationReference",
            "variablesReference",
        ],
        "goto" => &[],
        "gotoTargets" => &["targets"],
        "restartFrame" => &[],
        "runInTerminal" => &["processId", "shellProcessId"],
        "startDebugging" => &[],
        _ => return None,
    })
}

/// The protocol-defined top-level fields of event `event`'s body, sorted;
/// `None` for an event not defined here.
fn event_fields(event: &str) -> Option<&'static [&'static str]> {
    Some(match event {
        "initialized" => &[],
        "stopped" => &[
            "allThreadsStopped",
            "description",
            "hitBreakpointIds",
            "preserveFocusHint",
            "reason",
            "text",
            "threadId",
        ],
        "continued" => &["allThreadsContinued", "threadId"],
        "exited" => &["exitCode"],
        "terminated" => &["restart"],
        "thread" => &["reason", "threadId"],
        "output" => &[
            "category",
            "column",
            "data",
            "group",
            "line",
            "locationReference",
            "output",
            "source",
            "variablesReference",
        ],
        "breakpoint" => &["breakpoint", "reason"],
        "capabilities" => &["capabilities"],
        "invalidated" => &["areas", "stackFrameId", "threadId"],
        "process" => &[
            "isLocalProcess",
            "name",
            "pointerSize",
            "startMethod",
            "systemProcessId",
        ],
        "progressStart" => &[
            "cancellable",
            "message",
            "percentage",
            "progressId",
            "requestId",
            "title",
        ],
        "progressUpdate" => &["message", "percentage", "progressId"],
        "progressEnd" => &["message", "progressId"],
        _ => return None,
    })
}
