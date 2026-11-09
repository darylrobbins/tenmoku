//! The tab page commands (`:help tabpage`), which the host runs over its
//! editor tabs as [`HostCommand::Tab`], as VSCodeVim maps them: a
//! fastedit tab stands in for a buffer in a tab page, and real tab pages
//! (a window layout each) aren't modelled. fe-vim only reads the line, as
//! nvim 0.12 reads it.
//!
//! - `:tabnew [file]` and `:tabe[dit] [file]`: a new tab, showing `file`
//!   as `:e` reads it, else an untitled one.
//! - `:tabn[ext] [N]` and `:Ntabn[ext]`: tab `N`, 1-based, else the next.
//! - `:tabp[revious] [N]`, `:tabN[ext] [N]` and the `:N` forms: the `N`th
//!   tab before the current, 1 by default.
//! - `:tabc[lose][!]` and `:tabo[nly][!]`: the current tab closed, or
//!   every other.
//! - `:tabfir[st]` and `:tabr[ewind]`, `:tabl[ast]`: the ends.
//!
//! The next and previous tabs wrap, which nvim's `:tabnext` does and
//! `:tabprevious` does too. A count is `N` alone or the range before the
//! command (0 is E939 for the former; a number past 32 bits is the
//! largest). `:tabclose N`, a range before `:tabclose` and `:tabonly`, and
//! a range before `:tabnew` name another tab and aren't supported;
//! `:tabmove`, `:tabdo`, `:tabs` and `:tabfind` aren't commands here. The
//! file is the rest of the line, trimmed, a backslash escaping a blank;
//! `"` starts a comment.
//!
//! Not supported: `+cmd` before the file, and a `|` and a command after
//! one, which would run in the tab left, as the host runs the command
//! once the line has: the line is refused before any of it runs.

use kiln_l10n::tr;

use super::{
    Address, Base, ExCommand, ExError, HostCommand, trim_unescaped_end, uncommented, unescape,
};

/// A tab command, for the host to run over its tabs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TabCommand {
    /// `:tabnew` and `:tabedit`: a tab after the current, showing `file`.
    New { file: Option<String> },
    /// `:tabn[ext]`: the tab numbered `N` (from 1), or with none the next,
    /// wrapping.
    Next(Option<u32>),
    /// `:tabp[revious]` and `:tabN[ext]`: the `N`th tab before the
    /// current, wrapping.
    Previous(u32),
    /// `:tabc[lose]`: the current tab closed; `force` (`!`) discards its
    /// changes.
    Close { force: bool },
    /// `:tabo[nly]`: every other tab closed; `force` (`!`) closes those
    /// with changes too.
    Only { force: bool },
    /// `:tabfir[st]` and `:tabr[ewind]`: the first tab.
    First,
    /// `:tabl[ast]`: the last tab.
    Last,
}

/// Which tab command a name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    New,
    Next,
    Previous,
    Close,
    Only,
    First,
    Last,
}

/// The tab command `name` abbreviates, by nvim's shortest forms
/// (`fullcommand()`): `:tabnew` (`:tabne` is `:tabnext`), `:tabn`,
/// `:tabN`, `:tabp`, `:tabc`, `:tabo`, `:tabfir` (`:tabf` is `:tabfind`),
/// `:tabr`, `:tabl`, `:tabe`.
pub(super) fn kind(name: &str) -> Option<Kind> {
    let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name);
    Some(match name {
        "tabnew" => Kind::New,
        _ if is("tabedit", 4) => Kind::New,
        _ if is("tabnext", 4) => Kind::Next,
        _ if is("tabNext", 4) || is("tabprevious", 4) => Kind::Previous,
        _ if is("tabclose", 4) => Kind::Close,
        _ if is("tabonly", 4) => Kind::Only,
        _ if is("tabfirst", 6) || is("tabrewind", 4) => Kind::First,
        _ if is("tablast", 4) => Kind::Last,
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
    if args.contains('|') {
        return Err(ExError::Unsupported(
            tr!("vim-unsupported-bar-after-tab-command").into(),
        ));
    }
    let command = match kind {
        Kind::Next | Kind::Previous => {
            let count = match (args, range) {
                ("", []) => None,
                ("", range) => Some(range_count(range)?),
                (args, []) => Some(count(args)?),
                (args, _) => return Err(ExError::Trailing(args.to_string())),
            };
            match kind {
                Kind::Next => TabCommand::Next(count),
                _ => TabCommand::Previous(count.unwrap_or(1)),
            }
        }
        _ if !range.is_empty() => {
            return Err(ExError::Unsupported(
                tr!("vim-unsupported-tab-number").into(),
            ));
        }
        Kind::New => {
            if args.starts_with('+') {
                return Err(ExError::Unsupported(
                    tr!("vim-unsupported-cmd-and-opt").into(),
                ));
            }
            let file = (!args.is_empty()).then(|| unescape(args));
            TabCommand::New { file }
        }
        _ if !args.is_empty() => {
            return Err(match args.bytes().all(|byte| byte.is_ascii_digit()) {
                true => ExError::Unsupported(tr!("vim-unsupported-tab-number").into()),
                false => ExError::Trailing(args.to_string()),
            });
        }
        Kind::Close => TabCommand::Close { force },
        Kind::Only => TabCommand::Only { force },
        Kind::First => TabCommand::First,
        Kind::Last => TabCommand::Last,
    };
    Ok(ExCommand::Host(HostCommand::Tab(command)))
}

/// `:tabn`'s and `:tabp`'s count argument, `args`.
fn count(args: &str) -> Result<u32, ExError> {
    let digits = args
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(args.len());
    let (number, rest) = args.split_at(digits);
    match rest.trim_start_matches([' ', '\t']) {
        "" if digits > 0 => positive(number),
        "" => Err(ExError::Trailing(args.to_string())),
        rest => Err(ExError::Trailing(rest.to_string())),
    }
}

/// The count from `range`, its last address.
fn range_count(range: &[Address]) -> Result<u32, ExError> {
    let Some(address) = range.last() else {
        return Err(ExError::ZeroCount);
    };
    if !address.parts.is_empty() {
        return Err(ExError::Unsupported(
            tr!("vim-unsupported-tab-offset").into(),
        ));
    }
    match address.base {
        Base::Line(line) => u32::try_from(line)
            .ok()
            .filter(|n| *n > 0)
            .ok_or(ExError::ZeroCount),
        _ => Err(ExError::Unsupported(
            tr!("vim-unsupported-tab-non-number").into(),
        )),
    }
}

/// A count written as `digits`: E939 for 0, and the largest past 32 bits.
fn positive(digits: &str) -> Result<u32, ExError> {
    let max = i32::MAX as u32;
    let number = super::number(digits).map_or(max, |n| n.min(i64::from(max)) as u32);
    match number {
        0 => Err(ExError::ZeroCount),
        number => Ok(number),
    }
}
