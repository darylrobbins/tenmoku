//! The `tenmoku` command line: flags, and paths with an optional
//! `:line[:column]`, made absolute against the working directory.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// `--help`'s text, and what an unknown flag prints to stderr.
const USAGE: &str = "\
Tenmoku, a code editor.

Usage: tenmoku [options] [--] [path[:line[:column]] ...]

Each path opens as a tab, in order, the last one active; a path that doesn't
exist is a new file, which saving creates. The first folder is the project
root, or with no folder, the first file's folder. Lines and columns count
from 1. A file really named a:1 opens as itself.

Options:
      --new-window  Open in a new window (accepted; not acted on yet)
      --wait        Wait for the files to close (accepted; not acted on yet)
      --perf        Measure startup and scrolling on the file, print, and quit
  -h, --help        Print this help
  -V, --version     Print the version
      --            Treat everything after it as a path
";

/// A path to open, and where, 1-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) path: PathBuf,
    pub(crate) line: Option<u32>,
    pub(crate) col: Option<u32>,
}

/// A parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Cli {
    pub(crate) perf: bool,
    pub(crate) new_window: bool,
    pub(crate) wait: bool,
    pub(crate) targets: Vec<Target>,
}

/// A command line that ends the process instead of launching.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CliError {
    Help,
    Version,
    UnknownFlag(String),
}

impl CliError {
    /// The process's exit code.
    pub(crate) fn exit_code(&self) -> i32 {
        match self {
            CliError::Help | CliError::Version => 0,
            CliError::UnknownFlag(_) => 2,
        }
    }

    /// What to print: help and the version on stdout, an error on stderr.
    fn text(&self) -> String {
        match self {
            CliError::Help => USAGE.to_owned(),
            CliError::Version => format!("tenmoku {}\n", env!("CARGO_PKG_VERSION")),
            CliError::UnknownFlag(flag) => format!("tenmoku: unknown flag {flag}\n\n{USAGE}"),
        }
    }

    pub(crate) fn print(&self) {
        match self {
            CliError::Help | CliError::Version => print!("{}", self.text()),
            CliError::UnknownFlag(_) => eprint!("{}", self.text()),
        }
    }
}

/// What the targets launch: the project root and the files, in order.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) root: Option<PathBuf>,
    pub(crate) files: Vec<Target>,
}

impl Cli {
    /// The first folder is the root, and any other is left out; everything
    /// else is a file, a missing path being a new one. With only files, the
    /// root is the first one's parent.
    pub(crate) fn plan(self) -> Plan {
        let mut root = None;
        let mut files = Vec::new();
        for target in self.targets {
            if !target.path.is_dir() {
                files.push(target);
            } else if root.is_none() {
                root = Some(target.path);
            } else {
                log::warn!(target: "fastedit", "not opening {}: the first folder is the root", target.path.display());
            }
        }
        let root = root.or_else(|| {
            let first = files.first()?;
            first.path.parent().map(Path::to_owned)
        });
        Plan { root, files }
    }
}

/// Parses `args`, without the program's name, against the working directory.
pub(crate) fn parse(args: Vec<OsString>) -> Result<Cli, CliError> {
    parse_in(args, &std::env::current_dir().unwrap_or_default())
}

/// [`parse`], with `cwd` the working directory.
fn parse_in(mut args: Vec<OsString>, cwd: &Path) -> Result<Cli, CliError> {
    // pico-args has no notion of `--`: what follows it is never looked at.
    let literal = match args.iter().position(|arg| arg == "--") {
        Some(at) => {
            let literal = args.split_off(at + 1);
            args.pop();
            literal
        }
        None => Vec::new(),
    };
    let mut args = pico_args::Arguments::from_vec(args);
    if flag(&mut args, ["-h", "--help"]) {
        return Err(CliError::Help);
    }
    if flag(&mut args, ["-V", "--version"]) {
        return Err(CliError::Version);
    }
    let perf = flag(&mut args, "--perf");
    let new_window = flag(&mut args, "--new-window");
    let wait = flag(&mut args, "--wait");
    let paths = args.finish();
    // A lone `-` is no flag, here a file of that name.
    let unknown = paths
        .iter()
        .find(|arg| arg.as_encoded_bytes().starts_with(b"-") && arg.len() > 1);
    if let Some(unknown) = unknown {
        return Err(CliError::UnknownFlag(
            unknown.to_string_lossy().into_owned(),
        ));
    }
    let targets = (paths.iter().chain(&literal))
        .map(|arg| target(arg, cwd))
        .collect();
    Ok(Cli {
        perf,
        new_window,
        wait,
        targets,
    })
}

/// Whether `keys` was given; each `contains` takes only one of a repeat.
fn flag(args: &mut pico_args::Arguments, keys: impl Into<pico_args::Keys> + Copy) -> bool {
    let mut given = false;
    while args.contains(keys) {
        given = true;
    }
    given
}

/// `arg` as a path and position. The whole string wins if it exists, then
/// the string less its last `:number` (a file `a:1` at `a:1:2`). Otherwise
/// up to two such suffixes are the position, of a file that may be new.
/// Anything else after a colon is part of the name.
fn target(arg: &OsStr, cwd: &Path) -> Target {
    let at = |path: &OsStr, line, col| Target {
        path: absolute(path, cwd),
        line,
        col,
    };
    let exists = |path: &OsStr| absolute(path, cwd).symlink_metadata().is_ok();
    if exists(arg) {
        return at(arg, None, None);
    }
    let Some((rest, last)) = number_suffix(arg) else {
        return at(arg, None, None);
    };
    match number_suffix(rest).filter(|_| !exists(rest)) {
        Some((path, line)) => at(path, Some(line), Some(last)),
        None => at(rest, Some(last), None),
    }
}

/// `arg` split at a final `:number`, where there's something before it. The
/// number is 1 or more, 0 clamping up and one too big to hold down.
fn number_suffix(arg: &OsStr) -> Option<(&OsStr, u32)> {
    let bytes = arg.as_encoded_bytes();
    let colon = bytes.iter().rposition(|byte| *byte == b':')?;
    let digits = std::str::from_utf8(&bytes[colon + 1..]).ok()?;
    if colon == 0 || digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // SAFETY: split just before an ASCII `:`, from an `OsStr`'s own bytes,
    // which is a boundary `from_encoded_bytes_unchecked` allows.
    let rest = unsafe { OsStr::from_encoded_bytes_unchecked(&bytes[..colon]) };
    Some((rest, digits.parse().unwrap_or(u32::MAX).max(1)))
}

/// `path` against `cwd`, tidied as [`std::path::absolute`] does: no symlink
/// is resolved, so a tab shows the path as given.
fn absolute(path: &OsStr, cwd: &Path) -> PathBuf {
    let joined = cwd.join(path);
    std::path::absolute(&joined).unwrap_or(joined)
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
