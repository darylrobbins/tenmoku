//! The Agent Client Protocol (ACP) client for Claude Agent (the decision
//! record: `docs/decisions/50t8wt9we2-ai-agents.md`, § 4).
//!
//! - [`connect`] makes an ACP v1 client connection over a transport, and
//!   returns an [`AcpClient`], the stream of [`Incoming`] messages and the
//!   connection future for the caller to drive.
//! - [`AcpClient`] sends `initialize`, `session/new`, `session/load`,
//!   `session/prompt`, `session/cancel`, `session/set_config_option` and
//!   `session/set_mode`.
//! - [`Incoming`] carries session updates as [`AcpEvent`]s, and
//!   [`PermissionRequest`]s for the caller to answer.
//!
//! The protocol's message types are re-exported as [`schema`], and the
//! transports as [`Channel`] (in-process) and [`ByteStreams`] (a child's
//! stdin and stdout), so callers need no direct dependency on
//! `agent-client-protocol`.
//!
//! fe-acp is headless: no GPUI, and no runtime of its own.

mod client;
mod events;

pub use client::{
    AcpClient, CLIENT_NAME, Incoming, PermissionRequest, Result, client_capabilities, connect,
};
pub use events::AcpEvent;

pub use agent_client_protocol::v1::schema;
pub use agent_client_protocol::{ByteStreams, Channel, Client, ConnectTo, Error, ErrorCode};
