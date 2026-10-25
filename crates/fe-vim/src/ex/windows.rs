//! The window commands (`:help windows`), which the host runs over its
//! panes as [`HostCommand::Window`]: fe-vim knows no windows, so it only
//! reads the line, as nvim 0.12 reads it.
//!
//! - `:[N]sp[lit][!] [file]` and `:[N]vs[plit][!] [file]`: the current
//!   window split, showing `file`, or else what it shows.
//! - `:[N]new[!] [file]` and `:[N]vne[w][!] [file]`: a window split off
//!   with a new empty buffer, or with `file` as `:sp {file}` shows it.
//! - `:on[ly][!]`: every other window closed.
//! - `:clo[se][!]`: the current window closed, unless it's the last.
//!
//! A count before `:sp`, `:vs`, `:new` and `:vne` is the new window's
//! height or width, which the host's panes don't take: it's accepted and
//! ignored. One before `:on` and `:clo` (or after, `:on 2`) names another
//! window by number, which isn't supported. The file is the rest of the
//! line, trimmed, a backslash escaping a blank; `"` starts a comment. The
//! `!` of `:sp` and `:new` is the one `:edit` takes, for a buffer the
//! window leaves, and a split leaves none: it's ignored.
//!
//! Not supported: `++opt` and `+cmd` before the file; and a `|` and a
//! command after one, which nvim runs in the window the command leaves
//! current, but here would run in the one it was typed in, as the host
//! runs it once the line has run: the line is refused before any of it
//! runs. One before (`:2 | sp`) runs first, as in nvim. For the same
//! reason one run by `:g` or `:normal` is refused as it runs.

use super::{Address, ExCommand, ExError, HostCommand, trim_unescaped_end, uncommented, unescape};

/// A window command, for the host to run over its windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    /// `:sp[lit]` (`vertical` false) and `:vs[plit]`: the current window
    /// split, the new one showing `file`, or with none the same buffer.
    /// `:new {file}` and `:vne[w] {file}` are these too.
    Split {
        vertical: bool,
        file: Option<String>,
    },
    /// `:new` and `:vne[w]`: a window split off with a new empty buffer.
    New { vertical: bool },
    /// `:on[ly]`: every window but the current closed; `force` (`!`)
    /// closes those with changes too, keeping their buffers.
    Only { force: bool },
    /// `:clo[se]`: the current window closed, E444 for the last; `force`
    /// (`!`) closes one with changes, keeping its buffer.
    Close { force: bool },
}

/// Which window command a name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Split,
    VerticalSplit,
    New,
    VerticalNew,
    Only,
    Close,
}

/// The window command `name` abbreviates, by nvim's shortest forms
/// (`fullcommand()`): `:sp`, `:vs`, `:new`, `:vne`, `:on`, `:clo`.
pub(super) fn kind(name: &str) -> Option<Kind> {
    let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name);
    Some(match name {
        _ if is("split", 2) => Kind::Split,
        _ if is("vsplit", 2) => Kind::VerticalSplit,
        _ if is("new", 3) => Kind::New,
        _ if is("vnew", 3) => Kind::VerticalNew,
        _ if is("only", 2) => Kind::Only,
        _ if is("close", 3) => Kind::Close,
        _ => return None,
    })
}

/// The command `kind` names, with `range` before it and `after` after it.
pub(super) fn parse(kind: Kind, range: &[Address], after: &str) -> Result<ExCommand, ExError> {
    let (force, args) = match after.strip_prefix('!') {
        Some(args) => (true, args),
        None => (false, after),
    };
    let args = uncommented(args).trim_start_matches([' ', '\t']);
    let args = trim_unescaped_end(args);
    // The host runs the command once the line has, so one after it would
    // run in the window it was typed in.
    if args.contains('|') {
        return Err(ExError::Unsupported("| after a window command".into()));
    }
    let numbered = || ExError::Unsupported("a window number".into());
    let command = match kind {
        Kind::Only | Kind::Close if !range.is_empty() => return Err(numbered()),
        Kind::Only | Kind::Close if args.bytes().all(|byte| byte.is_ascii_digit()) => {
            if !args.is_empty() {
                return Err(numbered());
            }
            match kind {
                Kind::Only => WindowCommand::Only { force },
                _ => WindowCommand::Close { force },
            }
        }
        Kind::Only | Kind::Close => return Err(ExError::Trailing(args.to_string())),
        // The range, the new window's size, is ignored: panes take none.
        _ if args.starts_with('+') => return Err(ExError::Unsupported("+cmd and ++opt".into())),
        _ => {
            let vertical = matches!(kind, Kind::VerticalSplit | Kind::VerticalNew);
            match (kind, args) {
                (Kind::New | Kind::VerticalNew, "") => WindowCommand::New { vertical },
                (_, "") => WindowCommand::Split {
                    vertical,
                    file: None,
                },
                (_, file) => WindowCommand::Split {
                    vertical,
                    file: Some(unescape(file)),
                },
            }
        }
    };
    Ok(ExCommand::Host(HostCommand::Window(command)))
}
