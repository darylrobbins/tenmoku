// SPDX-License-Identifier: Apache-2.0

//! Hand-written serde types for the Debug Adapter Protocol messages the
//! client uses (docs/decisions/r1b9dw3g0g-debugger.md §2), at protocol 1.71.
//!
//! - [`Message`] is the envelope: a request, a response or an event, each
//!   with its `seq`.
//! - [`Request`] and [`Event`] tie a command or event name to its typed
//!   arguments and body: the marker types in [`request`], [`event`] and
//!   [`reverse`] (the requests an adapter sends the client).
//! - Every object keeps the fields it doesn't name in an `extra` map
//!   (`#[serde(flatten)]`), so nothing an adapter sends is lost on a round
//!   trip. Integers are `i64`, following the specification's `format`
//!   annotations. Open string enumerations (a stop's `reason`, an output's
//!   `category`) are `String`s, so a value this crate doesn't know still
//!   parses.
//! - `launch` and `attach` arguments are adapter-specific, so they are a
//!   plain JSON map.
//!
//! No DAP crate is used (§1): none tracks 1.71. No public item here names a
//! type of another crate except serde's traits and `serde_json`'s values.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::transport::ProtocolError;

/// A JSON object: free-form arguments, and the fields a type doesn't name.
pub type Object = Map<String, Value>;

/// One message, as framed by [`crate::transport`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    Request(RequestMessage),
    Response(ResponseMessage),
    Event(EventMessage),
}

impl Message {
    pub fn seq(&self) -> i64 {
        match self {
            Message::Request(request) => request.seq,
            Message::Response(response) => response.seq,
            Message::Event(event) => event.seq,
        }
    }
}

/// A request, from the client or (a reverse request) from the adapter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestMessage {
    pub seq: i64,
    pub command: String,
    /// Present-but-`null` is `Some(Value::Null)`, so it is written back.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub arguments: Option<Value>,
    #[serde(flatten)]
    pub extra: Object,
}

impl RequestMessage {
    /// Request `R` numbered `seq`, with `arguments`.
    pub fn new<R: Request>(seq: i64, arguments: &R::Arguments) -> Self {
        Self {
            seq,
            command: R::COMMAND.to_owned(),
            arguments: Some(to_value(arguments)),
            extra: Object::new(),
        }
    }

    /// Whether this is request `R`.
    pub fn is<R: Request>(&self) -> bool {
        self.command == R::COMMAND
    }

    /// Its arguments as `R`'s; missing arguments read as `{}`.
    pub fn arguments<R: Request>(&self) -> Result<R::Arguments, ProtocolError> {
        from_value(self.arguments.as_ref())
    }

    /// A successful response to it, numbered `seq`.
    pub fn respond<R: Request>(&self, seq: i64, body: &R::Body) -> ResponseMessage {
        ResponseMessage {
            seq,
            request_seq: self.seq,
            success: true,
            command: self.command.clone(),
            message: None,
            body: Some(to_value(body)),
            extra: Object::new(),
        }
    }

    /// A failed response to it, numbered `seq`: `message` is the short
    /// reason (`"cancelled"`, `"notStopped"` or free text) and `error` the
    /// detailed one.
    pub fn fail(
        &self,
        seq: i64,
        message: impl Into<String>,
        error: Option<ErrorMessage>,
    ) -> ResponseMessage {
        ResponseMessage {
            seq,
            request_seq: self.seq,
            success: false,
            command: self.command.clone(),
            message: Some(message.into()),
            body: error.map(|error| {
                to_value(&ErrorBody {
                    error: Some(error),
                    extra: Object::new(),
                })
            }),
            extra: Object::new(),
        }
    }
}

/// A response to a request. Its `request_seq` is the specification's one
/// snake_case field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResponseMessage {
    pub seq: i64,
    pub request_seq: i64,
    pub success: bool,
    pub command: String,
    /// On failure, the short reason: `"cancelled"`, `"notStopped"`, or
    /// free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Present-but-`null` is `Some(Value::Null)`, so it is written back.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub body: Option<Value>,
    #[serde(flatten)]
    pub extra: Object,
}

impl ResponseMessage {
    /// Its body as request `R`'s; a missing body reads as `{}`.
    pub fn body<R: Request>(&self) -> Result<R::Body, ProtocolError> {
        from_value(self.body.as_ref())
    }

    /// A failure's detailed error, if its body has one.
    pub fn error(&self) -> Option<ErrorMessage> {
        if self.success {
            return None;
        }
        from_value::<ErrorBody>(self.body.as_ref()).ok()?.error
    }
}

/// An event from the adapter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventMessage {
    pub seq: i64,
    pub event: String,
    /// Present-but-`null` is `Some(Value::Null)`, so it is written back.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub body: Option<Value>,
    #[serde(flatten)]
    pub extra: Object,
}

impl EventMessage {
    /// Event `E` numbered `seq`, with `body`.
    pub fn new<E: Event>(seq: i64, body: &E::Body) -> Self {
        Self {
            seq,
            event: E::EVENT.to_owned(),
            body: Some(to_value(body)),
            extra: Object::new(),
        }
    }

    /// Whether this is event `E`.
    pub fn is<E: Event>(&self) -> bool {
        self.event == E::EVENT
    }

    /// Its body as `E`'s; a missing body reads as `{}`.
    pub fn body<E: Event>(&self) -> Result<E::Body, ProtocolError> {
        from_value(self.body.as_ref())
    }
}

/// A request's command, arguments and response body.
pub trait Request {
    const COMMAND: &'static str;
    type Arguments: Serialize + DeserializeOwned;
    type Body: Serialize + DeserializeOwned;
}

/// An event's name and body.
pub trait Event {
    const EVENT: &'static str;
    type Body: Serialize + DeserializeOwned;
}

fn to_value<T: Serialize>(value: &T) -> Value {
    // These types serialise to JSON infallibly: string keys, no non-finite
    // floats from the client.
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A present field as `Some`, `null` included: for the fields the
/// specification lets be any JSON value, where a `null` an adapter sends
/// must come back as it went.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// Typed arguments or a typed body; missing or `null` reads as `{}`.
fn from_value<T: DeserializeOwned>(value: Option<&Value>) -> Result<T, ProtocolError> {
    let value = match value {
        None | Some(Value::Null) => Value::Object(Object::new()),
        Some(value) => value.clone(),
    };
    serde_json::from_value(value).map_err(|err| ProtocolError::BadMessage(err.to_string()))
}

/// A protocol object: the named fields, required then (after `;`)
/// optional, camelCase in JSON, with every other field in `extra`.
macro_rules! object {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $( $(#[$rmeta:meta])* $req:ident : $rty:ty, )*
            $( ; $( $(#[$ometa:meta])* $opt:ident : $oty:ty, )* )?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct $name {
            $( $(#[$rmeta])* pub $req: $rty, )*
            $($(
                $(#[$ometa])*
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $opt: Option<$oty>,
            )*)?
            /// The fields this type doesn't name, kept as they came.
            #[serde(flatten)]
            pub extra: Object,
        }
    };
}

object! {
    /// An object with no named fields: the body of a response or event
    /// that has none, and the arguments of a request that takes none.
    pub struct Empty {}
}

object! {
    /// A failed response's body.
    pub struct ErrorBody {
        ;
        error: ErrorMessage,
    }
}

object! {
    /// The specification's `Message`: a structured error.
    pub struct ErrorMessage {
        id: i64,
        /// A format string with `{name}` placeholders for `variables`.
        format: String,
        ;
        variables: BTreeMap<String, String>,
        send_telemetry: bool,
        show_user: bool,
        url: String,
        url_label: String,
    }
}

object! {
    /// What an adapter supports: `initialize`'s response, and the
    /// `capabilities` event's changes to it.
    pub struct Capabilities {
        ;
        supports_configuration_done_request: bool,
        supports_function_breakpoints: bool,
        supports_conditional_breakpoints: bool,
        supports_hit_conditional_breakpoints: bool,
        supports_evaluate_for_hovers: bool,
        exception_breakpoint_filters: Vec<ExceptionBreakpointsFilter>,
        supports_step_back: bool,
        supports_set_variable: bool,
        supports_restart_frame: bool,
        supports_goto_targets_request: bool,
        supports_step_in_targets_request: bool,
        supports_completions_request: bool,
        completion_trigger_characters: Vec<String>,
        supports_modules_request: bool,
        supports_restart_request: bool,
        supports_exception_options: bool,
        supports_value_formatting_options: bool,
        supports_exception_info_request: bool,
        support_terminate_debuggee: bool,
        support_suspend_debuggee: bool,
        supports_delayed_stack_trace_loading: bool,
        supports_loaded_sources_request: bool,
        supports_log_points: bool,
        supports_terminate_threads_request: bool,
        supports_set_expression: bool,
        supports_terminate_request: bool,
        supports_data_breakpoints: bool,
        supports_read_memory_request: bool,
        supports_write_memory_request: bool,
        supports_disassemble_request: bool,
        supports_cancel_request: bool,
        supports_breakpoint_locations_request: bool,
        supports_clipboard_context: bool,
        supports_stepping_granularity: bool,
        supports_instruction_breakpoints: bool,
        supports_exception_filter_options: bool,
        supports_single_thread_execution_requests: bool,
        supports_data_breakpoint_bytes: bool,
        #[serde(rename = "supportsANSIStyling")]
        supports_ansi_styling: bool,
    }
}

impl Capabilities {
    /// Applies a `capabilities` event: the fields it sets replace these.
    pub fn merge(&mut self, changes: &Capabilities) {
        let (Value::Object(mut merged), Value::Object(changes)) =
            (to_value(&*self), to_value(changes))
        else {
            return;
        };
        merged.extend(changes);
        if let Ok(caps) = serde_json::from_value(Value::Object(merged)) {
            *self = caps;
        }
    }
}

object! {
    /// An exception filter the adapter offers for `setExceptionBreakpoints`.
    pub struct ExceptionBreakpointsFilter {
        filter: String,
        label: String,
        ;
        description: String,
        /// Whether it is on unless the user turns it off.
        default: bool,
        supports_condition: bool,
        condition_description: String,
    }
}

object! {
    /// A source file, or adapter-held source (`sourceReference` > 0).
    pub struct Source {
        ;
        name: String,
        path: String,
        source_reference: i64,
        presentation_hint: String,
        origin: String,
        sources: Vec<Source>,
        #[serde(deserialize_with = "present")]
        adapter_data: Value,
        checksums: Vec<Checksum>,
    }
}

object! {
    pub struct Checksum {
        algorithm: String,
        checksum: String,
    }
}

object! {
    /// A breakpoint as the client asks for it.
    pub struct SourceBreakpoint {
        line: i64,
        ;
        column: i64,
        condition: String,
        hit_condition: String,
        log_message: String,
        mode: String,
    }
}

object! {
    pub struct FunctionBreakpoint {
        name: String,
        ;
        condition: String,
        hit_condition: String,
    }
}

object! {
    pub struct ExceptionFilterOptions {
        filter_id: String,
        ;
        condition: String,
        mode: String,
    }
}

object! {
    /// A breakpoint as the adapter reports it.
    pub struct Breakpoint {
        verified: bool,
        ;
        id: i64,
        message: String,
        source: Source,
        line: i64,
        column: i64,
        end_line: i64,
        end_column: i64,
        instruction_reference: String,
        offset: i64,
        /// Why it isn't verified: `"pending"` or `"failed"`.
        reason: String,
    }
}

object! {
    pub struct Thread {
        id: i64,
        name: String,
    }
}

object! {
    pub struct StackFrame {
        id: i64,
        name: String,
        line: i64,
        column: i64,
        ;
        source: Source,
        end_line: i64,
        end_column: i64,
        can_restart: bool,
        instruction_pointer_reference: String,
        /// A number or a string.
        module_id: Value,
        presentation_hint: String,
    }
}

object! {
    pub struct Scope {
        name: String,
        variables_reference: i64,
        expensive: bool,
        ;
        presentation_hint: String,
        named_variables: i64,
        indexed_variables: i64,
        source: Source,
        line: i64,
        column: i64,
        end_line: i64,
        end_column: i64,
    }
}

object! {
    pub struct Variable {
        name: String,
        value: String,
        /// Non-zero when it has children, fetched with `variables`.
        variables_reference: i64,
        ;
        r#type: String,
        presentation_hint: VariablePresentationHint,
        evaluate_name: String,
        named_variables: i64,
        indexed_variables: i64,
        memory_reference: String,
        declaration_location_reference: i64,
        value_location_reference: i64,
    }
}

object! {
    pub struct VariablePresentationHint {
        ;
        kind: String,
        attributes: Vec<String>,
        visibility: String,
        lazy: bool,
    }
}

object! {
    pub struct ValueFormat {
        ;
        hex: bool,
    }
}

object! {
    pub struct StackFrameFormat {
        ;
        hex: bool,
        parameters: bool,
        parameter_types: bool,
        parameter_names: bool,
        parameter_values: bool,
        line: bool,
        module: bool,
        include_all: bool,
    }
}

object! {
    pub struct CompletionItem {
        label: String,
        ;
        text: String,
        sort_text: String,
        detail: String,
        r#type: String,
        start: i64,
        length: i64,
        selection_start: i64,
        selection_length: i64,
    }
}

object! {
    pub struct GotoTarget {
        id: i64,
        label: String,
        line: i64,
        ;
        column: i64,
        end_line: i64,
        end_column: i64,
        instruction_pointer_reference: String,
    }
}

// Request arguments and response bodies.

object! {
    pub struct InitializeArguments {
        #[serde(rename = "adapterID")]
        adapter_id: String,
        ;
        #[serde(rename = "clientID")]
        client_id: String,
        client_name: String,
        locale: String,
        lines_start_at1: bool,
        columns_start_at1: bool,
        /// `"path"` or `"uri"`.
        path_format: String,
        supports_variable_type: bool,
        supports_variable_paging: bool,
        supports_run_in_terminal_request: bool,
        supports_memory_references: bool,
        supports_progress_reporting: bool,
        supports_invalidated_event: bool,
        supports_memory_event: bool,
        supports_args_can_be_interpreted_by_shell: bool,
        supports_start_debugging_request: bool,
        #[serde(rename = "supportsANSIStyling")]
        supports_ansi_styling: bool,
    }
}

object! {
    /// `setBreakpoints`' and `setFunctionBreakpoints`' body.
    pub struct BreakpointsBody {
        #[serde(default)]
        breakpoints: Vec<Breakpoint>,
    }
}

object! {
    pub struct SetBreakpointsArguments {
        source: Source,
        ;
        breakpoints: Vec<SourceBreakpoint>,
        lines: Vec<i64>,
        source_modified: bool,
    }
}

object! {
    pub struct SetFunctionBreakpointsArguments {
        breakpoints: Vec<FunctionBreakpoint>,
    }
}

object! {
    pub struct SetExceptionBreakpointsArguments {
        filters: Vec<String>,
        ;
        filter_options: Vec<ExceptionFilterOptions>,
        exception_options: Vec<Value>,
    }
}

object! {
    pub struct SetExceptionBreakpointsBody {
        ;
        breakpoints: Vec<Breakpoint>,
    }
}

object! {
    pub struct ThreadsBody {
        #[serde(default)]
        threads: Vec<Thread>,
    }
}

object! {
    pub struct StackTraceArguments {
        thread_id: i64,
        ;
        start_frame: i64,
        levels: i64,
        format: StackFrameFormat,
    }
}

object! {
    pub struct StackTraceBody {
        #[serde(default)]
        stack_frames: Vec<StackFrame>,
        ;
        total_frames: i64,
    }
}

object! {
    pub struct ScopesArguments {
        frame_id: i64,
    }
}

object! {
    pub struct ScopesBody {
        #[serde(default)]
        scopes: Vec<Scope>,
    }
}

object! {
    pub struct VariablesArguments {
        variables_reference: i64,
        ;
        /// `"indexed"` or `"named"`.
        filter: String,
        start: i64,
        count: i64,
        format: ValueFormat,
    }
}

object! {
    pub struct VariablesBody {
        #[serde(default)]
        variables: Vec<Variable>,
    }
}

object! {
    pub struct ContinueArguments {
        thread_id: i64,
        ;
        single_thread: bool,
    }
}

object! {
    pub struct ContinueBody {
        ;
        /// `false` when only the requested thread runs.
        all_threads_continued: bool,
    }
}

object! {
    /// `next`'s and `stepOut`'s arguments.
    pub struct StepArguments {
        thread_id: i64,
        ;
        single_thread: bool,
        /// `"statement"`, `"line"` or `"instruction"`.
        granularity: String,
    }
}

object! {
    pub struct StepInArguments {
        thread_id: i64,
        ;
        single_thread: bool,
        target_id: i64,
        granularity: String,
    }
}

object! {
    pub struct PauseArguments {
        thread_id: i64,
    }
}

object! {
    pub struct EvaluateArguments {
        expression: String,
        ;
        frame_id: i64,
        line: i64,
        column: i64,
        source: Source,
        /// `"watch"`, `"repl"`, `"hover"`, `"clipboard"` or `"variables"`.
        context: String,
        format: ValueFormat,
    }
}

object! {
    pub struct EvaluateBody {
        result: String,
        variables_reference: i64,
        ;
        r#type: String,
        presentation_hint: VariablePresentationHint,
        named_variables: i64,
        indexed_variables: i64,
        memory_reference: String,
        value_location_reference: i64,
    }
}

object! {
    pub struct TerminateArguments {
        ;
        restart: bool,
    }
}

object! {
    pub struct DisconnectArguments {
        ;
        restart: bool,
        terminate_debuggee: bool,
        suspend_debuggee: bool,
    }
}

object! {
    pub struct RestartArguments {
        ;
        /// The latest `launch` or `attach` arguments.
        arguments: Object,
    }
}

object! {
    pub struct SourceArguments {
        source_reference: i64,
        ;
        source: Source,
    }
}

object! {
    pub struct SourceBody {
        content: String,
        ;
        mime_type: String,
    }
}

object! {
    pub struct CancelArguments {
        ;
        request_id: i64,
        progress_id: String,
    }
}

object! {
    pub struct CompletionsArguments {
        text: String,
        column: i64,
        ;
        frame_id: i64,
        line: i64,
    }
}

object! {
    pub struct CompletionsBody {
        #[serde(default)]
        targets: Vec<CompletionItem>,
    }
}

object! {
    pub struct SetVariableArguments {
        variables_reference: i64,
        name: String,
        value: String,
        ;
        format: ValueFormat,
    }
}

object! {
    pub struct SetVariableBody {
        value: String,
        ;
        r#type: String,
        variables_reference: i64,
        named_variables: i64,
        indexed_variables: i64,
        memory_reference: String,
        value_location_reference: i64,
    }
}

object! {
    pub struct GotoArguments {
        thread_id: i64,
        target_id: i64,
    }
}

object! {
    pub struct GotoTargetsArguments {
        source: Source,
        line: i64,
        ;
        column: i64,
    }
}

object! {
    pub struct GotoTargetsBody {
        #[serde(default)]
        targets: Vec<GotoTarget>,
    }
}

object! {
    pub struct RestartFrameArguments {
        frame_id: i64,
    }
}

// Event bodies.

object! {
    pub struct StoppedBody {
        /// `"step"`, `"breakpoint"`, `"exception"`, `"pause"`, `"entry"`,
        /// `"goto"`, `"function breakpoint"`, … or the adapter's own.
        reason: String,
        ;
        description: String,
        thread_id: i64,
        preserve_focus_hint: bool,
        text: String,
        all_threads_stopped: bool,
        hit_breakpoint_ids: Vec<i64>,
    }
}

object! {
    pub struct ContinuedBody {
        thread_id: i64,
        ;
        all_threads_continued: bool,
    }
}

object! {
    pub struct ExitedBody {
        exit_code: i64,
    }
}

object! {
    pub struct TerminatedBody {
        ;
        /// The adapter's request to restart, passed back as `__restart`.
        #[serde(deserialize_with = "present")]
        restart: Value,
    }
}

object! {
    pub struct ThreadBody {
        /// `"started"`, `"exited"`, or the adapter's own.
        reason: String,
        thread_id: i64,
    }
}

object! {
    pub struct OutputBody {
        output: String,
        ;
        /// `"console"`, `"important"`, `"stdout"`, `"stderr"`,
        /// `"telemetry"`, or the adapter's own.
        category: String,
        group: String,
        variables_reference: i64,
        source: Source,
        line: i64,
        column: i64,
        #[serde(deserialize_with = "present")]
        data: Value,
        location_reference: i64,
    }
}

object! {
    pub struct BreakpointBody {
        /// `"changed"`, `"new"`, `"removed"`, or the adapter's own.
        reason: String,
        breakpoint: Breakpoint,
    }
}

object! {
    pub struct CapabilitiesBody {
        capabilities: Capabilities,
    }
}

object! {
    pub struct InvalidatedBody {
        ;
        /// `"all"`, `"stacks"`, `"threads"`, `"variables"`, ….
        areas: Vec<String>,
        thread_id: i64,
        stack_frame_id: i64,
    }
}

object! {
    pub struct ProcessBody {
        name: String,
        ;
        system_process_id: i64,
        is_local_process: bool,
        /// `"launch"`, `"attach"` or `"attachForSuspendedLaunch"`.
        start_method: String,
        pointer_size: i64,
    }
}

object! {
    pub struct ProgressStartBody {
        progress_id: String,
        title: String,
        ;
        request_id: i64,
        cancellable: bool,
        message: String,
        percentage: f64,
    }
}

object! {
    pub struct ProgressUpdateBody {
        progress_id: String,
        ;
        message: String,
        percentage: f64,
    }
}

object! {
    pub struct ProgressEndBody {
        progress_id: String,
        ;
        message: String,
    }
}

// Reverse request arguments and bodies.

object! {
    pub struct RunInTerminalArguments {
        cwd: String,
        args: Vec<String>,
        ;
        /// `"integrated"` or `"external"`.
        kind: String,
        title: String,
        /// A `null` value removes the variable.
        env: BTreeMap<String, Option<String>>,
        args_can_be_interpreted_by_shell: bool,
    }
}

object! {
    pub struct RunInTerminalBody {
        ;
        process_id: i64,
        shell_process_id: i64,
    }
}

object! {
    pub struct StartDebuggingArguments {
        configuration: Object,
        /// `"launch"` or `"attach"`.
        request: String,
    }
}

/// Declares marker types for requests or events.
macro_rules! markers {
    (request: $( $(#[$meta:meta])* $name:ident = $command:literal ($args:ty) -> $body:ty; )*) => {
        $(
            $(#[$meta])*
            #[derive(Clone, Copy, Debug)]
            pub enum $name {}

            impl $crate::protocol::Request for $name {
                const COMMAND: &'static str = $command;
                type Arguments = $args;
                type Body = $body;
            }
        )*
    };
    (event: $( $(#[$meta:meta])* $name:ident = $event:literal ($body:ty); )*) => {
        $(
            $(#[$meta])*
            #[derive(Clone, Copy, Debug)]
            pub enum $name {}

            impl $crate::protocol::Event for $name {
                const EVENT: &'static str = $event;
                type Body = $body;
            }
        )*
    };
}

/// The requests the client sends.
pub mod request {
    use super::*;

    markers! { request:
        Initialize = "initialize" (InitializeArguments) -> Capabilities;
        /// Its arguments are the adapter's own, from the launch configuration.
        Launch = "launch" (Object) -> Empty;
        Attach = "attach" (Object) -> Empty;
        ConfigurationDone = "configurationDone" (Empty) -> Empty;
        SetBreakpoints = "setBreakpoints" (SetBreakpointsArguments) -> BreakpointsBody;
        SetFunctionBreakpoints = "setFunctionBreakpoints" (SetFunctionBreakpointsArguments) -> BreakpointsBody;
        SetExceptionBreakpoints = "setExceptionBreakpoints" (SetExceptionBreakpointsArguments) -> SetExceptionBreakpointsBody;
        Threads = "threads" (Empty) -> ThreadsBody;
        StackTrace = "stackTrace" (StackTraceArguments) -> StackTraceBody;
        Scopes = "scopes" (ScopesArguments) -> ScopesBody;
        Variables = "variables" (VariablesArguments) -> VariablesBody;
        Continue = "continue" (ContinueArguments) -> ContinueBody;
        Next = "next" (StepArguments) -> Empty;
        StepIn = "stepIn" (StepInArguments) -> Empty;
        StepOut = "stepOut" (StepArguments) -> Empty;
        Pause = "pause" (PauseArguments) -> Empty;
        Evaluate = "evaluate" (EvaluateArguments) -> EvaluateBody;
        Terminate = "terminate" (TerminateArguments) -> Empty;
        Disconnect = "disconnect" (DisconnectArguments) -> Empty;
        Restart = "restart" (RestartArguments) -> Empty;
        SourceRequest = "source" (SourceArguments) -> SourceBody;
        Cancel = "cancel" (CancelArguments) -> Empty;
        Completions = "completions" (CompletionsArguments) -> CompletionsBody;
        SetVariable = "setVariable" (SetVariableArguments) -> SetVariableBody;
        Goto = "goto" (GotoArguments) -> Empty;
        GotoTargets = "gotoTargets" (GotoTargetsArguments) -> GotoTargetsBody;
        RestartFrame = "restartFrame" (RestartFrameArguments) -> Empty;
    }
}

/// The events an adapter sends. `module`, `loadedSource` and `memory` have
/// no types: the client logs and ignores them.
pub mod event {
    use super::*;

    markers! { event:
        Initialized = "initialized" (Empty);
        Stopped = "stopped" (StoppedBody);
        Continued = "continued" (ContinuedBody);
        Exited = "exited" (ExitedBody);
        Terminated = "terminated" (TerminatedBody);
        ThreadEvent = "thread" (ThreadBody);
        Output = "output" (OutputBody);
        BreakpointEvent = "breakpoint" (BreakpointBody);
        CapabilitiesEvent = "capabilities" (CapabilitiesBody);
        Invalidated = "invalidated" (InvalidatedBody);
        Process = "process" (ProcessBody);
        ProgressStart = "progressStart" (ProgressStartBody);
        ProgressUpdate = "progressUpdate" (ProgressUpdateBody);
        ProgressEnd = "progressEnd" (ProgressEndBody);
    }
}

/// The requests an adapter sends the client.
pub mod reverse {
    use super::*;

    markers! { request:
        RunInTerminal = "runInTerminal" (RunInTerminalArguments) -> RunInTerminalBody;
        /// Refused in v1: child sessions aren't supported (§2).
        StartDebugging = "startDebugging" (StartDebuggingArguments) -> Empty;
    }
}
