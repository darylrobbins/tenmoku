//! Golden screenshot of the workbench: the project tree, the tab bar and the
//! editor, rendered off screen with the real Metal renderer.
//!
//! ```text
//! cargo run -p fe-workbench --example screenshots                  # compare with golden/
//! UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots
//! cargo run -p fe-workbench --example screenshots -- render out.png <dir> [--size WxH]
//! ```
//!
//! The harness is kiln's (`kiln_platform::screenshots`, behind its
//! `test-support` feature). AppKit must run on the main thread, which
//! `main` is but test threads aren't, so the scenes render only under
//! `cargo run`, and only on macOS: elsewhere `main` says so and exits
//! cleanly, as the goldens are Metal's.

#[cfg(target_os = "macos")]
mod scenes;

use std::process::ExitCode;

#[cfg(target_os = "macos")]
fn main() -> ExitCode {
    scenes::main()
}

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    println!("skipped: the goldens are rendered with Metal, on macOS only");
    ExitCode::SUCCESS
}
