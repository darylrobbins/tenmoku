//! [`AcpEvent`]: what a session update means to the chat panel.
//!
//! The decision record's § 5 says how each ACP update renders. This module
//! keeps the updates the panel renders and names the rest, so the panel
//! never matches on the crate's `#[non_exhaustive]` `SessionUpdate` itself.

use agent_client_protocol::schema::MaybeUndefined;
use agent_client_protocol::v1::schema::{
    ContentBlock, Plan, SessionConfigOption, SessionModeId, SessionUpdate, ToolCall, ToolCallUpdate,
};

/// One session update, as the chat panel uses it.
#[derive(Debug, Clone, PartialEq)]
pub enum AcpEvent {
    /// A chunk of the user's message. The agent sends these when it replays
    /// a session's history on `session/load`.
    UserMessage(ContentBlock),
    /// A chunk of the agent's reply (`agent_message_chunk`).
    AgentMessage(ContentBlock),
    /// A chunk of the agent's reasoning (`agent_thought_chunk`).
    AgentThought(ContentBlock),
    /// A new tool call (`tool_call`).
    ToolCall(ToolCall),
    /// A change to a tool call: only the fields it carries changed
    /// (`tool_call_update`).
    ToolCallUpdate(ToolCallUpdate),
    /// The agent's plan, whole: it replaces the previous one (`plan`).
    Plan(Plan),
    /// The session's mode changed (`current_mode_update`).
    ModeChanged(SessionModeId),
    /// The session's configuration options, whole: model, effort and the
    /// like (`config_option_update`).
    ConfigOptions(Vec<SessionConfigOption>),
    /// The session's title was set, or cleared with `None`
    /// (`session_info_update` with a `title`).
    TitleChanged(Option<String>),
    /// An update the panel doesn't use, by its `sessionUpdate` name:
    /// available commands, usage, notices, compaction, a session info
    /// update without a title, and any variant this crate doesn't know
    /// (`"unknown"`). The client drops these with a debug log.
    Ignored(&'static str),
}

impl AcpEvent {
    /// The text of a message, thought or user chunk, when its content is
    /// text.
    #[must_use]
    pub fn chunk_text(&self) -> Option<&str> {
        match self {
            Self::UserMessage(block) | Self::AgentMessage(block) | Self::AgentThought(block) => {
                match block {
                    ContentBlock::Text(text) => Some(&text.text),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

impl From<SessionUpdate> for AcpEvent {
    fn from(update: SessionUpdate) -> Self {
        match update {
            SessionUpdate::UserMessageChunk(chunk) => Self::UserMessage(chunk.content),
            SessionUpdate::AgentMessageChunk(chunk) => Self::AgentMessage(chunk.content),
            SessionUpdate::AgentThoughtChunk(chunk) => Self::AgentThought(chunk.content),
            SessionUpdate::ToolCall(call) => Self::ToolCall(call),
            SessionUpdate::ToolCallUpdate(update) => Self::ToolCallUpdate(update),
            SessionUpdate::Plan(plan) => Self::Plan(plan),
            SessionUpdate::CurrentModeUpdate(update) => Self::ModeChanged(update.current_mode_id),
            SessionUpdate::ConfigOptionUpdate(update) => Self::ConfigOptions(update.config_options),
            SessionUpdate::SessionInfoUpdate(info) => match info.title {
                MaybeUndefined::Value(title) => Self::TitleChanged(Some(title)),
                MaybeUndefined::Null => Self::TitleChanged(None),
                MaybeUndefined::Undefined => Self::Ignored("session_info_update"),
            },
            SessionUpdate::AvailableCommandsUpdate(_) => Self::Ignored("available_commands_update"),
            SessionUpdate::UsageUpdate(_) => Self::Ignored("usage_update"),
            SessionUpdate::Notice(_) => Self::Ignored("notice"),
            SessionUpdate::CompactionUpdate(_) => Self::Ignored("compaction_update"),
            SessionUpdate::CompactionSummaryChunk(_) => Self::Ignored("compaction_summary_chunk"),
            // Unstable variants (subagents, plan operations) and any variant
            // added in a later schema.
            _ => Self::Ignored("unknown"),
        }
    }
}
