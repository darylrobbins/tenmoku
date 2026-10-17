//! `:s[ubstitute]`, `:&`, `:&&` and normal mode's `&` (`:help :s`, `:help
//! s_flags`, `:help sub-replace-special`): parsing a substitute's pattern,
//! replacement, flags and count, and running it over an ex range in one
//! transaction. The crate docs list the flags and specials.

use std::ops::Range;

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::commands::origin;

use crate::{
    VimTarget, edit,
    ex::ExError,
    insert, marks,
    motion::Cursor,
    operator,
    search::{self, Deadline, Interrupted, Pattern, PatternError},
};

/// A substitute's flags (`:help s_flags`): those fe-vim supports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// `&`, first if given: start from the last substitute's flags.
    pub keep: bool,
    /// `g`: every match on a line, not only the first. Each `g` toggles it.
    pub global: bool,
    /// `i` (`Some(true)`) or `I` (`Some(false)`), the last given: ignore or
    /// match case, unless the pattern holds `\c` or `\C`.
    pub ignore_case: Option<bool>,
    /// `n`: count the matches and report them, changing nothing.
    pub count_only: bool,
    /// `e`: finding nothing isn't an error. Each `e` toggles it.
    pub quiet: bool,
    /// `r`: a repeat takes the last search's pattern, not the last
    /// substitute's. (`&` doesn't carry it over.)
    pub search_pattern: bool,
}

/// What an `:s` line substitutes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Substitute {
    /// The pattern and the replacement as typed (`~` not yet expanded); an
    /// empty pattern is the last search's. `None` repeats the last
    /// substitute's (`:s` with no pattern, `:&`, `:&&`, `&`).
    pub with: Option<(String, String)>,
    pub flags: Flags,
    /// `[count]`: that many lines from the range's last.
    pub count: Option<u32>,
}

/// The last substitute, which `:s` with no pattern, `:&`, `:&&` and `&`
/// repeat, and whose replacement is `~`. Shared by every editor, as the last
/// search is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LastSubstitute {
    pattern: Option<String>,
    /// As typed, which a repeat expands `~` in again.
    replacement: String,
    /// The last replacement run with a valid pattern, `~` expanded: what
    /// the next `~` stands for.
    tilde: String,
    flags: Flags,
}

impl LastSubstitute {
    /// Its pattern, for a range's `\&`.
    pub(crate) fn pattern(&self) -> Option<&str> {
        self.pattern.as_deref()
    }
}

impl Substitute {
    /// The arguments of `:s`, what follows its name: a pattern and a
    /// replacement between delimiters (any character but a letter, digit,
    /// `\`, `"` or `|`, after blanks), then flags and a count; or with no
    /// delimiter, flags and a count for the last substitute.
    pub(crate) fn parse(args: &str) -> Result<Self, ExError> {
        let args = args.trim_start_matches([' ', '\t']);
        let Some(delimiter) = args.chars().next() else {
            return Self::repeat(args, false);
        };
        if delimiter.is_ascii_alphanumeric() || matches!(delimiter, '\\' | '"' | '|') {
            if delimiter.is_ascii_alphabetic() && !FLAGS.contains(delimiter) {
                return Err(ExError::LetterDelimiter);
            }
            if matches!(delimiter, '\\' | '"' | '|') {
                return Err(ExError::Unsupported(format!(":s{delimiter}")));
            }
            return Self::repeat(args, false);
        }
        let rest = &args[delimiter.len_utf8()..];
        let (pattern, after) = search::split_search(rest, delimiter);
        let (replacement, after) = match after {
            Some(after) => split_replacement(after, delimiter),
            None => (String::new(), ""),
        };
        Replacement::compile(&replacement)?;
        let (flags, count) = flags_and_count(after, true)?;
        Ok(Self {
            with: Some((pattern, replacement)),
            flags,
            count,
        })
    }

    /// `:&` (or `:s` with no pattern): `args` from its flags, after blanks
    /// (`&&`'s second `&` is `keep`, for `:&`).
    pub(crate) fn repeat(args: &str, keep_allowed: bool) -> Result<Self, ExError> {
        let args = args.trim_start_matches([' ', '\t']);
        let (flags, count) = flags_and_count(args, keep_allowed)?;
        Ok(Self {
            with: None,
            flags,
            count,
        })
    }
}

/// The flag letters nvim knows: those fe-vim doesn't support are refused
/// as trailing characters (`c`, which needs a prompt, with a message).
const FLAGS: &str = "&cegiInp#lr";

/// The replacement, up to an unescaped `delimiter`, as typed, and what
/// follows that delimiter.
fn split_replacement(line: &str, delimiter: char) -> (String, &str) {
    let mut chars = line.char_indices();
    while let Some((at, ch)) = chars.next() {
        if ch == delimiter {
            return (line[..at].to_string(), &line[at + ch.len_utf8()..]);
        }
        if ch == '\\' {
            chars.next();
        }
    }
    (line.to_string(), "")
}

/// Flags (`&` only first, if `keep_allowed`), blanks, a count, blanks.
fn flags_and_count(args: &str, keep_allowed: bool) -> Result<(Flags, Option<u32>), ExError> {
    let mut flags = Flags::default();
    let mut rest = args;
    if keep_allowed && let Some(after) = rest.strip_prefix('&') {
        flags.keep = true;
        rest = after;
    }
    while let Some(ch) = rest.chars().next() {
        match ch {
            'g' => flags.global = !flags.global,
            'i' => flags.ignore_case = Some(true),
            'I' => flags.ignore_case = Some(false),
            'n' => flags.count_only = true,
            'e' => flags.quiet = !flags.quiet,
            'r' => flags.search_pattern = true,
            'c' => return Err(ExError::Unsupported("the c flag (confirm)".into())),
            _ => break,
        }
        rest = &rest[1..];
    }
    let rest = rest.trim_start_matches([' ', '\t']);
    let digits = rest
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(rest.len());
    let count = match digits {
        0 => None,
        // nvim takes counts below 2147483647.
        _ => Some(
            rest[..digits]
                .parse::<u32>()
                .ok()
                .filter(|&count| count < i32::MAX as u32)
                .ok_or_else(|| ExError::ValueTooLarge(rest[..digits].to_string()))?,
        ),
    };
    if count == Some(0) {
        return Err(ExError::ZeroCount);
    }
    let rest = rest[digits..].trim_start_matches([' ', '\t']);
    match rest.is_empty() {
        true => Ok((flags, count)),
        false => Err(ExError::Trailing(rest.to_string())),
    }
}

/// `raw` with each unescaped `~` replaced by `previous`, the last
/// replacement as it was run (`\~` stays, for [`Replacement::compile`]).
fn expand_tilde(raw: &str, previous: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                out.push(ch);
                out.extend(chars.next());
            }
            '~' => out.push_str(previous),
            _ => out.push(ch),
        }
    }
    out
}

/// One piece of a compiled replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    /// Literal text, a line break as `\n`.
    Text(String),
    /// A group's text: `&` and `\0` are the whole match, `\1` to `\9` the
    /// groups (empty if a group took no part).
    Group(usize),
    /// `\u` (`true`) or `\l`: the next character's case.
    One(bool),
    /// `\U` (`Some(true)`), `\L`, or `\E` and `\e` (`None`): the case of
    /// the characters after, until the next.
    Run(Option<bool>),
}

/// A replacement, `~` expanded, ready to apply to matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Replacement {
    pieces: Vec<Piece>,
    /// It holds `\1` to `\9`.
    groups: bool,
}

impl Replacement {
    /// Reads `raw`'s specials (`:help sub-replace-special`, magic). `\n`
    /// (a NUL in Vim) and `\=` (an expression) are refused.
    pub(crate) fn compile(raw: &str) -> Result<Self, ExError> {
        if raw.starts_with("\\=") {
            return Err(ExError::Unsupported("\\= in a replacement".into()));
        }
        let mut pieces = Vec::new();
        let mut text = String::new();
        let push = |pieces: &mut Vec<Piece>, text: &mut String, piece: Piece| {
            if !text.is_empty() {
                pieces.push(Piece::Text(std::mem::take(text)));
            }
            pieces.push(piece);
        };
        let mut chars = raw.chars();
        while let Some(ch) = chars.next() {
            match ch {
                '&' => push(&mut pieces, &mut text, Piece::Group(0)),
                // A typed carriage return breaks the line, as `\r` does.
                '\r' => text.push('\n'),
                '\\' => match chars.next() {
                    // A trailing backslash is itself.
                    None => text.push('\\'),
                    Some(digit @ '0'..='9') => {
                        let n = digit as usize - '0' as usize;
                        push(&mut pieces, &mut text, Piece::Group(n));
                    }
                    Some('n') => return Err(ExError::NulInReplacement),
                    Some('r') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some('b') => text.push('\u{8}'),
                    Some(case @ ('u' | 'l')) => {
                        push(&mut pieces, &mut text, Piece::One(case == 'u'));
                    }
                    Some(case @ ('U' | 'L')) => {
                        push(&mut pieces, &mut text, Piece::Run(Some(case == 'U')));
                    }
                    Some('E' | 'e') => push(&mut pieces, &mut text, Piece::Run(None)),
                    // `\\`, `\&`, `\~`, an escaped delimiter and any other
                    // character are that character.
                    Some(other) => text.push(other),
                },
                _ => text.push(ch),
            }
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(text));
        }
        let groups = pieces
            .iter()
            .any(|piece| matches!(piece, Piece::Group(1..)));
        Ok(Self { pieces, groups })
    }

    /// Appends the replacement for a match to `out`, `group` giving each
    /// group's text; returns how many line breaks it wrote.
    fn apply<'a>(&self, group: impl Fn(usize) -> &'a str, out: &mut String) -> usize {
        let mut one: Option<bool> = None;
        let mut run: Option<bool> = None;
        let mut breaks = 0;
        let mut put = |text: &str, one: &mut Option<bool>, run: Option<bool>| {
            for ch in text.chars() {
                breaks += usize::from(ch == '\n');
                match one.take().or(run) {
                    Some(true) => out.push(edit::raised(ch)),
                    Some(false) => out.push(edit::lowered(ch)),
                    None => out.push(ch),
                }
            }
        };
        for piece in &self.pieces {
            match piece {
                Piece::Text(text) => put(text, &mut one, run),
                Piece::Group(n) => put(group(*n), &mut one, run),
                Piece::One(upper) => one = Some(*upper),
                Piece::Run(case) => {
                    run = *case;
                    // `\E` ends a `\u` or `\l` still waiting, too.
                    if case.is_none() {
                        one = None;
                    }
                }
            }
        }
        breaks
    }
}

type Handled = Result<Dispatched<TransactionId>, CommandError>;

/// Runs `sub` over `rows` (checked against the text) as nvim
/// does: the pattern (but a repeat's) becomes the last search, keeping its
/// direction, and the pattern, replacement and flags the last substitute's,
/// even if nothing matches. Every match is replaced in one transaction,
/// which leaves the caret on the first non-blank of the last line changed
/// (the last a line break made, after `\r`); with `n` nothing changes and
/// the caret goes to its own line's first non-blank. `.` doesn't repeat it.
///
/// A command that passes [`search::time_limit`] before it has matched every
/// row (an adversarial pattern on a long row) stops with
/// `ExError::Pattern("Interrupted")`, changing nothing in the buffer: no
/// transaction, no jump mark, no caret move. The last search, the last
/// substitute and the highlight are set as for any other run.
pub(crate) fn run(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    rows: Range<u32>,
    sub: &Substitute,
) -> Result<Handled, ExError> {
    insert::adopt_closing(target.state);
    let last = target.registers.substitute.clone();
    // `&` starts from the last flags: `g` and `e` toggle theirs.
    let flags = match sub.flags.keep {
        true => Flags {
            global: last.flags.global != sub.flags.global,
            ignore_case: sub.flags.ignore_case.or(last.flags.ignore_case),
            count_only: last.flags.count_only || sub.flags.count_only,
            quiet: last.flags.quiet != sub.flags.quiet,
            search_pattern: sub.flags.search_pattern,
            keep: false,
        },
        false => sub.flags,
    };
    let (pattern, raw) = match &sub.with {
        Some((pattern, raw)) => {
            let pattern = match pattern.is_empty() {
                true => target.registers.last_search().map(|(pattern, _)| pattern),
                false => Some(pattern.as_str()),
            };
            let pattern = pattern.ok_or(ExError::NoPreviousSearch)?.to_string();
            (pattern, raw.clone())
        }
        None if flags.search_pattern => (
            target
                .registers
                .last_search()
                .map(|(pattern, _)| pattern.to_string())
                .ok_or(ExError::NoPreviousSearch)?,
            last.replacement.clone(),
        ),
        None => (
            last.pattern.clone().ok_or(ExError::NoPreviousSubstitute)?,
            last.replacement.clone(),
        ),
    };
    let replacement = expand_tilde(&raw, &last.tilde);
    let compiled = Replacement::compile(&replacement)?;
    let parsed = Pattern::parse_with(&pattern, flags.ignore_case.unwrap_or(false));
    // As nvim's: the next `~` is this replacement only if the pattern is
    // valid, matching or not.
    target.registers.substitute = LastSubstitute {
        pattern: Some(pattern.clone()),
        replacement: raw,
        tilde: match parsed {
            Ok(_) => replacement,
            Err(_) => last.tilde,
        },
        flags,
    };
    // The pattern becomes the last search, as a search's does: even if Vim
    // rejects it, but not one fe-vim can't search.
    if sub.with.is_some()
        && !matches!(
            parsed,
            Err(PatternError::Unsupported(_) | PatternError::TooLarge(_))
        )
    {
        let forward = target.registers.last_search().is_none_or(|(_, f)| f);
        target.registers.set_search(&pattern, forward);
        // An empty pattern is the last search's already.
        if sub
            .with
            .as_ref()
            .is_some_and(|(typed, _)| !typed.is_empty())
        {
            target.registers.note_pattern(&pattern);
        }
        target.state.highlight = Pattern::parse(&pattern).ok();
    }
    let parsed = parsed.map_err(|error| ExError::Pattern(error.to_string()))?;
    let Ok(found) = find(target, &parsed, &compiled, rows, flags) else {
        return Err(ExError::Pattern(Interrupted.to_string()));
    };
    if found.matches == 0 {
        return match flags.quiet {
            true => Ok(Ok(Dispatched::new(Effect::Notify))),
            false => Err(ExError::NotFound(pattern)),
        };
    }
    // Finding a match makes it a jump, as in nvim.
    marks::jump_here(target);
    let starts = found.edits.iter().map(|(range, _)| range.start);
    target.state.marks.typing(target.model.snapshot(), starts);
    let report = |n: usize, one: &str, many: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    let VimTarget { model, state, .. } = target;
    // As nvim reports them: `n`'s count always, substitutions past two.
    if flags.count_only || found.matches > 2 {
        let what = match flags.count_only {
            true => report(found.matches, "match", "matches"),
            false => report(found.matches, "substitution", "substitutions"),
        };
        state.message = Some(format!(
            "{what} on {}",
            report(found.lines, "line", "lines")
        ));
    }
    state.goals = None;
    if flags.count_only {
        let text = model.text();
        let row = text.offset_to_point(model.primary_selection().head()).row;
        model.set_selections([operator::caret(first_non_blank(text, row))]);
        return Ok(Ok(Dispatched::new(Effect::Changed)));
    }
    let (transaction, snapshot) = operator::apply(model, found.edits, origin(actor));
    let text = snapshot.text();
    let caret = first_non_blank(text, found.last_row);
    operator::land(model, &snapshot, [caret]);
    if let Some(id) = transaction {
        // As nvim's undo and redo leave it: the first changed line's start.
        state.carets.record(id, vec![(found.first_row, 0)]);
    }
    Ok(Ok(
        Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect())
    ))
}

/// The first non-blank on `row`, as an offset.
fn first_non_blank(text: &fe_text::Text, row: u32) -> usize {
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.ix = cursor.line.first_non_blank();
    cursor.offset()
}

/// What [`find`] found.
struct Found {
    edits: Vec<(Range<usize>, String)>,
    matches: usize,
    lines: usize,
    /// The first line changed, as a row.
    first_row: u32,
    /// The last line changed, as a row of the text after the edits.
    last_row: u32,
}

/// The matches on `rows` (the first on each, or all with `g`) and their
/// replacements, each row read once; `Err` if the time limit ran out.
fn find(
    target: &mut VimTarget<'_>,
    pattern: &Pattern,
    replacement: &Replacement,
    rows: Range<u32>,
    flags: Flags,
) -> Result<Found, Interrupted> {
    let deadline = Deadline::new();
    target.model.sync_display();
    let text = target.model.text();
    let mut found = Found {
        edits: Vec::new(),
        matches: 0,
        lines: 0,
        first_row: 0,
        last_row: 0,
    };
    // Line breaks the replacements put in, so far.
    let mut breaks = 0;
    let mut buf = String::new();
    for row in rows.start..rows.end.min(text.row_count()) {
        let (start, hay) = search::row_text(text, row, &mut buf);
        let mut matches = pattern.substitutions(hay, replacement.groups, &deadline);
        let mut on_row = 0;
        // One edit per row, from its first match to its last, the text
        // between matches copied: a `g` over many matches is one edit.
        let mut span: Option<Range<usize>> = None;
        let mut new = String::new();
        while let Some(range) = matches.next() {
            on_row += 1;
            if !flags.count_only {
                let group = |n: usize| match n {
                    0 => &hay[range.clone()],
                    n => matches.group(n).map_or("", |range| &hay[range]),
                };
                let span = span.get_or_insert(range.start..range.start);
                new.push_str(&hay[span.end..range.start]);
                breaks += replacement.apply(group, &mut new);
                span.end = range.end;
            }
            if !flags.global {
                break;
            }
        }
        deadline.check()?;
        if let Some(span) = span {
            found
                .edits
                .push((start + span.start..start + span.end, new));
        }
        if on_row > 0 {
            if found.lines == 0 {
                found.first_row = row;
            }
            found.matches += on_row;
            found.lines += 1;
            found.last_row = row + u32::try_from(breaks).unwrap_or(u32::MAX);
        }
    }
    Ok(found)
}

/// `&`: the last substitute on the cursor's line, or `count` lines from it,
/// with its flags (nvim's default mapping, `:&&`).
pub(crate) fn repeat(target: &mut VimTarget<'_>, actor: &Actor, count: Option<u32>) -> Handled {
    let line = match count.unwrap_or(1).max(1) {
        1 => "&&".to_string(),
        count => format!(".,.+{}&&", count - 1),
    };
    crate::ex::run(target, actor, &line)
}
