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
//! audit looks at, dark (Yohen) and light (Hakuji), as
//! `<out-dir>/<surface>_<dark|light>.png`, and the chrome under every bundled
//! theme as `<out-dir>/<theme id>.png`, and checks each capture's token probe
//! points (`audit_scenes`, `docs/decisions/25w38k498b-ui-polish-audit.md`).
//! The comparing run doesn't render them.
//!
//! Every mode runs isolated from the user's state ([`isolate`]): the example
//! re-executes itself with Tenmoku's config, state and cache directories on a
//! fresh temporary folder, so no real settings or keymap show and the demo
//! folders the scenes open are recorded in a throwaway `recent.toml`.

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
    match isolate() {
        Ok(None) => scenes::main(),
        Ok(Some(code)) => code,
        Err(error) => {
            eprintln!("screenshots: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() -> ExitCode {
    println!("skipped: the goldens are rendered with Metal, on macOS only");
    ExitCode::SUCCESS
}

/// The variable that marks the child run: the temporary folder whose
/// `config`, `state` and `cache` the other variables point at.
#[cfg(target_os = "macos")]
const ISOLATION_ROOT: &str = "TENMOKU_SCREENSHOTS_ROOT";

/// What the child's directories are, under [`ISOLATION_ROOT`]: the variable
/// and its folder.
#[cfg(target_os = "macos")]
const ISOLATED: [(&str, &str); 3] = [
    ("TENMOKU_CONFIG_DIR", "config"),
    ("TENMOKU_STATE_DIR", "state"),
    ("XDG_CACHE_HOME", "cache"),
];

/// In the parent, runs this example again with the same arguments and its
/// directories on a fresh temporary folder, whatever the environment
/// already says, and returns the child's exit code. In the child, returns
/// `None` once [`isolated`] holds, so the scenes can run, and fails if it
/// doesn't.
#[cfg(target_os = "macos")]
fn isolate() -> kiln_platform::screenshots::Result<Option<ExitCode>> {
    if let Some(root) = std::env::var_os(ISOLATION_ROOT) {
        return isolated(std::path::Path::new(&root)).map(|()| None);
    }
    let root = tempfile::tempdir()?;
    let mut child = std::process::Command::new(std::env::current_exe()?);
    child
        .args(std::env::args_os().skip(1))
        .env(ISOLATION_ROOT, root.path());
    // `audit --pseudo`: the pseudo-locale's one way in is its variable, so
    // the child gets it before anything negotiates a language.
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "audit") && args.iter().any(|a| a == "--pseudo") {
        child.env(kiln_platform::locale::PSEUDO_ENV, "1");
    }
    for (var, dir) in ISOLATED {
        let dir = root.path().join(dir);
        std::fs::create_dir_all(&dir)?;
        child.env(var, dir);
    }
    let status = child.status()?;
    Ok(Some(match status.code() {
        Some(0) => ExitCode::SUCCESS,
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        None => ExitCode::FAILURE,
    }))
}

/// Fails unless every directory Tenmoku reads or records in is the
/// parent's temporary one under `root`: a child started some other way,
/// say with `TENMOKU_SCREENSHOTS_ROOT` set by hand, would touch the user's.
#[cfg(target_os = "macos")]
fn isolated(root: &std::path::Path) -> kiln_platform::screenshots::Result<()> {
    for (var, dir) in ISOLATED {
        let expected = root.join(dir);
        if std::env::var_os(var).as_deref() != Some(expected.as_os_str()) {
            return Err(format!("{var} isn't {}: not starting", expected.display()).into());
        }
    }
    let paths = fe_config::paths::app_paths();
    for (got, dir) in [
        (paths.config_dir(), "config"),
        (paths.state_dir(), "state"),
        (paths.cache_dir(), "cache"),
    ] {
        if !got.is_some_and(|got| got.starts_with(root.join(dir))) {
            return Err(format!("Tenmoku's {dir} directory is {got:?}: not starting").into());
        }
    }
    Ok(())
}
