//! `:s[ubstitute]`, `:&`, `:&&` and normal mode's `&` (`:help :s`, `:help
//! s_flags`, `:help sub-replace-special`): parsing a substitute's pattern,
//! replacement, flags and count, and running it over an ex range in one
//! transaction. The crate docs list the flags and specials.

use std::{cell::Cell, collections::VecDeque, ops::Range};

use fe_buffer::{Anchor, ResolvedSelection, TransactionId, Version};
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::{EditorModel, commands::origin};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Mode, Pending, Step, VimKey, VimState, VimTarget,
    args::JoinArgs,
    edit,
    ex::{ExError, global},
    insert, marks,
    motion::Cursor,
    operator, reducer,
    search::{
        self, Deadline, Interrupted, Pattern, PatternError,
        replace::{Finder, Taken, Took, Walk},
    },
    visual,
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
    /// `r`: a repeat or `:s\&` takes the last search's pattern, not the
    /// last substitute's. (`&` doesn't carry it over.)
    pub search_pattern: bool,
    /// `c`: ask before each match ([`Answer`]). `n` beats it.
    pub confirm: bool,
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
    /// `:s/\n//`, with no flag or only one of `g l p #` and no count: nvim
    /// joins the lines as `:join!` does instead (`sub_joining_lines`).
    pub joins: bool,
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
    /// Whether its pattern is `*`'s or `#`'s, which 'smartcase' doesn't
    /// apply to: it came from the last search when that was one.
    word: bool,
}

impl LastSubstitute {
    /// Its pattern, for a range's `\&`.
    pub(crate) fn pattern(&self) -> Option<&str> {
        self.pattern.as_deref()
    }

    /// Whether its pattern is `*`'s or `#`'s ([`LastSubstitute::word`]).
    pub(crate) fn word(&self) -> bool {
        self.word
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
            if delimiter == '\\' {
                return Self::last_pattern(&args[1..]);
            }
            if matches!(delimiter, '"' | '|') {
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
        let joins = pattern == "\\n"
            && replacement.is_empty()
            && matches!(after, "" | "g" | "l" | "p" | "#");
        let (flags, count) = flags_and_count(after, true)?;
        Ok(Self {
            with: Some((pattern, replacement)),
            flags,
            count,
            joins,
        })
    }

    /// `:s\/{string}/`, `:s\?{string}?` and `:s\&{string}&` (`args` after
    /// the backslash): `string` replaces the last search's pattern or, for
    /// `&`, the last substitute's, which [`run`] looks up.
    fn last_pattern(args: &str) -> Result<Self, ExError> {
        let Some(delimiter @ ('/' | '?' | '&')) = args.chars().next() else {
            return Err(ExError::BackslashDelimiter);
        };
        let (replacement, after) = split_replacement(&args[1..], delimiter);
        Replacement::compile(&replacement)?;
        let (flags, count) = flags_and_count(after, true)?;
        let pattern = match delimiter {
            '&' => LAST_SUBSTITUTE_PATTERN.to_string(),
            _ => String::new(),
        };
        Ok(Self {
            with: Some((pattern, replacement)),
            flags,
            count,
            joins: false,
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
            joins: false,
        })
    }
}

/// Stands for the last substitute's pattern where a [`Substitute`]'s pattern
/// is typed (`:s\&`): a NUL, which no typed pattern holds.
const LAST_SUBSTITUTE_PATTERN: &str = "\0";

/// The flag letters nvim knows.
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
            // Print flags: nvim prints the last line changed, which
            // fastedit has nowhere to do.
            'p' | '#' | 'l' => {}
            'c' => flags.confirm = true,
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
/// With `c` (but not `n`), it asks before each match instead ([`ask`]),
/// which a `:g` can't wait for: there it's refused, changing nothing.
///
/// A pattern that matches line breaks ([`Pattern::spans_rows`]) is matched
/// over rows as nvim's `:s` matches it ([`Walk`]): a replaced match joins
/// the rows it spans, their marks going as nvim's do, and the rows it
/// joins drop out of the range. A pattern taken from the last search or
/// the last substitute keeps whether it was `*`'s or `#`'s, which
/// 'smartcase' doesn't apply to. `:s/\n//` joins the lines as `:join!`
/// does ([`Substitute::joins`]).
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
    // One run while another asks (from a host's command) ends that one, as
    // `q` would.
    target.state.confirm = None;
    let last = target.registers.substitute.clone();
    // `&` starts from the last flags: `g` and `e` toggle theirs.
    let flags = match sub.flags.keep {
        true => Flags {
            global: last.flags.global != sub.flags.global,
            ignore_case: sub.flags.ignore_case.or(last.flags.ignore_case),
            count_only: last.flags.count_only || sub.flags.count_only,
            quiet: last.flags.quiet != sub.flags.quiet,
            search_pattern: sub.flags.search_pattern,
            confirm: last.flags.confirm || sub.flags.confirm,
            keep: false,
        },
        false => sub.flags,
    };
    // `n` counts without asking, as in nvim.
    let confirm = flags.confirm && !flags.count_only;
    if confirm && UNASKED.get() {
        return Err(ExError::Unsupported("the c flag (confirm) under :g".into()));
    }
    // The last search's pattern, and whether it is `*`'s or `#`'s.
    let last_search = || {
        target
            .registers
            .last_search()
            .map(|(pattern, _)| (pattern.to_string(), target.registers.last_search_word()))
            .ok_or(ExError::NoPreviousSearch)
    };
    let last_substitute = || {
        last.pattern
            .clone()
            .map(|pattern| (pattern, last.word))
            .ok_or(ExError::NoPreviousSubstitute)
    };
    // A pattern taken from the last search or the last substitute keeps
    // whether it was `*`'s or `#`'s, as nvim's does.
    let ((pattern, word), raw) = match &sub.with {
        Some((pattern, raw)) => {
            let pattern = match pattern.as_str() {
                // `r` makes it the last search's, as for a repeat.
                LAST_SUBSTITUTE_PATTERN if !flags.search_pattern => last_substitute()?,
                LAST_SUBSTITUTE_PATTERN | "" => last_search()?,
                typed => (typed.to_string(), false),
            };
            (pattern, raw.clone())
        }
        None if flags.search_pattern => (last_search()?, last.replacement.clone()),
        None => (last_substitute()?, last.replacement.clone()),
    };
    let replacement = expand_tilde(&raw, &last.tilde);
    let compiled = Replacement::compile(&replacement)?;
    // The prompt shows the replacement `~` expanded, as nvim's does.
    let replacement_shown = replacement.clone();
    let searched_ignoring = target.registers.ignores_case(&pattern, word);
    let ignore_case = flags.ignore_case.unwrap_or(searched_ignoring);
    let parsed = Pattern::parse_across(&pattern, ignore_case);
    // As nvim's: the next `~` is this replacement only if the pattern is
    // valid, matching or not. `:s/\n//` leaves the last flags be.
    target.registers.substitute = LastSubstitute {
        pattern: Some(pattern.clone()),
        replacement: raw,
        tilde: match parsed {
            Ok(_) => replacement,
            Err(_) => last.tilde,
        },
        flags: match sub.joins {
            true => last.flags,
            false => flags,
        },
        word,
    };
    // The pattern becomes the last search, as a search's does: even if Vim
    // rejects it, but not one fe-vim can't search.
    // (`:s\&`'s isn't: it was the last substitute's, not the last search's.)
    if sub
        .with
        .as_ref()
        .is_some_and(|(typed, _)| typed != LAST_SUBSTITUTE_PATTERN)
        && !matches!(
            parsed,
            Err(PatternError::Unsupported(_) | PatternError::TooLarge(_))
        )
    {
        let forward = target.registers.last_search().is_none_or(|(_, f)| f);
        target.registers.set_search(&pattern, forward);
        target.registers.set_search_word(word);
        // An empty pattern is the last search's already.
        if sub
            .with
            .as_ref()
            .is_some_and(|(typed, _)| !typed.is_empty())
        {
            target.registers.note_pattern(&pattern);
        }
        target.state.highlight = Pattern::parse_across(&pattern, searched_ignoring).ok();
    }
    let parsed = parsed.map_err(|error| ExError::Pattern(error.to_string()))?;
    if sub.joins {
        return join(target, actor, rows);
    }
    if confirm {
        let asked = Asked {
            pattern: parsed,
            replacement: compiled,
            shown: replacement_shown,
            flags,
        };
        return ask(target, asked, rows, pattern);
    }
    let found = match parsed.spans_rows() {
        true => find_across(target, &parsed, &compiled, rows, flags),
        false => find(target, &parsed, &compiled, rows, flags),
    };
    let Ok(found) = found else {
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
    // The rows a replacement joins go, and their marks with them, as nvim's
    // do.
    let rows = target.model.text().row_count();
    for &(first, last) in &found.joined {
        let snapshot = target.model.snapshot();
        target.state.marks.deleting(snapshot, first, last);
        if last + 1 == rows {
            visual::joining_at_end(target.state, snapshot, first);
        }
    }
    let starts = found.edits.iter().map(|(range, _)| range.start);
    target.state.marks.typing(target.model.snapshot(), starts);
    let VimTarget { model, state, .. } = target;
    if let Some(report) = report(found.matches, found.lines, flags.count_only) {
        state.message = Some(report);
    }
    state.goals = None;
    if flags.count_only {
        let text = model.text();
        let row = text.offset_to_point(model.primary_selection().head()).row;
        model.set_selections([operator::caret(first_non_blank(text, row))]);
        return Ok(Ok(Dispatched::new(Effect::Changed)));
    }
    let from = model.buffer().version();
    let (transaction, snapshot) = operator::apply(model, found.edits, origin(actor));
    // A `:g` running drops the rows joined, and only those.
    let joined = global::Change::Substituted {
        joined: found.joined,
    };
    global::noted(from, snapshot.version(), joined);
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

/// nvim's report of `matches` (with `n`, `count_only`) or substitutions on
/// `lines` lines: `n`'s count always, substitutions past two.
fn report(matches: usize, lines: usize, count_only: bool) -> Option<String> {
    let counted = |n: usize, one: &str, many: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    let what = match count_only {
        true => counted(matches, "match", "matches"),
        false if matches > 2 => counted(matches, "substitution", "substitutions"),
        false => return None,
    };
    Some(format!("{what} on {}", counted(lines, "line", "lines")))
}

/// The first non-blank on `row`, as an offset.
fn first_non_blank(text: &fe_text::Text, row: u32) -> usize {
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.go_to_first_non_blank();
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
    /// The rows (first and last) that replacements join to the row before
    /// them, which go.
    joined: Vec<(u32, u32)>,
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
        joined: Vec::new(),
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

/// [`find`] for a pattern that [`Pattern::spans_rows`]: the matches as
/// nvim's `:s` takes them over rows ([`Walk`]), reading the text as nvim
/// reads its buffer: with what it has replaced, but for the [`Line`] being
/// written. Each line written is one edit, those written one after another
/// on a line its joins make one, as for one row.
fn find_across(
    target: &mut VimTarget<'_>,
    pattern: &Pattern,
    replacement: &Replacement,
    rows: Range<u32>,
    flags: Flags,
) -> Result<Found, Interrupted> {
    let deadline = Deadline::new();
    target.model.sync_display();
    let text = target.model.text();
    let groups = match (flags.count_only, replacement.groups) {
        (true, _) => 0,
        (false, true) => 10,
        (false, false) => 1,
    };
    let mut across = Across {
        text,
        finder: Finder::new(pattern, text, &deadline, groups),
        walk: Walk::new(text, rows, flags.global),
        longer: 0,
        edit: None,
        found: Found {
            edits: Vec::new(),
            matches: 0,
            lines: 0,
            first_row: 0,
            last_row: 0,
            joined: Vec::new(),
        },
    };
    let mut pass = None;
    let mut line: Option<Line> = None;
    loop {
        let Some(mut taken) = across.walk.next(&mut across.finder, line.is_some())? else {
            match line.take() {
                Some(line) => {
                    across.write(line, None);
                    continue;
                }
                None => break,
            }
        };
        if let Some(written) = line.take_if(|line| line.row != taken.start_row) {
            across.write(written, Some(&mut taken));
        }
        let took = match flags.count_only {
            true => Took::Passed,
            false => Took::Replaced,
        };
        across.walk.took(across.finder.text(), &taken, took);
        let found = &mut across.found;
        found.matches += 1;
        if pass != Some(taken.pass) {
            pass = Some(taken.pass);
            found.lines += 1;
        }
        if flags.count_only {
            continue;
        }
        // Its rows in the text before the `:s`.
        let row = |at: usize| {
            text.offset_to_point(at.wrapping_add_signed(-across.longer))
                .row
        };
        let (start_row, end_row) = (row(taken.range.start), row(taken.range.end));
        if found.matches == 1 {
            found.first_row = start_row;
        }
        if taken.joins() {
            // Rows joined one after another go as one stretch.
            match found.joined.last_mut() {
                Some((_, last)) if *last == start_row => *last = end_row,
                _ => found.joined.push((start_row + 1, end_row)),
            }
        }
        let new = Line::open(&mut line, across.finder.text(), &taken);
        replacement.apply(|n| taken.group(n), new);
        if taken.joins() {
            across.write(line.take().expect("a line open"), None);
        }
    }
    if let Some((span, new)) = across.edit.take() {
        across.found.edits.extend(joining(text, span, new));
    }
    Ok(across.found)
}

/// The replacements nvim's `:s` over rows has made since it last wrote a
/// line, all on one row of the text it reads: it reads the row as it was
/// until it writes the line, after a replacement that joins rows (the
/// search going on in the joined line), before a match on a later row and
/// once the row's searches are done.
#[derive(Clone, Debug)]
struct Line {
    /// Its row, and the span of the text its replacements take, from the
    /// first's start to the last's end.
    row: u32,
    span: Range<usize>,
    /// What replaces the span: the replacements and the text between them.
    new: String,
}

impl Line {
    /// Adds `taken`, a match on `line`'s row (or the first of a new line),
    /// to `line`, reading the text before it from `text`: what replaces it
    /// goes after what this gives.
    fn open<'a>(line: &'a mut Option<Self>, text: &fe_text::Text, taken: &Taken) -> &'a mut String {
        let at = taken.range.start;
        let line = line.get_or_insert_with(|| Self {
            row: taken.start_row,
            span: at..at,
            new: String::new(),
        });
        line.new.push_str(&text.slice_to_string(line.span.end..at));
        line.span.end = taken.range.end;
        &mut line.new
    }

    /// How much longer the text is with it written.
    fn longer(&self) -> isize {
        self.new.len() as isize - self.span.len() as isize
    }
}

/// [`find_across`]'s walk over `text`, reading it with the lines written.
struct Across<'a> {
    text: &'a fe_text::Text,
    finder: Finder<'a>,
    walk: Walk,
    /// How much longer the text read is than `text`, past the lines written.
    longer: isize,
    /// The edit of the lines written last, those a join made one: the span
    /// of `text` it replaces, and what with.
    edit: Option<(Range<usize>, String)>,
    found: Found,
}

impl Across<'_> {
    /// Writes `line` into the text read, with `taken`, a match past it, and
    /// adds its edit of `text`.
    fn write(&mut self, line: Line, taken: Option<&mut Taken>) {
        let before = self.finder.text().clone();
        self.finder.edit(line.span.clone(), &line.new);
        let after = self.finder.text();
        self.walk
            .edited(&before, line.span.clone(), &line.new, after);
        if let Some(taken) = taken {
            taken.edited(&before, line.span.clone(), &line.new, after);
        }
        // Where the replacement ends, in the text after the edits.
        self.found.last_row = after.offset_to_point(line.span.start + line.new.len()).row;
        let back = |at: usize| at.wrapping_add_signed(-self.longer);
        let span = back(line.span.start)..back(line.span.end);
        self.longer += line.longer();
        let row = |at: usize| self.text.offset_to_point(at).row;
        match &mut self.edit {
            // The line a join made, written again.
            Some((last, new)) if row(last.end) == row(span.start) => {
                new.push_str(&self.text.slice_to_string(last.end..span.start));
                new.push_str(&line.new);
                last.end = span.end;
            }
            edit => {
                if let Some((span, new)) = edit.replace((span, line.new)) {
                    self.found.edits.extend(joining(self.text, span, new));
                }
            }
        }
    }
}

/// The edits replacing `span` of `text` with `new`. Over rows, two: the
/// rest of its first row replaced with `new` and what follows `span` on its
/// last, and the rows after the first through the last deleted whole, so
/// that what is anchored on them (`'<` and `'>`) goes to the row after
/// them, as nvim's marks on the rows `:s` joins do. One if `span` ends on
/// the last row.
fn joining(text: &fe_text::Text, span: Range<usize>, new: String) -> Vec<(Range<usize>, String)> {
    let first = text.offset_to_point(span.start).row;
    let last = text.offset_to_point(span.end).row;
    if first == last || last + 1 >= text.row_count() {
        return vec![(span, new)];
    }
    let row_end = |row: u32| text.row_start(row) + text.row_len(row) as usize;
    let tail = text.slice_to_string(span.end..row_end(last));
    vec![
        (span.start..row_end(first), new + &tail),
        (
            text.row_start(first + 1)..text.row_start(last + 1),
            String::new(),
        ),
    ]
}

/// `:s/\n//` over `rows` ([`Substitute::joins`]), as nvim runs it: the
/// lines joined with the one after the range (if it isn't the last), as
/// they are, as `:join!` joins them, the caret where `gJ` leaves it, with
/// nvim's report of substitutions and no match needed: over the last line
/// alone, nothing is joined, and the caret goes to it.
fn join(target: &mut VimTarget<'_>, actor: &Actor, rows: Range<u32>) -> Result<Handled, ExError> {
    let text = target.model.text();
    let last_row = text.row_count() - 1;
    let first = rows.start.min(last_row);
    let last = rows.end.saturating_sub(1).clamp(first, last_row);
    let count = last - first + 1 + u32::from(last < last_row);
    // The caret goes to the first line, at its byte column, as nvim's does.
    let head = target.model.primary_selection().head();
    let column = head - text.row_start(text.offset_to_point(head).row);
    let start = text.row_start(first);
    let at = Cursor::at(text, start + column.min(text.row_len(first) as usize)).offset();
    target.model.set_selections([operator::caret(at)]);
    target.state.goals = None;
    if count < 2 {
        return Ok(Ok(Dispatched::new(Effect::Changed)));
    }
    // `gJ`'s join, which `.` doesn't repeat.
    let last_change = (target.state.last_change.clone(), target.state.changes);
    let from = target.model.buffer().version();
    let joined = edit::join_raw(
        target,
        actor,
        JoinArgs {
            count: Some(count),
            raw: true,
        },
    );
    (target.state.last_change, target.state.changes) = last_change;
    let until = target.model.buffer().version();
    global::noted(from, until, global::Change::Joined);
    if let Some(report) = report(count as usize - 1, 1, false) {
        target.state.message = Some(report);
    }
    Ok(joined)
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

thread_local! {
    /// Set while a `:g` runs: a `:s` with `c` it runs is refused, since `:g`
    /// can't wait for the answers.
    static UNASKED: Cell<bool> = const { Cell::new(false) };
}

/// Refuses `:s`'s `c` until dropped, for a `:g` running.
pub(crate) struct Unasked(bool);

impl Unasked {
    pub(crate) fn start() -> Self {
        Self(UNASKED.replace(true))
    }
}

impl Drop for Unasked {
    fn drop(&mut self) {
        UNASKED.set(self.0);
    }
}

/// An answer to `:s`'s `c` prompt (`:help :s_c`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Answer {
    /// `y`: replace this match and ask about the next.
    #[serde(rename = "y")]
    Yes,
    /// `n`: skip this match.
    #[serde(rename = "n")]
    No,
    /// `a`: replace this match and every one after it, without asking.
    #[serde(rename = "a")]
    All,
    /// `q` (and `<Esc>` and `<C-c>`): replace no more.
    #[serde(rename = "q")]
    Quit,
    /// `l`: replace this match and no more.
    #[serde(rename = "l")]
    Last,
}

/// `vim.substitute_answer`'s args: `{"answer": "y"}`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AnswerArgs {
    pub answer: Answer,
}

/// A `:s` with `c` waiting for an answer about a match: what's left of it.
/// Rows are read as for any `:s`, once each before any of their matches is
/// replaced, so a replacement is never matched again.
#[derive(Clone, Debug)]
pub(crate) struct Confirm {
    pattern: Pattern,
    replacement: Replacement,
    /// The prompt, which shows the replacement.
    prompt: String,
    global: bool,
    /// The range's first row, before any is read.
    first: u32,
    /// The end of the last row read, which a replacement's line breaks
    /// move down.
    row_end: Option<Anchor>,
    /// The start of the row after the range's last, `None` past the text.
    end: Option<Anchor>,
    /// The matches left on the last row read, the one asked about first:
    /// where each starts, its length and its replacement.
    waiting: VecDeque<(Anchor, usize, String)>,
    /// The text's version once the last answer was taken: if it moved on,
    /// something else edited the text, and the asking ends.
    version: Version,
    /// The undo step every replacement goes in.
    transaction: Option<TransactionId>,
    substitutions: usize,
    /// The rows with a substitution, and whether the last row read has one.
    lines: usize,
    row_changed: bool,
    /// The row the last replacement ended on.
    last_row: u32,
    /// For a pattern that spans rows, the walk over the range's rows; it
    /// reads the rows in place of `row_end` and `end`.
    rows: Option<Rows>,
}

/// A `:s` with `c`'s walk over rows ([`Walk`]) and the text it reads, as
/// nvim's `:s` reads its buffer: the text but for the replacements of the
/// [`Line`] being written, which nvim shows but doesn't match in until it
/// writes the line.
#[derive(Clone, Debug)]
struct Rows {
    walk: Walk,
    read: fe_text::Text,
    line: Option<Line>,
    /// The match waiting.
    taken: Option<Taken>,
}

impl Rows {
    /// Writes `line` into the text read, with `taken`, a match past it.
    fn write(&mut self, line: Line, taken: Option<&mut Taken>) {
        let before = self.read.clone();
        self.read.replace(line.span.clone(), &line.new);
        self.walk
            .edited(&before, line.span.clone(), &line.new, &self.read);
        if let Some(taken) = taken {
            taken.edited(&before, line.span, &line.new, &self.read);
        }
    }

    /// Where `at`, past the line being written in the text read, is in the
    /// text.
    fn shown(&self, at: usize) -> usize {
        at.wrapping_add_signed(self.line.as_ref().map_or(0, Line::longer))
    }
}

/// What a `:s` with `c` asks about: its pattern, its replacement, as run
/// and as the prompt shows it (`~` expanded), and its flags.
struct Asked {
    pattern: Pattern,
    replacement: Replacement,
    shown: String,
    flags: Flags,
}

/// `:s` with `c` over `rows`, once the last search and the last substitute
/// are set: the caret goes to the first match, selecting it, and
/// [`VimState::message`](crate::VimState::message) asks about it as nvim's
/// prompt does, until `vim.substitute_answer` ([`answer`]) ends it. Finding
/// none is E486, or with `e` nothing, as without `c`.
fn ask(
    target: &mut VimTarget<'_>,
    asked: Asked,
    rows: Range<u32>,
    typed: String,
) -> Result<Handled, ExError> {
    operator::sync(target.model);
    let text = target.model.text();
    let end = (rows.end < text.row_count()).then(|| {
        target
            .model
            .snapshot()
            .anchor_after(text.row_start(rows.end))
    });
    let mut confirm = Box::new(Confirm {
        pattern: asked.pattern,
        replacement: asked.replacement,
        prompt: format!(
            "replace with {}? (y)es/(n)o/(a)ll/(q)uit/(l)ast/scroll up(^E)/down(^Y)",
            asked.shown
        ),
        global: asked.flags.global,
        first: rows.start,
        row_end: None,
        end,
        waiting: VecDeque::new(),
        version: target.model.snapshot().version(),
        transaction: None,
        substitutions: 0,
        lines: 0,
        row_changed: false,
        last_row: 0,
        rows: None,
    });
    if confirm.pattern.spans_rows() {
        confirm.rows = Some(Rows {
            walk: Walk::new(text, rows, asked.flags.global),
            read: text.clone(),
            line: None,
            taken: None,
        });
    }
    let Ok(found) = confirm.next(target.model, &Deadline::new()) else {
        return Err(ExError::Pattern(Interrupted.to_string()));
    };
    if !found {
        return match asked.flags.quiet {
            true => Ok(Ok(Dispatched::new(Effect::Notify))),
            false => Err(ExError::NotFound(typed)),
        };
    }
    // Finding a match makes it a jump, as in nvim.
    marks::jump_here(target);
    confirm.show(target);
    Ok(Ok(Dispatched::new(Effect::Changed)))
}

impl Confirm {
    /// Reads rows until a match waits, if one is left in the range; `Err`
    /// if the time limit ran out.
    fn next(&mut self, model: &EditorModel, deadline: &Deadline) -> Result<bool, Interrupted> {
        let snapshot = model.snapshot();
        let text = snapshot.text();
        if let Some(rows) = &mut self.rows {
            if !self.waiting.is_empty() {
                return Ok(true);
            }
            let groups = if self.replacement.groups { 10 } else { 1 };
            let mut taken = loop {
                let mut finder = Finder::new(&self.pattern, &rows.read, deadline, groups);
                let next = rows.walk.next(&mut finder, rows.line.is_some());
                drop(finder);
                match (next?, rows.line.take()) {
                    (Some(taken), line) => {
                        rows.line = line;
                        break taken;
                    }
                    (None, Some(line)) => rows.write(line, None),
                    (None, None) => return Ok(false),
                }
            };
            if let Some(line) = rows.line.take_if(|line| line.row != taken.start_row) {
                rows.write(line, Some(&mut taken));
            }
            if rows
                .taken
                .as_ref()
                .is_none_or(|last| last.pass != taken.pass)
            {
                self.row_changed = false;
            }
            let mut new = String::new();
            self.replacement.apply(|n| taken.group(n), &mut new);
            let at = snapshot.anchor_after(rows.shown(taken.range.start));
            self.waiting.push_back((at, taken.range.len(), new));
            rows.taken = Some(taken);
            return Ok(true);
        }
        let row_of = |anchor: &Anchor| text.offset_to_point(snapshot.resolve(anchor)).row;
        let end = self.end.as_ref().map_or(text.row_count(), row_of);
        let mut row = self
            .row_end
            .as_ref()
            .map_or(self.first, |at| row_of(at) + 1);
        let mut buf = String::new();
        while self.waiting.is_empty() && row < end.min(text.row_count()) {
            let (start, hay) = search::row_text(text, row, &mut buf);
            let mut matches = self
                .pattern
                .substitutions(hay, self.replacement.groups, deadline);
            while let Some(range) = matches.next() {
                let group = |n: usize| match n {
                    0 => &hay[range.clone()],
                    n => matches.group(n).map_or("", |range| &hay[range]),
                };
                let mut new = String::new();
                self.replacement.apply(group, &mut new);
                let at = snapshot.anchor_after(start + range.start);
                self.waiting.push_back((at, range.len(), new));
                if !self.global {
                    break;
                }
            }
            deadline.check()?;
            self.row_end = Some(snapshot.anchor_before(start + hay.len()));
            self.row_changed = false;
            row += 1;
        }
        Ok(!self.waiting.is_empty())
    }

    /// The match asked about, as a range in the text.
    fn current(&self, model: &EditorModel) -> Option<Range<usize>> {
        let (at, len, _) = self.waiting.front()?;
        let start = model.snapshot().resolve(at);
        Some(start..start + len)
    }

    /// Selects the match asked about, the caret on its start, and asks.
    fn show(self: Box<Self>, target: &mut VimTarget<'_>) {
        if let Some(range) = self.current(target.model) {
            target.model.set_selections([ResolvedSelection {
                range,
                reversed: true,
            }]);
        }
        target.state.goals = None;
        target.state.message = Some(self.prompt.clone());
        target.state.confirm = Some(self);
    }

    /// Replaces the match asked about, in the undo step, leaving the caret
    /// at the start of the row its replacement ends on, as nvim does.
    fn replace(&mut self, target: &mut VimTarget<'_>, actor: &Actor) {
        let Some(range) = self.current(target.model) else {
            return;
        };
        let (_, _, new) = self.waiting.pop_front().expect("a match waits");
        let VimTarget { model, state, .. } = target;
        let row = model.text().offset_to_point(range.start).row;
        let before = model.text().clone();
        if let Some(rows) = &mut self.rows
            && let Some(taken) = rows.taken.take()
        {
            rows.walk.took(&rows.read, &taken, Took::Replaced);
            // The rows it joins go, and their marks with them, as nvim's do.
            if taken.joins() {
                let row = |at: usize| before.offset_to_point(at).row;
                let (first, last) = (row(range.start) + 1, row(range.end));
                state.marks.deleting(model.snapshot(), first, last);
                if last + 1 == before.row_count() {
                    visual::joining_at_end(state, model.snapshot(), first);
                }
            }
            Line::open(&mut rows.line, &rows.read, &taken).push_str(&new);
            if taken.joins() {
                let line = rows.line.take().expect("a line open");
                rows.write(line, None);
            }
            rows.taken = Some(taken);
        }
        state.marks.typing(model.snapshot(), [range.start]);
        let end = range.start + new.len();
        let (transaction, snapshot) = {
            let mut buffer = model.buffer_mut();
            let edits = joining(&before, range.clone(), new);
            let transaction = match self.transaction {
                // Undone or dropped from the history, it's a new one.
                Some(id) => buffer
                    .edit_in(id, edits.clone())
                    .unwrap_or_else(|_| buffer.edit(edits, origin(actor))),
                None => buffer.edit(edits, origin(actor)),
            };
            (transaction, buffer.snapshot())
        };
        operator::sync(model);
        if let Some(id) = transaction {
            // As nvim's undo and redo leave it: the first changed line's
            // start.
            state.carets.record(id, vec![(row, 0)]);
            self.transaction = Some(id);
        }
        let text = snapshot.text();
        let last_row = text.offset_to_point(end).row;
        let row_end = text.row_start(last_row) + text.row_len(last_row) as usize;
        self.row_end = Some(snapshot.anchor_before(row_end));
        operator::land(model, &snapshot, [text.row_start(last_row)]);
        self.last_row = model
            .text()
            .offset_to_point(model.primary_selection().head())
            .row;
        self.substitutions += 1;
        self.lines += usize::from(!self.row_changed);
        self.row_changed = true;
    }

    /// Ends the asking: one undo step holds every replacement, nvim's
    /// report is the message, and the caret stays where the last answer
    /// left it, or after `a` goes to the first non-blank of the last line
    /// changed.
    fn end(self, target: &mut VimTarget<'_>, all: bool) -> Dispatched<TransactionId> {
        let VimTarget { model, state, .. } = target;
        let head = match all {
            true => first_non_blank(model.text(), self.last_row),
            false => model.primary_selection().head(),
        };
        model.set_selections([operator::caret(head)]);
        state.goals = None;
        state.message = report(self.substitutions, self.lines, false);
        Dispatched::new(Effect::Changed).with_transactions(self.transaction.into_iter().collect())
    }
}

/// `vim.substitute_answer`: answers the `:s` asking ([`ask`]): `y`
/// replaces the match and asks about the next, `n` skips it, `a` replaces
/// it and every one after it, `l` replaces it and ends, and `q` ends. The
/// asking ends too once no match is left, and as `q` does if anything else
/// edited the text since the last answer. With no `:s` asking, it fails.
pub(crate) fn answer(target: &mut VimTarget<'_>, actor: &Actor, args: AnswerArgs) -> Handled {
    let Some(mut confirm) = target.state.confirm.take() else {
        return Err(CommandError::new("no :s is asking"));
    };
    operator::sync(target.model);
    if target.model.snapshot().version() != confirm.version {
        return Ok(confirm.end(target, false));
    }
    let deadline = Deadline::new();
    match args.answer {
        Answer::Yes => confirm.replace(target, actor),
        Answer::No => {
            confirm.waiting.pop_front();
            if let Some(rows) = &mut confirm.rows
                && let Some(taken) = &rows.taken
            {
                rows.walk.took(&rows.read, taken, Took::Passed);
            }
        }
        Answer::Last => {
            confirm.replace(target, actor);
            return Ok(confirm.end(target, false));
        }
        Answer::Quit => return Ok(confirm.end(target, false)),
        Answer::All => loop {
            confirm.replace(target, actor);
            match confirm.next(target.model, &deadline) {
                Ok(true) => {}
                Ok(false) => return Ok(confirm.end(target, true)),
                Err(Interrupted) => return Ok(interrupted(confirm.end(target, true), target)),
            }
        },
    }
    match confirm.next(target.model, &deadline) {
        Ok(true) => {
            confirm.version = target.model.snapshot().version();
            let changed = confirm.transaction;
            confirm.show(target);
            Ok(Dispatched::new(Effect::Changed).with_transactions(changed.into_iter().collect()))
        }
        Ok(false) => Ok(confirm.end(target, false)),
        Err(Interrupted) => Ok(interrupted(confirm.end(target, false), target)),
    }
}

/// `ended`, with the time limit's message instead of the report.
fn interrupted(
    ended: Dispatched<TransactionId>,
    target: &mut VimTarget<'_>,
) -> Dispatched<TransactionId> {
    target.state.message = Some(Interrupted.to_string());
    target.state.failed = true;
    ended
}

/// A key read while a `:s` asks ([`VimState::feed`](crate::VimState::feed)):
/// `y n a q l` answer, `<Esc>` and `<C-c>` as `q`, committing
/// `vim.substitute_answer`, and every other key does nothing, the prompt
/// staying, as nvim waits for an answer (its `<C-e>` and `<C-y>` scroll:
/// not here). `None` if nothing asks, or the mode isn't normal any more
/// (the host left it): the asking is then dropped, as `q` would end it.
pub(crate) fn confirm_key(state: &mut VimState, key: VimKey) -> Option<Step> {
    state.confirm.as_ref()?;
    if state.mode != Mode::Normal {
        state.confirm = None;
        return None;
    }
    state.failed = false;
    state.macros.count();
    state.macros.command_start();
    state.macros.record(key);
    let answer = match key {
        VimKey::Char('y') => Answer::Yes,
        VimKey::Char('n') => Answer::No,
        VimKey::Char('a') => Answer::All,
        VimKey::Char('q') | VimKey::Esc | VimKey::Ctrl('c') => Answer::Quit,
        VimKey::Char('l') => Answer::Last,
        _ => return Some(Step::Wait(Pending::new())),
    };
    Some(reducer::invocation(
        "vim.substitute_answer",
        AnswerArgs { answer },
    ))
}
