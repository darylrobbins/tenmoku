//! The buffer list's commands (`:help buffer-list`), which the host runs
//! over its open files as [`HostCommand::Buffer`]: fe-vim knows no buffers
//! but its own, so it only reads the line, as nvim 0.12 reads it.
//!
//! - `:b[uffer][!] [N|name|#|%]` and `:[N]b`: the buffer shown in the
//!   current window; with nothing, the current one (nothing changes).
//! - `:[N]bn[ext][!] [N]`, `:[N]bN[ext][!] [N]` and `:[N]bp[revious][!]
//!   [N]`: the `N`th listed buffer after (before) the current, wrapping.
//! - `:ls[!]`, `:buffers[!]` and `:files[!]`: the open files in a picker
//!   (`Pick::buffers`), a row switching to it as `:b N` does.
//! - `:bf[irst][!]`, `:br[ewind][!]` and `:bl[ast][!]`: the first or last
//!   listed buffer.
//! - `:[N]bd[elete][!] [N|name|#|%]`, `:{N,M}bd` and `:%bd`: the buffers
//!   deleted from the list, refused for a modified one without `!`.
//!
//! A number argument is `N` alone (0 is E939; past 32 bits it is the
//! largest, as nvim reads it); digits with more after them are a name
//! (`:b 1x`), but for `:b 2 3`, E488. A name is the rest of the line,
//! trimmed. A range is buffer numbers, not lines: `N`, `.` (the current
//! buffer), `$` (the last) and `%` (all), with no offsets; `:b` takes its
//! last, `:bn` and `:bp` read a number there as their count (0 as 1), and
//! `:bf` and `:bl` ignore it. `"` starts a comment. nvim gives none of
//! them E471: with no argument each acts on the current buffer.
//!
//! Not supported: `+cmd` before the argument, and `:bd` of several buffers
//! by argument (`:bd 2 3`, `:bd a b`); offsets in a range (`:.+1b`); and a
//! `|` and a command after one (`:bn | bn`, `:b 2 | d`), which nvim runs in
//! the buffer switched to, but here would run in the one left, as the host
//! switches once the line has run: the line is refused before any of it
//! runs. One before (`:2 | bn`) runs first, as in nvim. For the same
//! reason one run by `:g` or `:normal` is refused as it runs, which stops
//! them there.

use super::{Address, Base, ExCommand, ExError, HostCommand, uncommented};

/// A buffer command, for the host to run over its buffers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BufferCommand {
    /// `:b[uffer]`: `target` shown in the current window.
    Switch(BufferTarget),
    /// `:bn[ext]`: the count's listed buffer after the current, wrapping.
    Next(u32),
    /// `:bN[ext]` and `:bp[revious]`: the count's listed buffer before the
    /// current, wrapping.
    Previous(u32),
    /// `:bd[elete]`: `target` deleted from the list, its windows closed;
    /// `force` (`!`) discards its changes, which are otherwise E89.
    Delete { target: BufferTarget, force: bool },
    /// `:bf[irst]` and `:br[ewind]`: the first listed buffer.
    First,
    /// `:bl[ast]`: the last listed buffer.
    Last,
    /// `:ls`, `:buffers` and `:files` (with `!` or not): the host lists its
    /// open files in a picker ([`super::Pick::buffers`]).
    List,
}

/// The buffer a [`BufferCommand`] names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BufferTarget {
    /// No argument (or `%`): the current buffer.
    Current,
    /// `#`: the alternate buffer, E23 with none.
    Alternate,
    /// `N`, as the argument: E86 if no buffer has that number.
    Number(u32),
    /// The one buffer whose name matches, E93 for several and E94 for none.
    Name(String),
    /// The range's buffers (`:3b`, `:2,3bd`, `:%bd`), `first` to `last`:
    /// E16 past the last buffer. `:b`'s is its last address alone.
    Range {
        first: BufferAddress,
        last: BufferAddress,
    },
}

/// A buffer number as a range gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferAddress {
    /// `N`, from 1.
    Number(u32),
    /// `.`: the current buffer.
    Current,
    /// `$`: the last buffer.
    Last,
}

/// Which buffer command a name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Buffer,
    Next,
    Previous,
    Delete,
    First,
    Last,
    List,
}

/// The buffer command `name` abbreviates, by nvim's shortest forms
/// (`fullcommand()`): `:b`, `:bn`, `:bN`, `:bp`, `:bd`, `:bf`, `:br`,
/// `:bl`.
pub(super) fn kind(name: &str) -> Option<Kind> {
    let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name);
    Some(match name {
        _ if is("buffer", 1) => Kind::Buffer,
        _ if is("bnext", 2) => Kind::Next,
        _ if is("bNext", 2) || is("bprevious", 2) => Kind::Previous,
        _ if is("bdelete", 2) => Kind::Delete,
        _ if is("bfirst", 2) || is("brewind", 2) => Kind::First,
        _ if is("blast", 2) => Kind::Last,
        "ls" | "buffers" | "files" => Kind::List,
        _ => return None,
    })
}

/// The command `kind` names, with `range` before it and `after` after it.
pub(super) fn parse(kind: Kind, range: &[Address], after: &str) -> Result<ExCommand, ExError> {
    let (force, args) = match after.strip_prefix('!') {
        Some(args) => (true, args),
        None => (false, after),
    };
    let args = uncommented(args).trim_matches([' ', '\t']);
    // The host switches buffers once the line has run, so a command after
    // one would run in the buffer it leaves: refused before any runs.
    if args.contains('|') {
        return Err(ExError::Unsupported("| after a buffer command".into()));
    }
    if args.starts_with('+') {
        return Err(ExError::Unsupported("+cmd".into()));
    }
    let command = match kind {
        Kind::List => {
            if !range.is_empty() {
                return Err(ExError::NoRange);
            }
            // Flags (`:ls u`) filter nvim's list; the picker lists every
            // open file, as `:ls!` does.
            if !args.is_empty() {
                return Err(ExError::Unsupported(":ls flags".into()));
            }
            BufferCommand::List
        }
        Kind::Buffer => {
            let target = match args {
                "" => match range.last() {
                    Some(address) => {
                        let at = self::address(address)?;
                        BufferTarget::Range {
                            first: at,
                            last: at,
                        }
                    }
                    None => BufferTarget::Current,
                },
                args => target(args, false)?,
            };
            BufferCommand::Switch(target)
        }
        Kind::Next | Kind::Previous => {
            let count = match args {
                "" => range_count(range)?,
                args => count(args)?,
            };
            match kind {
                Kind::Next => BufferCommand::Next(count),
                _ => BufferCommand::Previous(count),
            }
        }
        Kind::First | Kind::Last if !args.is_empty() => {
            return Err(ExError::Trailing(args.to_string()));
        }
        Kind::First => BufferCommand::First,
        Kind::Last => BufferCommand::Last,
        Kind::Delete => {
            let target = match (args, range) {
                ("", []) => BufferTarget::Current,
                ("", [address]) if address.base == Base::All => BufferTarget::Range {
                    first: BufferAddress::Number(1),
                    last: BufferAddress::Last,
                },
                ("", [.., first, last]) => BufferTarget::Range {
                    first: self::address(first)?,
                    last: self::address(last)?,
                },
                ("", [one]) => {
                    let at = self::address(one)?;
                    BufferTarget::Range {
                        first: at,
                        last: at,
                    }
                }
                (args, _) => target(args, true)?,
            };
            BufferCommand::Delete { target, force }
        }
    };
    Ok(ExCommand::Host(HostCommand::Buffer(command)))
}

/// The buffer `args`, a whole argument, names; `delete` for `:bd`, which
/// would read several there.
fn target(args: &str, delete: bool) -> Result<BufferTarget, ExError> {
    match args {
        "#" => return Ok(BufferTarget::Alternate),
        "%" => return Ok(BufferTarget::Current),
        _ => {}
    }
    let several = || ExError::Unsupported(":bd of several buffers".into());
    let digits = args
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(args.len());
    let (number, rest) = args.split_at(digits);
    if digits > 0 && (rest.is_empty() || rest.starts_with([' ', '\t'])) {
        let rest = rest.trim_start_matches([' ', '\t']);
        return match rest {
            "" => Ok(BufferTarget::Number(positive(number)?)),
            _ if delete => Err(several()),
            rest => Err(ExError::Trailing(rest.to_string())),
        };
    }
    if delete && args.contains([' ', '\t']) {
        return Err(several());
    }
    Ok(BufferTarget::Name(args.to_string()))
}

/// `:bn`'s and `:bp`'s count argument, `args`.
fn count(args: &str) -> Result<u32, ExError> {
    let digits = args
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(args.len());
    let (number, rest) = args.split_at(digits);
    match rest.trim_start_matches([' ', '\t']) {
        "" => positive(number),
        rest => Err(ExError::Trailing(rest.to_string())),
    }
}

/// `:bn`'s and `:bp`'s count from `range`, its last address: 1 with none,
/// and for 0.
fn range_count(range: &[Address]) -> Result<u32, ExError> {
    let Some(address) = range.last() else {
        return Ok(1);
    };
    if !address.parts.is_empty() {
        return Err(offsets());
    }
    match address.base {
        Base::Line(line) => Ok(u32::try_from(line).unwrap_or(0).max(1)),
        _ => Err(ExError::Unsupported("a count other than a number".into())),
    }
}

/// A buffer number written as `digits`: E939 for 0, and the largest past 32
/// bits, as nvim reads it.
fn positive(digits: &str) -> Result<u32, ExError> {
    let max = i32::MAX as u32;
    let number = super::number(digits).map_or(max, |n| n.min(i64::from(max)) as u32);
    match number {
        0 => Err(ExError::ZeroCount),
        number => Ok(number),
    }
}

/// A buffer address in a range: E16 for 0 or before it.
fn address(address: &Address) -> Result<BufferAddress, ExError> {
    if !address.parts.is_empty() {
        return Err(offsets());
    }
    match address.base {
        Base::Line(line) if line < 1 => Err(ExError::InvalidRange),
        Base::Line(line) => Ok(BufferAddress::Number(line as u32)),
        Base::Current => Ok(BufferAddress::Current),
        Base::Last | Base::All => Ok(BufferAddress::Last),
        Base::Mark(_) => Err(ExError::Unsupported("a mark as a buffer number".into())),
    }
}

fn offsets() -> ExError {
    ExError::Unsupported("an offset in a buffer range".into())
}
