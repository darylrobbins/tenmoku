//! The provider seam: what a chat sends ([`ChatRequest`]), what streams back
//! ([`ChatEvent`]) and how it fails ([`ProviderError`]). The record's § 2.

use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use futures::Stream;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

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
    /// Empty in v1.
    pub tools: Vec<ToolSpec>,
    pub messages: Vec<ApiMessage>,
    pub max_tokens: u32,
    pub effort: Effort,
    /// Whether the reply's thinking is shown (summarized) rather than omitted.
    pub show_thinking: bool,
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
}

/// One message of the API history: its content blocks, verbatim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiMessage {
    pub role: Role,
    pub content: Vec<Value>,
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

/// A client tool the model may call, for the agent loop (v1 sends none).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// The JSON schema of the tool's input.
    pub input_schema: Value,
    /// Whether the model's input must match the schema exactly.
    pub strict: bool,
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

/// A tool call the model made: a finished `tool_use` block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}
