//! The quickfix and location lists' commands (`:help quickfix`), which the
//! host runs over its lists as [`HostCommand::Quickfix`]: fe-vim keeps no
//! lists, so it only reads the line, as nvim 0.12 reads it. Each `:c` form
//! has an `:l` twin for the window's location list
//! ([`QuickfixCommand::location`]).
//!
//! - `:[N]cn[ext][!] [N]` and `:[N]lne[xt][!] [N]`: the `N`th item after
//!   the current; `:[N]cp[revious][!] [N]`, `:[N]cN[ext][!] [N]`,
//!   `:[N]lp[revious][!] [N]` and `:[N]lN[ext][!] [N]` before it.
//! - `:[N]cnf[ile][!] [N]` and `:lnf[ile]`: the first item of the `N`th
//!   file after the current item's; `:[N]cNf[ile][!] [N]`, `:cpf[ile]`,
//!   `:lNf[ile]` and `:lpf[ile]` the last item of the `N`th file before.
//! - `:cc[!] [N]` and `:ll[!] [N]`: item `N`, or with none the current one
//!   again; `:cfir[st][!]`, `:cr[ewind][!]`, `:lfir[st]` and `:lr[ewind]`
//!   the first, `:cla[st][!]` and `:lla[st]` the last, and with a number
//!   (`:cfir 3`, `:3cla`) item `N`, as `:cc N`.
//! - `:[N]col[der] [N]`, `:[N]cnew[er] [N]`, `:lol[der]` and `:lnew[er]`:
//!   the list `N` older (newer) in the stack of lists.
//! - `:cl[ist]` and `:lli[st]`: the items, in the status line.
//! - `:gr[ep]`, `:lgr[ep]`, `:vim[grep]`, `:lv[imgrep]` (each needing an
//!   argument, E471 without) and `:mak[e]`, `:lmak[e]`, with the rest of
//!   the line as it was typed: the commands that fill a list. The host
//!   splits `:vimgrep`'s with [`QuickfixAction::parse_vimgrep`].
//!
//! - `:[N]cope[n] [N]`, `:cw[indow] [N]`, `:lop[en]` and `:lw[indow]`:
//!   the list's window shown and focused ([`QuickfixAction::Open`]); the
//!   host has a panel in its place, so the height is read and dropped, and
//!   any range is accepted, as nvim accepts one. `:cwindow` is `:copen`
//!   here, as the panel shows an empty list as one with items.
//! - `:ccl[ose]` and `:lcl[ose]`: the window hidden, if shown
//!   ([`QuickfixAction::Close`]), with no range or argument.
//!
//! Neither takes a `!` (E477), unlike the commands that move.
//!
//! A count is `N` after the name or a range of one number before it, 1
//! with neither (and for a range of 0, as nvim moves one item then); with
//! both, the range's number and the count less one, as nvim adds them. A
//! count of 0 is E939, and past 32 bits it is the largest. Any other range
//! (`:%cn`, `:.cn`, `:1,2cn`, `:+cn`, `:'acn`) is E481, as is one before
//! `:cl` or a filling command; `:0cc` is E16, as in nvim. `!` is accepted
//! and ignored, as nvim does for these. `"` starts a comment but for the
//! filling commands, whose argument may hold one.
//!
//! Not supported: `:cl`'s `[from] [, [to]]` and `+N`, and a `|` and a
//! command after a navigating one (`:cn | d`), which nvim runs in the file
//! landed in, but here would run in the one left, as the host lands once
//! the line has run: the line is refused before any of it runs, and so is
//! one run by `:g` or `:normal`, as the buffer commands are
//! (`ex/buffers.rs`). A filling command's argument is the rest of the line,
//! `|` and all.

use super::{Address, Base, ExCommand, ExError, HostCommand, uncommented};

/// A quickfix or location list command, for the host to run over its
/// lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickfixCommand {
    pub action: QuickfixAction,
    /// The `:l` forms: the window's location list, not the quickfix list.
    pub location: bool,
}

/// What a [`QuickfixCommand`] does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuickfixAction {
    /// `:cn[ext]`: the count's item after the current.
    Next(u32),
    /// `:cp[revious]` and `:cN[ext]`: the count's item before the current.
    Previous(u32),
    /// `:cnf[ile]`: the first item of the count's file after the current
    /// item's.
    NextFile(u32),
    /// `:cNf[ile]` and `:cpf[ile]`: the last item of the count's file
    /// before the current item's.
    PreviousFile(u32),
    /// `:cc [N]`: item `N`, from 1 (past the last, the last); with none,
    /// the current one again.
    Go(Option<u32>),
    /// `:cfir[st]` and `:cr[ewind]` with no number: the first item.
    First,
    /// `:cla[st]` with no number: the last item.
    Last,
    /// `:col[der]`: the list the count older in the stack.
    Older(u32),
    /// `:cnew[er]`: the list the count newer in the stack.
    Newer(u32),
    /// `:cl[ist]`: the items listed.
    List,
    /// `:gr[ep] {args}`: `args` as typed.
    Grep(String),
    /// `:vim[grep] {args}`: `args` as typed.
    VimGrep(String),
    /// `:mak[e] [args]`: `args` as typed.
    Make(String),
    /// `:cope[n]` and `:cw[indow]`: the list shown, and focused.
    Open,
    /// `:ccl[ose]`: the list hidden, if shown.
    Close,
}

/// Which quickfix command a name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Next,
    Previous,
    NextFile,
    PreviousFile,
    Go,
    First,
    Last,
    Older,
    Newer,
    List,
    Grep,
    VimGrep,
    Make,
    Open,
    Close,
}

/// The quickfix command `name` abbreviates, and whether it is an `:l`
/// form, by nvim's shortest forms (`fullcommand()`).
pub(super) fn kind(name: &str) -> Option<(Kind, bool)> {
    let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name);
    Some(match name {
        _ if is("cnext", 2) => (Kind::Next, false),
        _ if is("lnext", 3) => (Kind::Next, true),
        _ if is("cNext", 2) || is("cprevious", 2) => (Kind::Previous, false),
        _ if is("lNext", 2) || is("lprevious", 2) => (Kind::Previous, true),
        _ if is("cnfile", 3) => (Kind::NextFile, false),
        _ if is("lnfile", 3) => (Kind::NextFile, true),
        _ if is("cNfile", 3) || is("cpfile", 3) => (Kind::PreviousFile, false),
        _ if is("lNfile", 3) || is("lpfile", 3) => (Kind::PreviousFile, true),
        "cc" => (Kind::Go, false),
        "ll" => (Kind::Go, true),
        _ if is("cfirst", 4) || is("crewind", 2) => (Kind::First, false),
        _ if is("lfirst", 4) || is("lrewind", 2) => (Kind::First, true),
        _ if is("clast", 3) => (Kind::Last, false),
        _ if is("llast", 3) => (Kind::Last, true),
        _ if is("colder", 3) => (Kind::Older, false),
        _ if is("lolder", 3) => (Kind::Older, true),
        _ if is("cnewer", 4) => (Kind::Newer, false),
        _ if is("lnewer", 4) => (Kind::Newer, true),
        _ if is("clist", 2) => (Kind::List, false),
        _ if is("llist", 3) => (Kind::List, true),
        _ if is("grep", 2) => (Kind::Grep, false),
        _ if is("lgrep", 3) => (Kind::Grep, true),
        _ if is("vimgrep", 3) => (Kind::VimGrep, false),
        _ if is("lvimgrep", 2) => (Kind::VimGrep, true),
        _ if is("make", 3) => (Kind::Make, false),
        _ if is("lmake", 4) => (Kind::Make, true),
        _ if is("copen", 4) || is("cwindow", 2) => (Kind::Open, false),
        _ if is("lopen", 3) || is("lwindow", 2) => (Kind::Open, true),
        _ if is("cclose", 3) => (Kind::Close, false),
        _ if is("lclose", 3) => (Kind::Close, true),
        _ => return None,
    })
}

/// The command `kind` names (an `:l` form if `location`), with `range`
/// before it and `after` after it.
pub(super) fn parse(
    (kind, location): (Kind, bool),
    range: &[Address],
    after: &str,
) -> Result<ExCommand, ExError> {
    if matches!(kind, Kind::Open | Kind::Close) && after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    let after = after.strip_prefix('!').unwrap_or(after);
    let action = match kind {
        Kind::Grep | Kind::VimGrep | Kind::Make => {
            if !range.is_empty() {
                return Err(ExError::NoRange);
            }
            let args = after.trim_matches([' ', '\t']).to_string();
            match kind {
                _ if args.is_empty() && kind != Kind::Make => {
                    return Err(ExError::ArgumentRequired);
                }
                Kind::Grep => QuickfixAction::Grep(args),
                Kind::VimGrep => QuickfixAction::VimGrep(args),
                _ => QuickfixAction::Make(args),
            }
        }
        _ => {
            let args =
                super::trim_unescaped_end(uncommented(after).trim_start_matches([' ', '\t']));
            // The host lands once the line has run, so a command after one
            // would run in the file it leaves: refused before any runs.
            if args.contains('|') {
                return Err(ExError::Unsupported("| after a quickfix command".into()));
            }
            navigation(kind, range, args)?
        }
    };
    Ok(ExCommand::Host(HostCommand::Quickfix(QuickfixCommand {
        action,
        location,
    })))
}

/// A navigating, listing, opening or closing command `kind`, with `range`
/// before it and `args`, trimmed and uncommented, after it.
fn navigation(kind: Kind, range: &[Address], args: &str) -> Result<QuickfixAction, ExError> {
    match kind {
        // A height, checked as nvim checks it, for a window the host
        // hasn't: any range is one too.
        Kind::Open => return count(args).map(|_| QuickfixAction::Open),
        Kind::Close if !range.is_empty() => return Err(ExError::NoRange),
        Kind::Close if !args.is_empty() => return Err(ExError::Trailing(args.to_string())),
        Kind::Close => return Ok(QuickfixAction::Close),
        _ => {}
    }
    if kind == Kind::List {
        if !range.is_empty() {
            return Err(ExError::NoRange);
        }
        if !args.is_empty() {
            return Err(ExError::Unsupported(":clist's range".into()));
        }
        return Ok(QuickfixAction::List);
    }
    let before = range_number(range)?;
    if kind == Kind::Go && before == Some(0) {
        return Err(ExError::InvalidRange);
    }
    let after = count(args)?;
    // nvim adds a count after a range to its number, less one.
    let number = match (before, after) {
        (Some(before), Some(after)) => Some(before.max(1).saturating_add(after - 1)),
        (before, after) => after.or(before),
    };
    let count = number.unwrap_or(1).max(1);
    Ok(match kind {
        Kind::Next => QuickfixAction::Next(count),
        Kind::Previous => QuickfixAction::Previous(count),
        Kind::NextFile => QuickfixAction::NextFile(count),
        Kind::PreviousFile => QuickfixAction::PreviousFile(count),
        Kind::Older => QuickfixAction::Older(count),
        Kind::Newer => QuickfixAction::Newer(count),
        Kind::Go => QuickfixAction::Go(number),
        Kind::First | Kind::Last if number.is_some() => QuickfixAction::Go(number),
        Kind::First => QuickfixAction::First,
        _ => QuickfixAction::Last,
    })
}

/// The number a range of one number gives (`:3cn`), none with no range;
/// any other range is E481.
fn range_number(range: &[Address]) -> Result<Option<u32>, ExError> {
    match range {
        [] => Ok(None),
        [
            Address {
                base: Base::Line(line),
                parts,
                semicolon: false,
            },
        ] if parts.is_empty() && *line >= 0 => Ok(Some(*line as u32)),
        _ => Err(ExError::NoRange),
    }
}

/// The count `args` give, none if they're empty: digits, then only
/// blanks (E488 for more). 0 is E939, and past 32 bits it is the largest.
fn count(args: &str) -> Result<Option<u32>, ExError> {
    if args.is_empty() {
        return Ok(None);
    }
    let digits = args
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(args.len());
    let (number, rest) = args.split_at(digits);
    let rest = rest.trim_start_matches([' ', '\t']);
    if digits == 0 || !rest.is_empty() {
        let rest = if digits == 0 { args } else { rest };
        return Err(ExError::Trailing(rest.to_string()));
    }
    let max = i32::MAX as u32;
    match super::number(number).map_or(max, |n| n.min(i64::from(max)) as u32) {
        0 => Err(ExError::ZeroCount),
        count => Ok(Some(count)),
    }
}

impl QuickfixAction {
    /// `:vim[grep] {args}` (as [`QuickfixAction::VimGrep`] carries them)
    /// split as nvim splits them (`skip_vimgrep_pat()`): `(pattern, flags,
    /// globs)`.
    ///
    /// - `/{pattern}/[g][j] {glob}…`: the delimiter is the first char, any
    ///   but a word char (a letter, digit or `_`); the pattern runs to the
    ///   next one not escaped by `\` nor inside a `[…]` collection, and is
    ///   kept as typed, escapes and all. Then any of the flags `g` (every
    ///   match on a line, not only the first) and `j` (don't jump to the
    ///   first item), in the order typed.
    /// - `{pattern} {glob}…`, the pattern starting with a word char: the
    ///   pattern is the first word, with no flags.
    ///
    /// The globs are the rest, split at blanks not escaped by `\`, each as
    /// typed (`\ ` and all). As in nvim, nothing need come between the
    /// flags and the first glob: `/a/gx` searches `x`.
    ///
    /// A pattern with no closing delimiter is E682, and the `f` (fuzzy)
    /// flag isn't supported. An empty pattern (`//`) is the host's to
    /// answer, with the last search's.
    pub fn parse_vimgrep(args: &str) -> Result<(String, String, Vec<String>), ExError> {
        let args = args.trim_start_matches([' ', '\t']);
        let Some(delimiter) = args.chars().next() else {
            return Err(ExError::ArgumentRequired);
        };
        let (pattern, flags, rest) = if delimiter.is_alphanumeric() || delimiter == '_' {
            let end = args.find([' ', '\t']).unwrap_or(args.len());
            (&args[..end], "", &args[end..])
        } else {
            let body = &args[delimiter.len_utf8()..];
            let end = pattern_end(body, delimiter).ok_or_else(|| {
                ExError::Pattern("E682: Invalid search pattern or delimiter".into())
            })?;
            let after = &body[end + delimiter.len_utf8()..];
            let flags_end = after
                .find(|ch: char| !matches!(ch, 'g' | 'j' | 'f'))
                .unwrap_or(after.len());
            let flags = &after[..flags_end];
            if flags.contains('f') {
                return Err(ExError::Unsupported(":vimgrep's f flag".into()));
            }
            (&body[..end], flags, &after[flags_end..])
        };
        Ok((pattern.to_owned(), flags.to_owned(), words(rest)))
    }
}

/// Where in `body` the pattern ends at an unescaped `delimiter` outside a
/// `[…]` collection, as nvim's `skip_regexp()` finds it; none if it never
/// does.
fn pattern_end(body: &str, delimiter: char) -> Option<usize> {
    let mut ix = 0;
    while let Some(ch) = body[ix..].chars().next() {
        let next = ix + ch.len_utf8();
        ix = match ch {
            _ if ch == delimiter => return Some(ix),
            '\\' => body[next..]
                .chars()
                .next()
                .map_or(next, |ch| next + ch.len_utf8()),
            // A collection with no `]` is a plain `[`, as in nvim.
            '[' => collection_len(&body[next..]).map_or(next, |len| next + len),
            _ => next,
        };
    }
    None
}

/// The length of a collection's body after its `[`, up to and with its
/// `]`: a `^` first, then a `]` or `-` first, are members, and `\` escapes
/// the char after it. A `[:name:]` class, a `[=x=]` or `[.x.]` member and,
/// as the host's regex reads them, a nested `[…]` set are members whole, so
/// a `]` or delimiter in one doesn't end the collection; a `[` opening
/// none of them, or nested [`NESTING`] deep, is a plain member. None if it
/// never closes, as it can't once a set in it doesn't.
fn collection_len(after: &str) -> Option<usize> {
    nested_len(after, 0)
}

/// How deep sets nest before a `[` is a plain member: bounds the
/// recursion.
const NESTING: usize = 32;

/// [`collection_len`] of a set nested `depth` deep.
fn nested_len(after: &str, depth: usize) -> Option<usize> {
    let mut ix = 0;
    let first = |ix: usize| after[ix..].chars().next();
    if first(ix) == Some('^') {
        ix += 1;
    }
    if matches!(first(ix), Some(']' | '-')) {
        ix += 1;
    }
    while let Some(ch) = first(ix) {
        let next = ix + ch.len_utf8();
        ix = match ch {
            ']' => return Some(next),
            '\\' => first(next).map_or(next, |ch| next + ch.len_utf8()),
            '[' => match class_len(&after[next..]) {
                Some(len) => next + len,
                // An unclosed nested set reached the end: so would this.
                None if depth < NESTING => next + nested_len(&after[next..], depth + 1)?,
                None => next,
            },
            _ => next,
        };
    }
    None
}

/// The length of `:name:]`, `=x=]` or `.x.]` at the start of `after`, if
/// it starts with one.
fn class_len(after: &str) -> Option<usize> {
    let mut chars = after.chars();
    let end = match chars.next()? {
        ':' => after[1..]
            .find(|ch: char| !ch.is_ascii_alphabetic())
            .filter(|&name| name > 0)?,
        '=' | '.' => chars.next()?.len_utf8(),
        _ => return None,
    } + 1;
    let close = [&after[..1], "]"].concat();
    after[end..].starts_with(&close).then_some(end + 2)
}

/// `text` split at blanks not escaped by `\`, each word as typed.
fn words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            ' ' | '\t' => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            '\\' => {
                word.push(ch);
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            _ => word.push(ch),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}
