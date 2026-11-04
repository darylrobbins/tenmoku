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
//!
//! After the golden, the comparing run renders the scene under two probe
//! themes and fails when more of its pixels than `token_probe` allows are
//! the same in both: a colour that bypasses the theme. It writes
//! `target/screenshots/workbench_tokens.diff.png`, those pixels in magenta.
//!
//! Then the setup wizard's sheet over the same workbench, dark and light
//! (`setup_scene`), against goldens of their own.

#[cfg(target_os = "macos")]
mod scenes;
#[cfg(target_os = "macos")]
mod setup_scene;
#[cfg(target_os = "macos")]
mod token_probe;

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
