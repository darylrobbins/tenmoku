//! The AI chat's GPUI view (the decision record:
//! `docs/decisions/5at38c8v89-ai-chat.md`, § 1): a [`ChatView`] over any
//! [`fe_chat::ChatProvider`], with the transcript, the composer and the
//! attachments waiting to be sent.
//!
//! - It streams a reply into its turn as the provider's events arrive,
//!   stops it ([`StopReply`]) by dropping the stream, and resends a failed
//!   or refused turn ([`RetryTurn`]).
//! - Each turn shows the record's § 14 state: the error, refusal, fallback
//!   and cut-off banners, with their actions.
//! - Replies are plain text; Markdown is a later card.
//!
//! It knows nothing of fastedit's workbench: the host gives it a provider
//! and context items and listens for [`ChatViewEvent`]s.

use gpui::App;

mod view;

pub use view::{
    Banner, BannerAction, ChatOptions, ChatView, ChatViewEvent, RetryTurn, SendMessage, StopReply,
    TurnDisplay,
};

/// The key context of a [`ChatView`], which its commands run in.
pub const KEY_CONTEXT: &str = "chat";

/// The key context of the composer inside it.
pub const COMPOSER_CONTEXT: &str = "chat_composer";

// The view's typed commands (`#[kiln::command]`), which `init` registers.
kiln::module!(pub fn module);

/// Registers the chat view's commands. Call after [`fe_gpui::init`].
pub fn init(cx: &mut App) {
    module(cx);
}
