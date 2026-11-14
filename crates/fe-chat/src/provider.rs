//! The provider seam: what a chat sends ([`ChatRequest`]), what streams back
//! ([`ChatEvent`]) and how it fails ([`ProviderError`]). The record's § 2.

use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use futures::Stream;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

/// A model behind a streaming chat API.
pub trait ChatProvider: Send + Sync {
    /// The provider's name, such as `"anthropic"`.
    fn name(&self) -> &str;

    /// Sends `request` and streams the reply. Dropping the stream cancels:
    /// it closes the connection, which stops generation server-side. There
    /// is no separate cancel call.
    fn stream(&self, request: ChatRequest) -> ChatStream;
}

/// A reply as it streams.
pub type ChatStream = Pin<Box<dyn Stream<Item = ChatEvent> + Send>>;

/// One request to the model: the whole prompt, assembled by
/// [`build_request`](crate::build_request).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub system: Vec<SystemBlock>,
    /// The chat's tool set, frozen at its first send and sent the same in
    /// every request after (a withdrawal is a `tool_removal` message, never
    /// a change here). Empty for a chat without tools.
    pub tools: Vec<ToolSpec>,
    /// The history and the new user message, with an effort-only system
    /// message ([`Role::System`]) before each turn whose effort changed on a
    /// model that takes one.
    pub messages: Vec<ApiMessage>,
    pub max_tokens: u32,
    /// The top-level effort: on a model that takes effort messages, the
    /// chat's first effort, so the cached prefix holds; on any other, the
    /// effort asked for now.
    pub effort: Effort,
    /// Whether the reply's thinking is shown (summarized) rather than omitted.
    pub show_thinking: bool,
    /// The chat's task budget in tokens (`output_config.task_budget`, beta
    /// `task-budgets-2026-03-13`), frozen with its tools; `None` when off.
    pub task_budget: Option<u32>,
}

/// A `text` block of the system prompt, serialised as the API takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemBlock {
    pub text: String,
    pub cache_control: Option<CacheControl>,
}

impl Serialize for SystemBlock {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let fields = if self.cache_control.is_some() { 3 } else { 2 };
        let mut block = serializer.serialize_struct("SystemBlock", fields)?;
        block.serialize_field("type", "text")?;
        block.serialize_field("text", &self.text)?;
        if let Some(cache_control) = &self.cache_control {
            block.serialize_field("cache_control", cache_control)?;
        }
        block.end()
    }
}

/// A prompt-cache breakpoint: `{"type": "ephemeral"}`, the 5-minute TTL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CacheControl {
    Ephemeral,
}

/// Who a message is from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    /// An effort-only system message (`content: []` with
    /// `output_config.effort`, beta `mid-conversation-output-config-2026-07-01`):
    /// the effort from the next user message on, without the messages-cache
    /// reset a top-level change makes. Its content is empty. See
    /// [`ApiMessage::effort`].
    System {
        effort: Effort,
    },
}

/// One message of the API history: its content blocks, verbatim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: Role,
    pub content: Vec<Value>,
}

impl ApiMessage {
    /// The effort-only system message setting `effort` for the turns from
    /// the next user message on.
    pub fn effort(effort: Effort) -> ApiMessage {
        ApiMessage {
            role: Role::System { effort },
            content: Vec::new(),
        }
    }

    /// The effort an effort-only system message sets; `None` for any other
    /// message, a [`tool_removal`](ApiMessage::tool_removal) one included.
    pub fn effort_change(&self) -> Option<Effort> {
        match self.role {
            Role::System { effort } if self.content.is_empty() => Some(effort),
            Role::User | Role::Assistant | Role::System { .. } => None,
        }
    }

    /// The system message withdrawing the tools `names`: one `tool_removal`
    /// block per tool, naming it by reference (beta
    /// `mid-conversation-tool-changes-2026-07-01`). It must follow a user
    /// message and be last or before an assistant message.
    ///
    /// Its role carries `in_effect`, the effort already in effect, so it
    /// changes none: it isn't an effort-only message, and the wire leaves
    /// its effort out.
    pub fn tool_removal<'a>(
        names: impl IntoIterator<Item = &'a str>,
        in_effect: Effort,
    ) -> ApiMessage {
        let content = names
            .into_iter()
            .map(|name| {
                let mut tool = Map::new();
                tool.insert("name".to_owned(), Value::String(name.to_owned()));
                tool.insert("type".to_owned(), "tool_reference".into());
                let mut block = Map::new();
                block.insert("tool".to_owned(), Value::Object(tool));
                block.insert("type".to_owned(), "tool_removal".into());
                Value::Object(block)
            })
            .collect();
        ApiMessage {
            role: Role::System { effort: in_effect },
            content,
        }
    }

    /// The tools a [`tool_removal`](ApiMessage::tool_removal) message
    /// withdraws; empty for any other message.
    pub fn removed_tools(&self) -> Vec<&str> {
        if !matches!(self.role, Role::System { .. }) {
            return Vec::new();
        }
        self.content
            .iter()
            .filter(|block| block["type"] == "tool_removal")
            .filter_map(|block| block["tool"]["name"].as_str())
            .collect()
    }
}

/// How hard the model thinks (`output_config.effort`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    /// Claude Opus 5.5's own default.
    #[default]
    Medium,
    High,
    XHigh,
    Max,
}

/// What streams back for one request.
#[derive(Clone, Debug, PartialEq)]
pub enum ChatEvent {
    /// The reply began; `model` is the model that serves it, which differs
    /// from the one asked for when a server-side fallback ran.
    Started {
        model: String,
        message_id: String,
    },
    BlockStarted {
        index: usize,
        kind: BlockKind,
    },
    TextDelta {
        index: usize,
        text: String,
    },
    ThinkingDelta {
        index: usize,
        text: String,
    },
    ToolInputDelta {
        index: usize,
        partial_json: String,
    },
    /// How a `tool_use` block's input parsed, sent just before its
    /// [`ChatEvent::BlockFinished`]. Without it, a string input is taken as
    /// [`InputStatus::Unparsed`] and anything else as parsed; an object
    /// input sends none.
    ToolInputParsed {
        index: usize,
        status: InputStatus,
    },
    /// The whole block, verbatim, as the API would echo it back.
    BlockFinished {
        index: usize,
        block: Value,
    },
    Usage(Usage),
    Finished {
        stop_reason: StopReason,
        stop_details: Option<Value>,
    },
    Failed(ProviderError),
}

/// The kind of a reply's content block.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Text,
    Thinking,
    RedactedThinking,
    ToolUse {
        id: String,
        name: String,
    },
    /// A server-side fallback ran here, from one model to another.
    Fallback {
        from: String,
        to: String,
    },
    /// A block type this crate doesn't know, kept rather than an error.
    Other(String),
}

impl BlockKind {
    /// The kind of a block the API sent, by its `type`.
    pub fn of(block: &Value) -> BlockKind {
        let field = |name: &str| block[name].as_str().unwrap_or_default().to_owned();
        match block["type"].as_str().unwrap_or_default() {
            "text" => BlockKind::Text,
            "thinking" => BlockKind::Thinking,
            "redacted_thinking" => BlockKind::RedactedThinking,
            "tool_use" => BlockKind::ToolUse {
                id: field("id"),
                name: field("name"),
            },
            "fallback" => BlockKind::Fallback {
                from: field("from"),
                to: field("to"),
            },
            other => BlockKind::Other(other.to_owned()),
        }
    }
}

/// Why the model stopped.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    PauseTurn,
    Refusal,
    Other(String),
}

impl StopReason {
    /// The API's name for it, such as `"end_turn"`.
    pub fn as_str(&self) -> &str {
        match self {
            StopReason::EndTurn => "end_turn",
            StopReason::MaxTokens => "max_tokens",
            StopReason::StopSequence => "stop_sequence",
            StopReason::ToolUse => "tool_use",
            StopReason::PauseTurn => "pause_turn",
            StopReason::Refusal => "refusal",
            StopReason::Other(other) => other,
        }
    }
}

impl From<&str> for StopReason {
    fn from(name: &str) -> StopReason {
        match name {
            "end_turn" => StopReason::EndTurn,
            "max_tokens" => StopReason::MaxTokens,
            "stop_sequence" => StopReason::StopSequence,
            "tool_use" => StopReason::ToolUse,
            "pause_turn" => StopReason::PauseTurn,
            "refusal" => StopReason::Refusal,
            other => StopReason::Other(other.to_owned()),
        }
    }
}

impl From<String> for StopReason {
    fn from(name: String) -> StopReason {
        StopReason::from(name.as_str())
    }
}

impl From<StopReason> for String {
    fn from(reason: StopReason) -> String {
        reason.as_str().to_owned()
    }
}

/// Token counts for a reply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl Usage {
    /// The whole prompt's size: uncached input plus what was written to and
    /// read from the cache. What the context meter measures.
    pub fn total_input(&self) -> u64 {
        self.input_tokens + self.cache_creation_input_tokens + self.cache_read_input_tokens
    }
}

/// Why a request failed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    pub kind: ErrorKind,
    pub message: String,
    /// How long the server asked to wait (`retry-after`).
    pub retry_after: Option<Duration>,
}

impl ProviderError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> ProviderError {
        ProviderError {
            kind,
            message: message.into(),
            retry_after: None,
        }
    }

    /// Whether the request may be sent again.
    pub fn retryable(&self) -> bool {
        self.kind.retryable()
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ProviderError {}

/// The kinds of [`ProviderError`], by HTTP status where there is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The connection failed before the stream started.
    Network,
    /// 429.
    RateLimited,
    /// 529.
    Overloaded,
    /// 500.
    Server,
    /// 401.
    Auth,
    /// 402.
    Billing,
    /// 403.
    Permission,
    /// 404.
    NotFound,
    /// 413.
    TooLarge,
    /// 400.
    InvalidRequest,
    /// An SSE `error` event, or the connection cut after the stream started.
    Stream,
}

impl ErrorKind {
    /// True for network errors, 429, 529 and 500.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::Network | ErrorKind::RateLimited | ErrorKind::Overloaded | ErrorKind::Server
        )
    }
}

/// A client tool the model may call, for the tool loop
/// ([`tool_set`](crate::tool_set) gives the chat's five).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// The JSON schema of the tool's input.
    pub input_schema: Value,
    /// Whether the model's input must match the schema exactly.
    pub strict: bool,
    /// Whether the input streams as it's generated, unbuffered and
    /// unvalidated by the API, so it's validated here at the block's end.
    #[serde(default)]
    pub eager_input_streaming: bool,
}

/// Whether the model may call tools. Forced choices (`any`, `tool`) are a
/// 400 on Claude Opus 5.5, so they aren't offered: steer by prompt and
/// `strict`, and check a call was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolChoice {
    #[default]
    Auto,
    None,
}

/// How a finished `tool_use` block's streamed input parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputStatus {
    /// Valid JSON, an object.
    Object,
    /// Valid JSON, but not an object (a string, array, ...): finished, and
    /// invalid as a call's input.
    NonObject,
    /// Not valid JSON, as when the input was cut short; the block keeps
    /// the raw string.
    Unparsed,
}

/// A tool call the model made: a finished `tool_use` block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}
