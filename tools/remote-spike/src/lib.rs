//! Throwaway spike for remote development (T1986). See README.md and
//! docs/decisions/j8wvgszah0-remote-development.md. Not shipped code: no crate
//! depends on it and it is outside fastedit's workspace.

pub mod bench;
pub mod client;
pub mod codec;
pub mod frame;
pub mod server;
pub mod transport;
