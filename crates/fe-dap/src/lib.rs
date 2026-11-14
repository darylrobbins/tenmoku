// SPDX-License-Identifier: Apache-2.0

//! A Debug Adapter Protocol client's foundation, with no GPUI
//! (docs/decisions/r1b9dw3g0g-debugger.md §2, card D1).
//!
//! - [`transport`]: `Content-Length` framing over an adapter's stdio or a
//!   TCP connection to it, with a 64 MB body limit.
//! - [`protocol`]: hand-written serde types for the requests, events and
//!   reverse requests the client uses, keeping every field they don't name.
//! - [`AdapterLauncher`]: the launch seam, and [`LocalLauncher`], which
//!   starts an adapter in its own process group and, for a TCP adapter,
//!   substitutes `${port}` and connects with retries on an injected
//!   [`Clock`].
//! - `fake` (feature `test-support`): a scripted in-process adapter, and
//!   the `fe-dap-fake` binary serving it over TCP.
//!
//! Its runtime is fe-lsp's: a thread per session under `async-io`, the
//! adapter through `async-process`, no tokio. No public item names a
//! dependency's type, apart from `futures`' I/O traits and `serde_json`'s
//! values.

pub mod clock;
#[cfg(any(test, feature = "test-support"))]
pub mod fake;
pub mod l10n;
pub mod launcher;
pub mod protocol;
pub mod transport;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use clock::ManualClock;
pub use clock::{Clock, SystemClock};
pub use launcher::{
    AdapterIo, AdapterLauncher, AdapterProcess, AdapterSpec, AdapterTransport, CONNECT_RETRY,
    CONNECT_TIMEOUT, ExitInfo, LaunchCancel, LocalLauncher, PORT_PLACEHOLDER,
};
pub use protocol::Message;
pub use transport::{FrameReader, FrameWriter, MAX_BODY, ProtocolError, TransportError};
