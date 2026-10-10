//! Golden screenshots of the editor view, rendered off screen with the real
//! Metal renderer (no screen-recording permission needed).
//!
//! ```text
//! cargo run -p fe-gpui --example screenshots                  # compare with golden/
//! UPDATE_SCREENSHOTS=1 cargo run -p fe-gpui --example screenshots
//! cargo run -p fe-gpui --example screenshots -- render out.png <file> [--size WxH]
//! ```
//!
//! AppKit must run on the main thread, which `main` is but test threads
//! aren't, so `cargo test` only runs the comparison's unit tests. The
//! scenes render only on macOS; elsewhere `main` says so and fails.

// Under `cargo test` the harness replaces `main`, leaving the rest unused,
// as it is off macOS, where there are no scenes.
#![cfg_attr(any(test, not(target_os = "macos")), allow(dead_code))]

mod compare;
#[cfg(target_os = "macos")]
mod scenes;

use std::process::ExitCode;

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    scenes::main()
}

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    eprintln!("error: screenshots render with Metal, on macOS only");
    ExitCode::FAILURE
}
