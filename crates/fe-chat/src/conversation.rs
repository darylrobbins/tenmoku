//! A chat: its turns, the reducer that builds a streaming reply from
//! [`ChatEvent`]s, and the API history the next request is built from. The
//! record's § 4, and the agents record's § 6 for the tool loop: a turn is
//! the user message and one or more rounds, each a reply and, when it
//! called tools, one message of their results.

use kiln_l10n::tr;
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::provider::{
    ApiMessage, BlockKind, ChatEvent, Effort, ErrorKind, InputStatus, ProviderError, Role,
    StopReason, ToolCall, ToolSpec, Usage,
};
use crate::request::ChatSettings;
use crate::tools::{self, MIN_TASK_BUDGET, NotRun};

/// How many chars of the first user line make the title.
const TITLE_CHARS: usize = 60;

/// The detail of a round that failed because its tool input was invalid
/// twice, as a provider error's detail is: the API's own English text.
const INVALID_TOOL_INPUT: &str = "the model's tool input wasn't valid";

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
        f.write_str(&tr!("chat-id-invalid"))
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
    /// The model the next turn asks for. Picked from the chat's header; a
    /// switch applies from the next send, earlier turns going back
    /// unchanged (the first request after it misses the cache, which is per
    /// model).
    pub model: String,
    /// The effort the next turn asks for, picked like `model`. On a model
    /// that takes effort messages a change goes through one (see
    /// [`build_request`](crate::build_request)) and keeps the cache.
    pub effort: Effort,
    /// Fixed for the chat: a change empties the messages cache.
    pub show_thinking: bool,
    pub turns: Vec<Turn>,
    /// The chat's tools, sorted by name and frozen at its first send (see
    /// [`Conversation::set_tools`]): every request sends the same. Empty
    /// for a chat started without tools, which stays without them.
    #[serde(default)]
    pub tools: Vec<ToolSpec>,
    /// The chat's task budget in tokens, frozen with its tools; `None` when
    /// off.
    #[serde(default)]
    pub task_budget: Option<u32>,
    /// Whether the tools were withdrawn (the folder's trust was, or its
    /// root changed). The next turn sent carries the `tool_removal`
    /// message; after it the chat has no tools, though `tools` is still
    /// sent unchanged.
    #[serde(default)]
    pub tools_withdrawn: bool,
}

/// One exchange: what the user sent and the reply, over one or more
/// rounds.
///
/// Each round is one request. The rounds whose replies called tools and
/// were answered are [`rounds`](Turn::rounds), committed: their replies and
/// results go back byte for byte in every later request. The last round's
/// reply is [`reply`](Turn::reply), as it streams and as the turn ended. A
/// chat saved before rounds loads with no committed rounds and its one
/// reply there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    /// The user message's content blocks, rendered once at send and never
    /// again.
    pub user_content: Vec<Value>,
    /// What was attached, for the UI; the content is in `user_content`.
    pub attachments: Vec<Attachment>,
    /// The committed rounds, in order. Each has at least one call and
    /// exactly one result per call.
    #[serde(default)]
    pub rounds: Vec<Round>,
    /// The last round's reply: its blocks by index, as they stream.
    pub reply: Vec<ReplyBlock>,
    /// The results of the last round's calls as they finish, in any order,
    /// until the round is committed: the pending result message.
    #[serde(default)]
    pub results: Vec<ToolResult>,
    /// Whether the `tool_removal` message follows this turn's user message:
    /// it is the first turn sent after the tools were withdrawn.
    #[serde(default)]
    pub removes_tools: bool,
    /// The request settings the turn's rounds are sent with, recorded by
    /// [`Conversation::begin_turn_with`] (or, for a turn begun without
    /// them, its first [`round_request`](crate::round_request)), so a
    /// resend or a Retry is the same request whatever the settings are now.
    #[serde(default)]
    pub sent_with: Option<RoundSettings>,
    /// The model that served the reply.
    pub model: Option<String>,
    /// The model the turn asked for; `None` in chats saved before the
    /// picker, whose turns all asked the chat's `model`.
    #[serde(default)]
    pub asked_model: Option<String>,
    /// The effort the turn asked for; `None` in chats saved before the
    /// picker, whose turns all asked the chat's `effort`. The request
    /// replays it, so a reloaded chat sends the same messages.
    #[serde(default)]
    pub asked_effort: Option<Effort>,
    /// How the last round's reply ended.
    pub stop_reason: Option<StopReason>,
    pub stop_details: Option<Value>,
    /// The last round's usage.
    pub usage: Option<Usage>,
    pub state: TurnState,
    /// Whether the last round's reply had invalid tool input and its
    /// request was sent again, which happens once a round.
    #[serde(skip)]
    invalid_resent: bool,
    /// Whether the last round's reply was dropped for invalid tool input
    /// and its request must be sent again, until the next event.
    #[serde(skip)]
    resend: bool,
}

/// What a turn's round requests take from the chat settings, kept for
/// the turn's life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundSettings {
    pub max_tokens: u32,
    /// Whether effort changes go through effort-only messages.
    pub effort_messages: bool,
}

impl RoundSettings {
    /// What `settings` give a request asking for `model`.
    pub fn new(model: &str, settings: &ChatSettings) -> RoundSettings {
        RoundSettings {
            max_tokens: settings.max_tokens,
            effort_messages: settings.effort_message_models.contains(&model),
        }
    }
}

/// A committed round: a reply that called tools, and its results, one per
/// call, in the calls' order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Round {
    pub reply: Vec<ReplyBlock>,
    pub results: Vec<ToolResult>,
}

/// The result of one tool call, as a `tool_result` block answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    /// The `tool_use` block's id.
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    /// A call's result.
    pub fn ok(tool_use_id: impl Into<String>, content: impl Into<String>) -> ToolResult {
        ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: false,
        }
    }

    /// A call's failure, denial or refusal: `is_error: true`.
    pub fn error(tool_use_id: impl Into<String>, content: impl Into<String>) -> ToolResult {
        ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error: true,
        }
    }

    /// The answer to a call that wasn't run, for `why`.
    pub fn not_run(tool_use_id: impl Into<String>, why: NotRun) -> ToolResult {
        ToolResult::error(tool_use_id, why.text())
    }

    /// The `tool_result` block, its keys inserted in sorted order.
    pub fn block(&self) -> Value {
        let mut block = Map::new();
        block.insert("content".to_owned(), Value::String(self.content.clone()));
        block.insert("is_error".to_owned(), Value::Bool(self.is_error));
        block.insert(
            "tool_use_id".to_owned(),
            Value::String(self.tool_use_id.clone()),
        );
        block.insert("type".to_owned(), Value::String("tool_result".to_owned()));
        Value::Object(block)
    }
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
    /// How a finished call's input parsed, when the provider said.
    #[serde(default)]
    pub input_status: Option<InputStatus>,
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
    /// A new, empty chat, its model, effort and thinking from `settings`,
    /// without tools.
    pub fn new(settings: &ChatSettings) -> Conversation {
        Conversation {
            id: ChatId::new(),
            title: String::new(),
            model: settings.model.clone(),
            effort: settings.effort,
            show_thinking: settings.show_thinking,
            turns: Vec::new(),
            tools: Vec::new(),
            task_budget: None,
            tools_withdrawn: false,
        }
    }

    /// Declares the chat's tools, sorted by name, and its task budget in
    /// tokens (0 for none; a budget under [`MIN_TASK_BUDGET`] is raised to
    /// it). Only before the first send: once a turn exists they're frozen,
    /// and this changes nothing and returns false.
    pub fn set_tools(&mut self, mut tools: Vec<ToolSpec>, task_budget: u32) -> bool {
        if !self.turns.is_empty() {
            return false;
        }
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        self.tools = tools;
        self.task_budget = (task_budget > 0).then(|| task_budget.max(MIN_TASK_BUDGET));
        self.tools_withdrawn = false;
        true
    }

    /// Whether the model may call tools now: the chat declared some and
    /// they weren't withdrawn.
    pub fn has_tools(&self) -> bool {
        !self.tools.is_empty() && !self.tools_withdrawn
    }

    /// Picks the model for the turns from the next send on. Nothing earlier
    /// changes.
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.settle_asked();
        self.model = model.into();
    }

    /// Picks the effort for the turns from the next send on. Nothing
    /// earlier changes.
    pub fn set_effort(&mut self, effort: Effort) {
        self.settle_asked();
        self.effort = effort;
    }

    /// Writes what each turn of a chat saved before the picker asked for,
    /// the chat's model and effort, into the turn, before they change.
    fn settle_asked(&mut self) {
        for turn in &mut self.turns {
            turn.asked_model.get_or_insert_with(|| self.model.clone());
            turn.asked_effort.get_or_insert(self.effort);
        }
    }

    /// The model `turn` asked for.
    pub fn asked_model<'a>(&'a self, turn: &'a Turn) -> &'a str {
        turn.asked_model.as_deref().unwrap_or(&self.model)
    }

    /// The effort `turn` asked for.
    pub fn asked_effort(&self, turn: &Turn) -> Effort {
        turn.asked_effort.unwrap_or(self.effort)
    }

    /// The effort the chat's first turn asked for, or its effort now before
    /// one: the top-level effort while effort messages carry the changes.
    pub fn first_effort(&self) -> Effort {
        self.turns
            .first()
            .map_or(self.effort, |turn| self.asked_effort(turn))
    }

    /// Starts a turn with the user message as sent, asking for the chat's
    /// model and effort now. A turn still streaming is stopped first (see
    /// [`Conversation::stop`]), so stop it before building the new turn's
    /// request.
    ///
    /// A chat with tools begins its turns with [`Conversation::begin_turn_with`]
    /// instead, which records the request settings for its rounds.
    pub fn begin_turn(&mut self, user_content: Vec<Value>, attachments: Vec<Attachment>) {
        self.start_turn(user_content, attachments, None);
    }

    /// [`Conversation::begin_turn`], recording the request settings the
    /// turn's first request was built with (by [`build_request`] or
    /// [`round_request`], from the same `settings`): every round of the
    /// turn, a resend or a Retry of its first round included, is sent with
    /// them, whatever the settings are by then.
    ///
    /// [`build_request`]: crate::build_request
    /// [`round_request`]: crate::round_request
    pub fn begin_turn_with(
        &mut self,
        user_content: Vec<Value>,
        attachments: Vec<Attachment>,
        settings: &ChatSettings,
    ) {
        let sent_with = RoundSettings::new(&self.model, settings);
        self.start_turn(user_content, attachments, Some(sent_with));
    }

    fn start_turn(
        &mut self,
        user_content: Vec<Value>,
        attachments: Vec<Attachment>,
        sent_with: Option<RoundSettings>,
    ) {
        self.stop();
        if self.title.is_empty() {
            self.title = title_of(&user_content);
        }
        let removes_tools = self.removal_pending();
        self.turns.push(Turn {
            user_content,
            attachments,
            rounds: Vec::new(),
            reply: Vec::new(),
            results: Vec::new(),
            removes_tools,
            sent_with,
            model: None,
            asked_model: Some(self.model.clone()),
            asked_effort: Some(self.effort),
            stop_reason: None,
            stop_details: None,
            usage: None,
            state: TurnState::Streaming,
            invalid_resent: false,
            resend: false,
        });
    }

    /// Whether the next turn sent carries the `tool_removal` message: the
    /// tools were withdrawn and no turn in the history carries it yet.
    pub fn removal_pending(&self) -> bool {
        self.tools_withdrawn
            && !self.tools.is_empty()
            && !self.turns.iter().any(|turn| {
                turn.removes_tools && turn.state != TurnState::Streaming && turn.in_history()
            })
    }

    /// The turn being streamed into, if any: a reply streaming, its calls
    /// running, or its request to be sent again.
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

    /// Applies one event to the streaming turn's reply; ignored when no
    /// turn is streaming (a stopped turn's stream may still deliver).
    ///
    /// When the reply finishes:
    /// - `refusal`: the turn is refused, the reply dropped and any earlier
    ///   rounds kept;
    /// - ended during a call's input (a `tool_use` block that never
    ///   finished, or `max_tokens` with the reply's last block a call whose
    ///   input was cut short): the reply isn't committed. The turn is done
    ///   with the reply's finished text only: no `tool_use`, no results;
    /// - else, a finished call whose input isn't valid (not JSON, or not
    ///   the tool's input), whatever the stop reason: the reply is
    ///   dropped, never entering the history, and the request is to be
    ///   sent again ([`Conversation::needs_resend`]); the second time, the
    ///   turn fails;
    /// - `tool_use`: the turn waits for the calls' results
    ///   ([`Conversation::pending_calls`]), unless the chat has no tools or
    ///   they were withdrawn, whose calls are answered as not run and the
    ///   turn done;
    /// - anything else, `max_tokens` among them: the turn is done, every
    ///   call answered as not run ("cut off at the token limit" for
    ///   `max_tokens`).
    pub fn apply(&mut self, event: ChatEvent) {
        let unrun = if self.tools.is_empty() {
            Some(NotRun::Ended)
        } else if self.tools_withdrawn {
            Some(NotRun::TrustWithdrawn)
        } else {
            None
        };
        let Some(turn) = self.streaming_turn_mut() else {
            return;
        };
        turn.resend = false;
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
            ChatEvent::ToolInputParsed { index, status } => {
                let kind = || BlockKind::ToolUse {
                    id: String::new(),
                    name: String::new(),
                };
                turn.block_mut(index, kind).input_status = Some(status);
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
                turn.stop_reason = Some(stop_reason.clone());
                turn.stop_details = stop_details;
                match stop_reason {
                    StopReason::Refusal => turn.state = TurnState::Refused,
                    _ if turn.ended_mid_input(&stop_reason) => {
                        turn.keep_text_only();
                        turn.state = TurnState::Done;
                    }
                    _ if !turn.calls_are_valid() => turn.invalid_reply(),
                    StopReason::ToolUse => match unrun {
                        None if !turn.calls().is_empty() => {}
                        None => turn.end_round(NotRun::Ended, TurnState::Done),
                        Some(why) => turn.end_round(why, TurnState::Done),
                    },
                    StopReason::MaxTokens => turn.end_round(NotRun::CutOff, TurnState::Done),
                    _ => turn.end_round(unrun.unwrap_or(NotRun::Ended), TurnState::Done),
                }
            }
            // A reply that finished can't fail after: its calls are waiting.
            ChatEvent::Failed(_) if turn.stop_reason.is_some() => {}
            ChatEvent::Failed(error) => turn.state = TurnState::Failed(error),
        }
    }

    /// The streaming turn's calls still waiting for a result: those of a
    /// reply that finished with `tool_use`, in the reply's order. Empty
    /// otherwise.
    pub fn pending_calls(&self) -> Vec<ToolCall> {
        let Some(turn) = self.streaming_turn().filter(|turn| turn.awaits_results()) else {
            return Vec::new();
        };
        turn.calls()
            .into_iter()
            .filter(|call| !turn.answered(&call.id))
            .collect()
    }

    /// Adds a call's result to the streaming turn's pending result message.
    /// False, and nothing added, when no call of the round waits for it: an
    /// unknown id, or one already answered.
    pub fn add_result(&mut self, result: ToolResult) -> bool {
        let Some(turn) = self.streaming_turn_mut() else {
            return false;
        };
        let waiting = turn.awaits_results()
            && !turn.answered(&result.tool_use_id)
            && turn
                .calls()
                .iter()
                .any(|call| call.id == result.tool_use_id);
        if waiting {
            turn.results.push(result);
        }
        waiting
    }

    /// Commits the round once every call has its result, its results in the
    /// calls' order, and starts the next round: the turn streams again, its
    /// request built by [`round_request`](crate::round_request). False when
    /// no round waits or a call still has no result.
    pub fn next_round(&mut self) -> bool {
        let Some(turn) = self.streaming_turn_mut() else {
            return false;
        };
        let calls = turn.calls();
        if !turn.awaits_results() || calls.iter().any(|call| !turn.answered(&call.id)) {
            return false;
        }
        turn.commit();
        turn.stop_reason = None;
        turn.stop_details = None;
        turn.invalid_resent = false;
        true
    }

    /// Ends the streaming turn whose calls wait, without another round:
    /// every call without a result is answered as not run for `why`, the
    /// round is committed, and the turn ends with that result message
    /// last. Stopped for [`NotRun::Stopped`] and
    /// [`NotRun::TrustWithdrawn`], done otherwise: at the round or time
    /// limit, [`Conversation::resume`] continues it. False when no calls
    /// wait.
    pub fn end_round(&mut self, why: NotRun) -> bool {
        let Some(turn) = self
            .streaming_turn_mut()
            .filter(|turn| turn.awaits_results())
        else {
            return false;
        };
        let state = match why {
            NotRun::Stopped | NotRun::TrustWithdrawn => TurnState::Stopped,
            _ => TurnState::Done,
        };
        turn.end_round(why, state);
        true
    }

    /// Marks the streaming turn stopped, if there is one.
    ///
    /// - A reply still streaming isn't committed: it keeps its finished
    ///   text only, with no `tool_use` and no results.
    /// - Calls waiting for results each get one: their own if it came,
    ///   else "Stopped by the user". The round is committed, and the next
    ///   user text joins its result message.
    ///
    /// Earlier rounds are unchanged either way.
    pub fn stop(&mut self) {
        if !self.end_round(NotRun::Stopped)
            && let Some(turn) = self.streaming_turn_mut()
        {
            turn.state = TurnState::Stopped;
            turn.results.clear();
        }
    }

    /// Withdraws the chat's tools (the folder's trust was withdrawn, or its
    /// root changed), ending the streaming turn stopped:
    ///
    /// - a reply still streaming is finalised as [`Conversation::stop`]
    ///   does: finished text only;
    /// - of calls waiting for results, a finished `propose_edit` keeps its
    ///   result; every other call is answered "Folder trust was withdrawn",
    ///   a finished one's content discarded.
    ///
    /// The next turn sent carries the `tool_removal` message after its
    /// user text, and later calls are answered as not run.
    pub fn withdraw_tools(&mut self) {
        if let Some(turn) = self
            .streaming_turn_mut()
            .filter(|turn| turn.awaits_results())
        {
            let calls = turn.calls();
            turn.results.retain(|result| {
                calls
                    .iter()
                    .any(|call| call.id == result.tool_use_id && call.name == tools::PROPOSE_EDIT)
            });
        }
        if !self.end_round(NotRun::TrustWithdrawn)
            && let Some(turn) = self.streaming_turn_mut()
        {
            turn.state = TurnState::Stopped;
            turn.results.clear();
        }
        if !self.tools.is_empty() {
            self.tools_withdrawn = true;
        }
    }

    /// Whether the streaming turn's reply was dropped for invalid tool
    /// input and its request must be sent again, unchanged, as
    /// [`round_request`](crate::round_request) builds it. Cleared by the
    /// next event.
    pub fn needs_resend(&self) -> bool {
        self.streaming_turn().is_some_and(|turn| turn.resend)
    }

    /// Continues the last turn after it ended with a result message last
    /// (at the round or time limit): it streams again, and its next request
    /// sends that message. False when it didn't end so, or the tools were
    /// withdrawn.
    pub fn resume(&mut self) -> bool {
        if self.tools_withdrawn {
            return false;
        }
        let Some(turn) = self.turns.last_mut() else {
            return false;
        };
        let ended = matches!(turn.state, TurnState::Done | TurnState::Stopped);
        if !ended || turn.rounds.is_empty() || !turn.last_reply().is_empty() {
            return false;
        }
        turn.restart();
        true
    }

    /// Retries the last turn's failed round: it streams again, and its
    /// request, built by [`round_request`](crate::round_request), is the
    /// one that failed, byte for byte. False when the last turn didn't
    /// fail.
    ///
    /// After the tools were withdrawn, a round whose request had no
    /// `tool_removal` message (neither the turn nor one before it in the
    /// history carries it) isn't retried: the next send carries the
    /// removal instead.
    pub fn retry_round(&mut self) -> bool {
        let Some(last) = self.turns.len().checked_sub(1) else {
            return false;
        };
        let removal_sent = self.turns.iter().enumerate().any(|(at, turn)| {
            turn.removes_tools
                && (at == last || (turn.state != TurnState::Streaming && turn.in_history()))
        });
        if self.tools_withdrawn && !self.tools.is_empty() && !removal_sent {
            return false;
        }
        let turn = &mut self.turns[last];
        if !matches!(turn.state, TurnState::Failed(_)) {
            return false;
        }
        turn.restart();
        true
    }

    /// The API history: every turn in it, passed back unchanged, so each
    /// request's prefix is the last one's.
    ///
    /// Per turn: its user content; then each committed round's reply,
    /// verbatim, and its result message; then the last round's reply:
    ///
    /// - done: its finished blocks, thinking included; after a mid-output
    ///   fallback, without the `thinking`, `redacted_thinking` and
    ///   `tool_use` blocks before the last `fallback` block;
    /// - stopped: its finished `text` blocks only;
    /// - refused, failed and streaming: left out (the partial is
    ///   discarded).
    ///
    /// A turn with nothing from the model in it is left out, since an
    /// empty assistant message is a 400; a streaming turn is too. A turn
    /// that ends with a result message has the next turn's user content
    /// join that message, after the results. The first turn sent after the
    /// tools were withdrawn has the `tool_removal` message after its user
    /// content.
    pub fn history(&self) -> Vec<ApiMessage> {
        self.layout(false, false).messages
    }

    /// The history laid out, with effort-only messages when
    /// `effort_messages`; with `current`, the last turn too, as its next
    /// round's request sends it: its user message and committed rounds.
    pub(crate) fn layout(&self, effort_messages: bool, current: bool) -> Layout {
        let mut layout = Layout {
            messages: Vec::new(),
            in_effect: self.first_effort(),
            effort_messages,
        };
        let last = self.turns.len().checked_sub(1);
        for (at, turn) in self.turns.iter().enumerate() {
            let current = current && Some(at) == last;
            if !current && (turn.state == TurnState::Streaming || !turn.in_history()) {
                continue;
            }
            layout.user(self.asked_effort(turn), &turn.user_content);
            if turn.removes_tools {
                layout.removal(&self.tools);
            }
            for round in &turn.rounds {
                layout.assistant(echoed(round.reply.iter().filter_map(|b| b.block.as_ref())));
                layout.results(&round.results);
            }
            if !current {
                layout.assistant(turn.last_reply());
            }
        }
        layout
    }
}

/// Messages as a request sends them, laid out turn by turn.
pub(crate) struct Layout {
    pub(crate) messages: Vec<ApiMessage>,
    /// The effort in effect after the last message.
    pub(crate) in_effect: Effort,
    effort_messages: bool,
}

impl Layout {
    /// A turn's user content, asking for `asked`: after an effort-only
    /// message when it changes the effort in effect (with effort
    /// messages), and else joined to the last message when that is the
    /// user's, a result message nothing answered, after its blocks.
    pub(crate) fn user(&mut self, asked: Effort, content: &[Value]) {
        if self.effort_messages && asked != self.in_effect {
            self.messages.push(ApiMessage::effort(asked));
            self.in_effect = asked;
        }
        match self.messages.last_mut() {
            Some(last) if last.role == Role::User => last.content.extend_from_slice(content),
            _ => self.messages.push(ApiMessage {
                role: Role::User,
                content: content.to_vec(),
            }),
        }
    }

    /// The `tool_removal` message withdrawing `tools`, if there are any.
    pub(crate) fn removal(&mut self, tools: &[ToolSpec]) {
        if !tools.is_empty() {
            let names = tools.iter().map(|tool| tool.name.as_str());
            self.messages
                .push(ApiMessage::tool_removal(names, self.in_effect));
        }
    }

    fn assistant(&mut self, content: Vec<Value>) {
        if !content.is_empty() {
            self.messages.push(ApiMessage {
                role: Role::Assistant,
                content,
            });
        }
    }

    fn results(&mut self, results: &[ToolResult]) {
        self.messages.push(ApiMessage {
            role: Role::User,
            content: results.iter().map(ToolResult::block).collect(),
        });
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
                    input_status: None,
                };
                self.reply.insert(at, block);
                at
            }
        };
        &mut self.reply[at]
    }

    /// The last round's reply's finished blocks, verbatim, in order.
    pub fn reply_blocks(&self) -> impl Iterator<Item = &Value> {
        self.reply.iter().filter_map(|block| block.block.as_ref())
    }

    /// The reply's text as it streamed, over every round: each text
    /// block's deltas, joined, with a blank line between rounds.
    pub fn text(&self) -> String {
        self.streamed(|kind| *kind == BlockKind::Text)
    }

    /// The reply's thinking as it streamed, over every round.
    pub fn thinking(&self) -> String {
        self.streamed(|kind| *kind == BlockKind::Thinking)
    }

    fn streamed(&self, wanted: impl Fn(&BlockKind) -> bool) -> String {
        let of = |reply: &[ReplyBlock]| -> String {
            reply
                .iter()
                .filter(|block| wanted(&block.kind))
                .map(|block| block.text.as_str())
                .collect()
        };
        let rounds = self.rounds.iter().map(|round| of(&round.reply));
        let parts: Vec<String> = rounds
            .chain([of(&self.reply)])
            .filter(|part| !part.is_empty())
            .collect();
        parts.join("\n\n")
    }

    /// Whether the reply was cut off at the token limit.
    pub fn cut_off(&self) -> bool {
        self.stop_reason == Some(StopReason::MaxTokens)
    }

    /// The last round's reply's tool calls, as they go back: its finished
    /// `tool_use` blocks, after a mid-output fallback only those after the
    /// last `fallback` block. Each is answered by exactly one result.
    pub fn calls(&self) -> Vec<ToolCall> {
        echoed(self.reply_blocks())
            .into_iter()
            .filter(|block| block["type"] == "tool_use")
            .map(|block| ToolCall {
                id: block["id"].as_str().unwrap_or_default().to_owned(),
                name: block["name"].as_str().unwrap_or_default().to_owned(),
                input: block["input"].clone(),
            })
            .collect()
    }

    /// The last round's reply as the history sends it, by the turn's
    /// state: a done reply's blocks, a stopped one's finished text, else
    /// nothing.
    fn last_reply(&self) -> Vec<Value> {
        match self.state {
            TurnState::Done => echoed(self.reply_blocks()),
            TurnState::Stopped => self
                .reply_blocks()
                .filter(|block| block["type"] == "text")
                .cloned()
                .collect(),
            TurnState::Streaming | TurnState::Refused | TurnState::Failed(_) => Vec::new(),
        }
    }

    /// Whether the turn has anything from the model to send, which puts it
    /// in the history.
    fn in_history(&self) -> bool {
        !self.rounds.is_empty() || !self.last_reply().is_empty()
    }

    /// Whether the reply finished asking for tools and waits for their
    /// results.
    fn awaits_results(&self) -> bool {
        self.state == TurnState::Streaming && self.stop_reason == Some(StopReason::ToolUse)
    }

    fn answered(&self, id: &str) -> bool {
        self.results.iter().any(|result| result.tool_use_id == id)
    }

    /// The `tool_use` blocks that go back with the reply: those after the
    /// last fallback block, finished or not.
    fn kept_tool_uses(&self) -> impl Iterator<Item = &ReplyBlock> {
        let last_fallback = self
            .reply
            .iter()
            .rposition(|block| matches!(block.kind, BlockKind::Fallback { .. }));
        self.reply
            .iter()
            .skip(last_fallback.map_or(0, |at| at + 1))
            .filter(|block| matches!(block.kind, BlockKind::ToolUse { .. }))
    }

    /// Whether the reply ended during a call's input: a kept `tool_use`
    /// block never finished, or, at `max_tokens`, the reply's last block is
    /// a kept call whose input was cut short as it streamed, so isn't JSON.
    fn ended_mid_input(&self, stop_reason: &StopReason) -> bool {
        if self.kept_tool_uses().any(|block| block.block.is_none()) {
            return true;
        }
        // Input cut short isn't JSON: the provider says it's unparsed (or,
        // without a status, keeps the raw string). Complete JSON that fails
        // validation, a string included, wasn't cut short.
        *stop_reason == StopReason::MaxTokens
            && self.reply.last().is_some_and(|last| {
                self.kept_tool_uses()
                    .last()
                    .is_some_and(|kept| kept.index == last.index)
                    && last.block.as_ref().is_some_and(|block| {
                        let _ = block;
                        last.input_status == Some(InputStatus::Unparsed)
                    })
            })
    }

    /// Keeps the reply's finished text only, as an uncommitted reply goes
    /// back: every other block unfinished, so it doesn't.
    fn keep_text_only(&mut self) {
        for block in &mut self.reply {
            if block
                .block
                .as_ref()
                .is_some_and(|block| block["type"] != "text")
            {
                block.block = None;
            }
        }
    }

    /// Whether every finished call the reply keeps has valid input.
    fn calls_are_valid(&self) -> bool {
        self.kept_tool_uses()
            .filter_map(|block| block.block.as_ref())
            .all(valid_call)
    }

    /// Drops a reply whose tool input was invalid, never committing it:
    /// the first time, its request is to be sent again; the second, the
    /// turn fails.
    fn invalid_reply(&mut self) {
        if self.invalid_resent {
            let error = ProviderError::new(ErrorKind::Stream, INVALID_TOOL_INPUT);
            self.state = TurnState::Failed(error);
            return;
        }
        self.invalid_resent = true;
        self.resend = true;
        self.reply.clear();
        self.results.clear();
        self.stop_reason = None;
        self.stop_details = None;
    }

    /// Answers every call without a result as not run for `why`, commits
    /// the round when it called any, and ends the turn as `state`.
    fn end_round(&mut self, why: NotRun, state: TurnState) {
        let calls = self.calls();
        for call in &calls {
            if !self.answered(&call.id) {
                self.results.push(ToolResult::not_run(&call.id, why));
            }
        }
        if !calls.is_empty() {
            self.commit();
        }
        self.state = state;
    }

    /// Moves the reply and its results, in the calls' order, into a
    /// committed round. Every call must have its result.
    fn commit(&mut self) {
        let mut results = std::mem::take(&mut self.results);
        let ordered = self
            .calls()
            .iter()
            .filter_map(|call| {
                let at = results.iter().position(|r| r.tool_use_id == call.id)?;
                Some(results.remove(at))
            })
            .collect();
        let reply = std::mem::take(&mut self.reply);
        self.rounds.push(Round {
            reply,
            results: ordered,
        });
    }

    /// Streams the last round again, from an empty reply.
    fn restart(&mut self) {
        self.state = TurnState::Streaming;
        self.reply.clear();
        self.results.clear();
        self.stop_reason = None;
        self.stop_details = None;
        self.invalid_resent = false;
        self.resend = false;
    }
}

/// Whether a finished `tool_use` block's input can go back and be
/// answered: see [`tools::input_is_valid`].
fn valid_call(block: &Value) -> bool {
    let name = block["name"].as_str().unwrap_or_default();
    tools::input_is_valid(name, &block["input"])
}
