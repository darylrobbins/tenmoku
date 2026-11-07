//! Golden screenshot of the workbench: the project tree, the tab bar and the
//! editor, rendered off screen with the real Metal renderer.
//!
//! ```text
//! cargo run -p fe-workbench --example screenshots                  # compare with golden/
//! UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots
//! cargo run -p fe-workbench --example screenshots -- render out.png <dir> [--size WxH]
//! cargo run -p fe-workbench --example screenshots -- audit target/ui-audit
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
//! (`setup_scene`), against goldens of their own. They load settings, so
//! they draw the default `workbench.icon_theme`'s coloured icons; the two
//! scenes before them have no settings and stay on the line icons.
//!
//! Last, the project tree's file and folder icons, dark and light
//! (`icons_scene`).
//!
//! `audit <out-dir>` compares nothing: it writes every surface the UI polish
//! audit looks at, dark and light, as `<out-dir>/<surface>_<dark|light>.png`,
//! and the chrome under every bundled theme as `<out-dir>/<theme id>.png`
//! (`audit_scenes`, `docs/decisions/25w38k498b-ui-polish-audit.md`). The
//! comparing run doesn't render them.

#[cfg(target_os = "macos")]
mod audit_scenes;
#[cfg(target_os = "macos")]
mod icons_scene;
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
