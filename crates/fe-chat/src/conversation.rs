//! A chat: its turns, the reducer that builds a streaming reply from
//! [`ChatEvent`]s, and the API history the next request is built from. The
//! record's § 4.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::provider::{
    ApiMessage, BlockKind, ChatEvent, Effort, ProviderError, Role, StopReason, Usage,
};
use crate::request::ChatSettings;

/// How many chars of the first user line make the title.
const TITLE_CHARS: usize = 60;

/// A chat's id: 128 random bits, printed as 26 lowercase Crockford base32
/// chars. It names the chat's file and the AI's undo session.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChatId(u128);

const BASE32: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

impl ChatId {
    /// A new random id.
    pub fn new() -> ChatId {
        ChatId(u128::from(random_u64()) << 64 | u128::from(random_u64()))
    }

    pub fn from_u128(bits: u128) -> ChatId {
        ChatId(bits)
    }

    pub fn as_u128(self) -> u128 {
        self.0
    }
}

impl Default for ChatId {
    fn default() -> ChatId {
        ChatId::new()
    }
}

/// 64 bits from std's randomly keyed SipHash, which is enough to keep chat
/// ids apart without a `rand` dependency.
fn random_u64() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    hasher.write_u128(now.as_nanos());
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.finish()
}

impl fmt::Display for ChatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 26 chars of 5 bits hold 130: the first char has the top 3 bits.
        let mut chars = [0u8; 26];
        for (i, char) in chars.iter_mut().enumerate() {
            let shift = 5 * (25 - i);
            *char = BASE32[(self.0 >> shift & 0x1f) as usize];
        }
        f.write_str(std::str::from_utf8(&chars).expect("base32 is ASCII"))
    }
}

impl fmt::Debug for ChatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChatId({self})")
    }
}

/// Why a string isn't a [`ChatId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseChatIdError;

impl fmt::Display for ParseChatIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a chat id is 26 Crockford base32 chars")
    }
}

impl std::error::Error for ParseChatIdError {}

impl FromStr for ChatId {
    type Err = ParseChatIdError;

    fn from_str(text: &str) -> Result<ChatId, ParseChatIdError> {
        if text.len() != 26 {
            return Err(ParseChatIdError);
        }
        let mut bits: u128 = 0;
        for (i, byte) in text.bytes().enumerate() {
            let digit = BASE32
                .iter()
                .position(|&c| c == byte.to_ascii_lowercase())
                .ok_or(ParseChatIdError)? as u128;
            if i == 0 && digit > 0b111 {
                return Err(ParseChatIdError);
            }
            bits = bits << 5 | digit;
        }
        Ok(ChatId(bits))
    }
}

impl Serialize for ChatId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ChatId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<ChatId, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// A chat with the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ChatId,
    /// The first user line, cut to 60 chars; empty before the first turn.
    pub title: String,
    /// The model asked for, fixed for the chat: a switch would empty the
    /// prompt cache and drop the thinking blocks.
    pub model: String,
    /// Fixed for the chat: a change empties the messages cache.
    pub effort: Effort,
    /// Fixed for the chat: a change empties the messages cache.
    pub show_thinking: bool,
    pub turns: Vec<Turn>,
}

/// One exchange: what the user sent and the reply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    /// The user message's content blocks, rendered once at send and never
    /// again.
    pub user_content: Vec<Value>,
    /// What was attached, for the UI; the content is in `user_content`.
    pub attachments: Vec<Attachment>,
    /// The reply's blocks by index, as they stream.
    pub reply: Vec<ReplyBlock>,
    /// The model that served the reply.
    pub model: Option<String>,
    pub stop_reason: Option<StopReason>,
    pub stop_details: Option<Value>,
    pub usage: Option<Usage>,
    pub state: TurnState,
}

/// One block of a reply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReplyBlock {
    /// The block's index in the reply.
    pub index: usize,
    pub kind: BlockKind,
    /// What has streamed into it, for the UI: text, thinking, or a tool
    /// call's partial input JSON.
    pub text: String,
    /// The whole block, verbatim, once it finished.
    pub block: Option<Value>,
}

/// Where a turn is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SavedTurnState", into = "SavedTurnState")]
pub enum TurnState {
    Streaming,
    Done,
    /// The user stopped it.
    Stopped,
    Refused,
    Failed(ProviderError),
}

/// [`TurnState`] as saved: a reply can't still be streaming once read back,
/// so a streaming turn is saved as stopped.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SavedTurnState {
    Done,
    Stopped,
    Refused,
    Failed(ProviderError),
}

impl From<TurnState> for SavedTurnState {
    fn from(state: TurnState) -> SavedTurnState {
        match state {
            TurnState::Streaming | TurnState::Stopped => SavedTurnState::Stopped,
            TurnState::Done => SavedTurnState::Done,
            TurnState::Refused => SavedTurnState::Refused,
            TurnState::Failed(error) => SavedTurnState::Failed(error),
        }
    }
}

impl From<SavedTurnState> for TurnState {
    fn from(state: SavedTurnState) -> TurnState {
        match state {
            SavedTurnState::Done => TurnState::Done,
            SavedTurnState::Stopped => TurnState::Stopped,
            SavedTurnState::Refused => TurnState::Refused,
            SavedTurnState::Failed(error) => TurnState::Failed(error),
        }
    }
}

/// What an attachment is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AttachmentKind {
    File,
    Selection,
    Diagnostics,
    OpenEditors,
}

/// 1-based rows, both ends included.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rows {
    pub first: u32,
    pub last: u32,
}

impl fmt::Display for Rows {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.first, self.last)
    }
}

/// An attachment's metadata, for the transcript's chips.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub kind: AttachmentKind,
    pub label: String,
    pub path: Option<String>,
    pub rows: Option<Rows>,
    /// Its token estimate.
    pub estimate: u32,
}

impl Conversation {
    /// A new, empty chat, its model, effort and thinking fixed from
    /// `settings`.
    pub fn new(settings: &ChatSettings) -> Conversation {
        Conversation {
            id: ChatId::new(),
            title: String::new(),
            model: settings.model.clone(),
            effort: settings.effort,
            show_thinking: settings.show_thinking,
            turns: Vec::new(),
        }
    }

    /// Starts a turn with the user message as sent. A turn still streaming
    /// is stopped first.
    pub fn begin_turn(&mut self, user_content: Vec<Value>, attachments: Vec<Attachment>) {
        self.stop();
        if self.title.is_empty() {
            self.title = title_of(&user_content);
        }
        self.turns.push(Turn {
            user_content,
            attachments,
            reply: Vec::new(),
            model: None,
            stop_reason: None,
            stop_details: None,
            usage: None,
            state: TurnState::Streaming,
        });
    }

    /// The turn being streamed into, if any.
    pub fn streaming_turn(&self) -> Option<&Turn> {
        self.turns
            .last()
            .filter(|turn| turn.state == TurnState::Streaming)
    }

    fn streaming_turn_mut(&mut self) -> Option<&mut Turn> {
        self.turns
            .last_mut()
            .filter(|turn| turn.state == TurnState::Streaming)
    }

    /// Applies one event to the streaming turn; ignored when no turn is
    /// streaming (a stopped turn's stream may still deliver).
    pub fn apply(&mut self, event: ChatEvent) {
        let Some(turn) = self.streaming_turn_mut() else {
            return;
        };
        match event {
            ChatEvent::Started { model, .. } => turn.model = Some(model),
            ChatEvent::BlockStarted { index, kind } => {
                let started = kind.clone();
                turn.block_mut(index, || started).kind = kind;
            }
            ChatEvent::TextDelta { index, text } => {
                turn.block_mut(index, || BlockKind::Text)
                    .text
                    .push_str(&text);
            }
            ChatEvent::ThinkingDelta { index, text } => {
                turn.block_mut(index, || BlockKind::Thinking)
                    .text
                    .push_str(&text);
            }
            ChatEvent::ToolInputDelta {
                index,
                partial_json,
            } => {
                let kind = || BlockKind::ToolUse {
                    id: String::new(),
                    name: String::new(),
                };
                turn.block_mut(index, kind).text.push_str(&partial_json);
            }
            ChatEvent::BlockFinished { index, block } => {
                let kind = BlockKind::of(&block);
                turn.block_mut(index, || kind).block = Some(block);
            }
            ChatEvent::Usage(usage) => turn.usage = Some(usage),
            ChatEvent::Finished {
                stop_reason,
                stop_details,
            } => {
                turn.state = if stop_reason == StopReason::Refusal {
                    TurnState::Refused
                } else {
                    TurnState::Done
                };
                turn.stop_reason = Some(stop_reason);
                turn.stop_details = stop_details;
            }
            ChatEvent::Failed(error) => turn.state = TurnState::Failed(error),
        }
    }

    /// Marks the streaming turn stopped, if there is one.
    pub fn stop(&mut self) {
        if let Some(turn) = self.streaming_turn_mut() {
            turn.state = TurnState::Stopped;
        }
    }

    /// The API history: every done turn's user message and reply, passed
    /// back unchanged, so each request's prefix is the last one's.
    ///
    /// - A done turn: its user content and its reply's finished blocks,
    ///   thinking included; after a mid-output fallback, without the
    ///   `thinking`, `redacted_thinking` and `tool_use` blocks before the
    ///   last `fallback` block.
    /// - A stopped turn: its user content and its finished `text` blocks
    ///   only; left out when it has none.
    /// - Refused, failed and streaming turns: left out (the partial is
    ///   discarded).
    ///
    /// A done turn with no finished blocks is left out too, since an empty
    /// assistant message is a 400.
    pub fn history(&self) -> Vec<ApiMessage> {
        let mut messages = Vec::new();
        for turn in &self.turns {
            let reply = match turn.state {
                TurnState::Done => echoed(turn.reply_blocks()),
                TurnState::Stopped => turn
                    .reply_blocks()
                    .filter(|block| block["type"] == "text")
                    .cloned()
                    .collect(),
                TurnState::Streaming | TurnState::Refused | TurnState::Failed(_) => continue,
            };
            if reply.is_empty() {
                continue;
            }
            messages.push(ApiMessage {
                role: Role::User,
                content: turn.user_content.clone(),
            });
            messages.push(ApiMessage {
                role: Role::Assistant,
                content: reply,
            });
        }
        messages
    }
}

/// A done reply's blocks as they go back: after a mid-output fallback, the
/// thinking, redacted thinking and tool calls before the last `fallback`
/// block are left out, as the fallback rules require.
fn echoed<'a>(blocks: impl Iterator<Item = &'a Value>) -> Vec<Value> {
    let blocks: Vec<&Value> = blocks.collect();
    let last_fallback = blocks.iter().rposition(|block| block["type"] == "fallback");
    blocks
        .into_iter()
        .enumerate()
        .filter(|(i, block)| {
            last_fallback.is_none_or(|fallback| {
                *i > fallback
                    || !matches!(
                        block["type"].as_str(),
                        Some("thinking" | "redacted_thinking" | "tool_use")
                    )
            })
        })
        .map(|(_, block)| block.clone())
        .collect()
}

/// The first line of the user's question (the last `text` block), cut to
/// 60 chars.
fn title_of(user_content: &[Value]) -> String {
    let question = user_content
        .iter()
        .rev()
        .find(|block| block["type"] == "text")
        .and_then(|block| block["text"].as_str())
        .unwrap_or_default();
    let line = question
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    line.chars().take(TITLE_CHARS).collect()
}

impl Turn {
    /// The block at `index`, started as `kind()` if it wasn't yet, kept in
    /// index order.
    fn block_mut(&mut self, index: usize, kind: impl FnOnce() -> BlockKind) -> &mut ReplyBlock {
        let at = match self.reply.binary_search_by_key(&index, |block| block.index) {
            Ok(at) => at,
            Err(at) => {
                let block = ReplyBlock {
                    index,
                    kind: kind(),
                    text: String::new(),
                    block: None,
                };
                self.reply.insert(at, block);
                at
            }
        };
        &mut self.reply[at]
    }

    /// The reply's finished blocks, verbatim, in order.
    pub fn reply_blocks(&self) -> impl Iterator<Item = &Value> {
        self.reply.iter().filter_map(|block| block.block.as_ref())
    }

    /// The reply's text as it streamed: every text block's deltas, joined.
    pub fn text(&self) -> String {
        self.streamed(|kind| *kind == BlockKind::Text)
    }

    /// The reply's thinking as it streamed.
    pub fn thinking(&self) -> String {
        self.streamed(|kind| *kind == BlockKind::Thinking)
    }

    fn streamed(&self, wanted: impl Fn(&BlockKind) -> bool) -> String {
        self.reply
            .iter()
            .filter(|block| wanted(&block.kind))
            .map(|block| block.text.as_str())
            .collect()
    }

    /// Whether the reply was cut off at the token limit.
    pub fn cut_off(&self) -> bool {
        self.stop_reason == Some(StopReason::MaxTokens)
    }
}
