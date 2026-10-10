//! Golden screenshot of the workbench: the project tree, the tab bar and the
//! editor, rendered off screen with the real Metal renderer. The harness is
//! fe-gpui's (`crates/fe-gpui/examples/screenshots`), whose comparison it
//! includes.
//!
//! ```text
//! cargo run -p fe-workbench --example screenshots                  # compare with golden/
//! UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots
//! cargo run -p fe-workbench --example screenshots -- render out.png <dir> [--size WxH]
//! ```
//!
//! AppKit must run on the main thread, which `main` is but test threads
//! aren't, so `cargo test` only runs the comparison's unit tests. The
//! scenes render only on macOS; elsewhere `main` says so and fails.

// Under `cargo test` the harness replaces `main`, leaving the rest unused,
// as it is off macOS, where there are no scenes.
#![cfg_attr(any(test, not(target_os = "macos")), allow(dead_code))]

#[path = "../../../fe-gpui/examples/screenshots/compare.rs"]
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
