//! The AI chat's headless core, which fastedit's chat panel and the host
//! product share (the decision record:
//! `docs/decisions/5at38c8v89-ai-chat.md`).
//!
//! - [`ChatProvider`] is the seam to a model: a [`ChatRequest`] in, a stream
//!   of [`ChatEvent`]s out, dropped to cancel.
//! - [`Conversation`] holds a chat's turns. [`Conversation::apply`] builds a
//!   streaming reply from the events, keeping each finished block verbatim,
//!   and [`Conversation::history`] gives back what the API is sent: append
//!   only, never re-rendered. A turn runs in rounds when the model calls
//!   tools: [`Conversation::pending_calls`], [`Conversation::add_result`]
//!   and [`Conversation::next_round`], then [`round_request`].
//! - [`tool_set`] is the API chat's five tools, and [`ToolInput`] a call's
//!   validated input.
//! - [`build_request`] lays a request out for the prompt cache: a constant
//!   [`SYSTEM_PROMPT`] with a breakpoint, the history, then the new turn
//!   from [`render_user_content`].
//! - [`budget`] fits a turn's [`ContextItem`]s into the context budget,
//!   in the record's order, cutting or dropping what doesn't fit.
//! - [`parse_edit_block`] reads a reply's code block as an edit: a whole
//!   file, the selection, or SEARCH/REPLACE sections.
//! - [`inline_request`], [`InlineEditSession`], [`run_inline_edit`] and
//!   [`to_proposal`] turn a selection and an instruction into a proposed
//!   change.
//! - [`ChatStore`] keeps a folder's chats as atomically written files,
//!   listed newest first.
//! - [`ScriptedProvider`] plays scripted replies and records the requests,
//!   for tests; [`ReplyScript`] writes a reply's events, tool calls
//!   included, for multi-round scripts.
//!
//! No network here, and this crate must never depend on GPUI.

mod context;
mod conversation;
mod edits;
mod fake;
mod inline;
pub mod l10n;
pub mod permissions;
mod provider;
mod request;
mod store;
mod tools;

pub use context::{Budgeted, ContextItem, ContextKind, Fit, MIN_ROWS, budget};
pub use conversation::{
    Attachment, AttachmentKind, ChatId, Conversation, ParseChatIdError, ReplyBlock, Round,
    RoundSettings, Rows, ToolResult, Turn, TurnState,
};
pub use edits::{
    DIVIDER_MARKER, EditBlock, EditBlockError, EditBody, REPLACE_MARKER, SEARCH_MARKER, info_path,
    parse_edit_block,
};
pub use fake::{ReplyScript, ScriptedProvider};
pub use inline::{
    CONTEXT_BYTES, INLINE_EDIT_AGENT, InlineEditInput, InlineEditSession, InlineEditState,
    InlineEditUpdate, inline_request, run_inline_edit, to_proposal,
};
pub use provider::{
    ApiMessage, BlockKind, CacheControl, ChatEvent, ChatProvider, ChatRequest, ChatStream, Effort,
    ErrorKind, InputStatus, ProviderError, Role, StopReason, SystemBlock, ToolCall, ToolChoice,
    ToolSpec, Usage,
};
pub use request::{ChatSettings, SYSTEM_PROMPT, build_request, render_user_content, round_request};
pub use store::{ChatStore, ChatSummary, StoreError};
pub use tools::{
    InputError, LIST_FILES, ListFilesInput, MIN_TASK_BUDGET, NotRun, PROPOSE_EDIT,
    ProposeEditInput, ProposedEdit, READ_FILE, RUN_COMMAND, ReadFileInput, RunCommandInput, SEARCH,
    SearchInput, ToolInput, input_is_valid, tool_set,
};
