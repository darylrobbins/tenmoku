// SPDX-License-Identifier: LicenseRef-Proprietary

//! Golden screenshots, for an app's examples: a view drawn in a window off
//! screen by the platform's real renderer, captured once it settles and
//! compared with a PNG checked in beside it. Behind the `test-support`
//! feature.
//!
//! An example's `main` makes an [`OffscreenApp`] with [`offscreen_app`],
//! opens each scene with [`Offscreen::open_offscreen_window`], captures it
//! with a [`Settle`] that observes its views, and checks it with
//! [`Goldens::check`]:
//!
//! ```text
//! cargo run -p <crate> --example <screenshots>                       # compare
//! UPDATE_SCREENSHOTS=1 cargo run -p <crate> --example <screenshots>  # rewrite
//! ```
//!
//! A pixel matches when each RGBA channel is within [`CHANNEL_TOLERANCE`]
//! of the golden's, and a scene when at most [`MAX_DIFFERING`] of its
//! pixels differ. A failure writes `<scene>.actual.png` and
//! `<scene>.diff.png` (the capture dimmed, differing pixels in magenta) to
//! `target/screenshots/`. Goldens are 2×; with [`ALLOW_OTHER_SCALE`] set, a
//! capture at another scale is skipped instead of failing on its size.
//!
//! On macOS the windows are real ones at (-10000, -10000) drawn by Metal,
//! and AppKit needs them on the main thread, so scenes render under
//! `cargo run`, not `cargo test`. Elsewhere [`OffscreenApp`] is GPUI's
//! headless context: Linux draws with wgpu where there's a Vulkan device,
//! with its own fonts and text system, so its captures are for looking at,
//! not for comparing with goldens made on macOS.

mod capture;
mod compare;
mod goldens;

pub use capture::{MAX_FRAMES, Offscreen, OffscreenApp, Settle, offscreen_app};
pub use compare::{
    ALLOW_OTHER_SCALE, CHANNEL_TOLERANCE, MAX_DIFFERING, Mismatch, MismatchKind, compare,
    save_failure, skip_other_scale,
};
pub use goldens::{Goldens, UPDATE};
pub use image::{self, RgbaImage};

/// The harness's errors: from GPUI, the file system or a PNG.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// `render <out.png> <target> [--size WxH]`'s arguments after `render`:
/// the image to write, what to show, and the window's size in logical
/// pixels (`size` unless given).
pub fn render_args<'a>(
    args: &'a [String],
    size: gpui::Size<gpui::Pixels>,
    usage: &str,
) -> Result<(&'a str, &'a str, gpui::Size<gpui::Pixels>)> {
    let (out, target, rest) = match args {
        [out, target, rest @ ..] => (out, target, rest),
        _ => return Err(usage.into()),
    };
    let size = match rest {
        [] => size,
        [flag, value] if flag == "--size" => {
            let (w, h) = value.split_once('x').ok_or(usage)?;
            gpui::size(gpui::px(w.parse()?), gpui::px(h.parse()?))
        }
        _ => return Err(usage.into()),
    };
    Ok((out, target, size))
}

/// The exit code for a run that returned whether every scene matched,
/// printing an error.
pub fn exit_code(result: Result<bool>) -> std::process::ExitCode {
    match result {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{px, size};

    use super::*;

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn render_args_take_an_optional_size() {
        let default = size(px(800.), px(480.));
        let given = args(&["out.png", "file.rs"]);
        let (out, target, got) = render_args(&given, default, "usage").unwrap();
        assert_eq!((out, target, got), ("out.png", "file.rs", default));
        let sized = args(&["out.png", "dir", "--size", "1000x600"]);
        let (_, _, got) = render_args(&sized, default, "usage").unwrap();
        assert_eq!(got, size(px(1000.), px(600.)));
        for bad in [
            &["out.png"][..],
            &["a", "b", "--size"],
            &["a", "b", "--size", "10"],
        ] {
            assert!(
                render_args(&args(bad), default, "usage").is_err(),
                "{bad:?}"
            );
        }
    }
}
