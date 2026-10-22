//! The AI chat's headless core, which fastedit's chat panel and the host
//! product share (the decision record:
//! `docs/decisions/5at38c8v89-ai-chat.md`).
//!
//! - [`ChatProvider`] is the seam to a model: a [`ChatRequest`] in, a stream
//!   of [`ChatEvent`]s out, dropped to cancel.
//! - [`Conversation`] holds a chat's turns. [`Conversation::apply`] builds a
//!   streaming reply from the events, keeping each finished block verbatim,
//!   and [`Conversation::history`] gives back what the API is sent: append
//!   only, never re-rendered.
//! - [`build_request`] lays a request out for the prompt cache: a constant
//!   [`SYSTEM_PROMPT`] with a breakpoint, the history, then the new turn
//!   from [`render_user_content`].
//! - [`budget`] fits a turn's [`ContextItem`]s into the context budget,
//!   in the record's order, cutting or dropping what doesn't fit.
//! - [`parse_edit_block`] reads a reply's code block as an edit: a whole
//!   file, the selection, or SEARCH/REPLACE sections.
//! - [`ScriptedProvider`] plays scripted replies and records the requests,
//!   for tests.
//!
//! No network here, and this crate must never depend on GPUI.

mod context;
mod conversation;
mod edits;
mod fake;
mod provider;
mod request;

pub use context::{Budgeted, ContextItem, ContextKind, Fit, MIN_ROWS, budget};
pub use conversation::{
    Attachment, AttachmentKind, ChatId, Conversation, ParseChatIdError, ReplyBlock, Rows, Turn,
    TurnState,
};
pub use edits::{
    DIVIDER_MARKER, EditBlock, EditBlockError, EditBody, REPLACE_MARKER, SEARCH_MARKER, info_path,
    parse_edit_block,
};
pub use fake::ScriptedProvider;
pub use provider::{
    ApiMessage, BlockKind, CacheControl, ChatEvent, ChatProvider, ChatRequest, ChatStream, Effort,
    ErrorKind, ProviderError, Role, StopReason, SystemBlock, ToolCall, ToolChoice, ToolSpec, Usage,
};
pub use request::{ChatSettings, SYSTEM_PROMPT, build_request, render_user_content};
