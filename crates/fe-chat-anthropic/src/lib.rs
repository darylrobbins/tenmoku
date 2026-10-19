//! Claude through the Anthropic Messages API, for `fe-chat` (the decision
//! record: `docs/decisions/5at38c8v89-ai-chat.md`, § 3 and § 14).
//!
//! - [`AnthropicProvider`] implements [`fe_chat::ChatProvider`]: it sends a
//!   [`fe_chat::ChatRequest`] as a streamed `POST /v1/messages`, turns the
//!   reply's server-sent events into [`fe_chat::ChatEvent`]s, and retries
//!   what the record says may be retried.
//! - [`Transport`] is the seam to HTTP. This crate has no network of its
//!   own: `fe-net` supplies the HTTPS one, and tests replay recorded bytes.
//! - [`SseParser`] reads server-sent events incrementally, from bytes split
//!   anywhere.
//!
//! This crate must never depend on GPUI.

mod error;
mod events;
mod provider;
mod sse;
mod transport;
mod wire;

pub use error::error_from_response;
pub use events::EventDecoder;
pub use provider::{AnthropicProvider, ApiKey, DEFAULT_BASE_URL, Sleep};
pub use sse::{SseEvent, SseParser};
pub use transport::{Body, Response, Transport, TransportError};
pub use wire::{ANTHROPIC_VERSION, FALLBACK_BETA, request_body, request_headers};
