//! Ex command lines, what `:` opens: a line is `[range] command [!] [args]`
//! (`:help cmdline-ranges`), parsed by [`Ex::parse`] without the buffer and
//! then run against it. The crate docs list the commands and ranges.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched, Effect};

use crate::{
    Motion, MoveArgs, OperateArgs, Operator, Registers, Target, VimTarget, insert, marks,
    motion::{self, Cursor},
    operator,
    search::{self, Pattern, PatternError},
    substitute::{self, Substitute},
};

/// A parsed ex line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ex {
    /// The line specifiers, in order: none when the line gives no range,
    /// so the command takes its default (the cursor's line). Of several,
    /// the last two are the range; with one, it's both ends.
    pub range: Vec<Address>,
    pub command: ExCommand,
}

/// One line specifier: its base, then its searches and `+N` and `-N`
/// offsets in the order written, each applied to the line before it (a
/// bare `+` is `+1`, and a number after the rest is `+N`: `.2` is `.+2`).
/// `+2` alone is `.+2`, and `/x/` alone searches from the cursor's line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Address {
    pub base: Base,
    pub parts: Vec<Part>,
    /// A `;` follows it: the cursor goes to its line before the next
    /// specifier is read (a `,` leaves it).
    pub semicolon: bool,
}

/// Where a line specifier starts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Base {
    /// `.`, or nothing: the cursor's line.
    #[default]
    Current,
    /// `N`, from 1; 0 is before the first line (the first, for most
    /// commands). As in nvim it's 32 bits: past `2^31 - 1` it wraps (to a
    /// line before the first: E16), and past what 64 bits hold it's 0.
    Line(i32),
    /// `$`.
    Last,
    /// `%`: the whole text, `1,$`. As in nvim, searches and offsets after
    /// it are ignored, and with no command after them the line is E492.
    All,
    /// `'x`: the line of mark `x`: `a` to `z`, `'` (the jump mark), or `<`
    /// and `>`, the last visual extent's ends ([`crate::VimState::mark`]).
    Mark(char),
}

/// A step from a line specifier's line to another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Search(LineSearch),
    /// `+N` or `-N`, signed. Each is checked against 32 bits as it's
    /// applied, so a later one can't cancel an earlier overflow.
    Offset(i64),
}

/// A search for the next (or previous) line that matches, from a line,
/// wrapping round the text; the line it starts from comes last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineSearch {
    /// `/pat/` or `?pat?` (the closing delimiter optional at the line's
    /// end): the pattern becomes the last search, with its direction. An
    /// empty one is the last search's.
    Pattern { pattern: String, forward: bool },
    /// `\/` or `\?`: the last search's pattern.
    LastSearch { forward: bool },
    /// `\&`: the last substitute's pattern, forward (E33 before one): it
    /// becomes the last search.
    LastSubstitute,
}

/// What an ex line runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExCommand {
    /// `:{range}` alone: the cursor goes to the range's last line, keeping
    /// its goal column as `G` does. With no range it does nothing.
    Goto,
    /// `:d[elete] [x] [count]`: the range's lines (the count's from its
    /// last line) deleted linewise into register `x`, as `dd` does.
    Delete {
        register: Option<char>,
        count: Option<u32>,
    },
    /// `:noh[lsearch]`: no highlight until the next search.
    NoHighlight,
    /// `:s[ubstitute]`, `:&` and `:&&`: see [`Substitute`].
    Substitute(Substitute),
    /// A file or window command, which the host runs.
    Host(HostCommand),
}

/// A file or window command fe-vim parses but leaves to the host (it knows
/// no files, tabs or windows): [`crate::VimState::take_host_command`] hands
/// over the last one submitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCommand {
    /// `:w[rite]`: save the buffer to its file; `force` is `!`.
    Write { force: bool },
    /// `:q[uit]`: close the window; `force` (`!`) discards changes.
    Quit { force: bool },
    /// `:wq` (`only_if_modified` false) and `:x[it]`, `:exi[t]` (true):
    /// save, then close; `:x` writes only a modified buffer.
    WriteQuit { force: bool, only_if_modified: bool },
    /// `:e[dit] [path]`: open `path`, or with none reload the file, which
    /// with `force` (`!`) discards changes.
    Edit { path: Option<String>, force: bool },
    /// `:qa[ll]`, `:quita[ll]`: close every window.
    QuitAll { force: bool },
    /// `:wa[ll]`: save every modified buffer (`:wa!` is the same).
    WriteAll,
}

/// Why an ex line didn't run, as nvim says it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExError {
    /// The line, from its command (or range), isn't a command fe-vim knows.
    #[error("E492: Not an editor command: {0}")]
    NotACommand(String),
    #[error("E477: No ! allowed")]
    NoBang,
    #[error("E481: No range allowed")]
    NoRange,
    #[error("E488: Trailing characters: {0}")]
    Trailing(String),
    #[error("E939: Positive count required")]
    ZeroCount,
    /// A line before the first, or (but for [`ExCommand::Goto`], which
    /// stops at the last line) past the last.
    #[error("E16: Invalid range")]
    InvalidRange,
    /// An offset taking a line past 32 bits.
    #[error("E1247: Line number out of range")]
    OutOfRange,
    /// The range's first line is after its last. nvim asks whether to swap
    /// them, and headless nvim's answer is no.
    #[error("E493: Backwards range given")]
    Backwards,
    #[error("E20: Mark not set")]
    MarkNotSet,
    #[error("E35: No previous regular expression")]
    NoPreviousSearch,
    #[error("E33: No previous substitute regular expression")]
    NoPreviousSubstitute,
    #[error("E486: Pattern not found: {0}")]
    NotFound(String),
    /// A count of 2147483647 or more, as typed.
    #[error("E1510: Value too large: {0}")]
    ValueTooLarge(String),
    #[error("E146: Regular expressions can't be delimited by letters")]
    LetterDelimiter,
    /// `\n` in a replacement, which in Vim puts in a NUL.
    #[error("\\n in a replacement (a NUL): not supported")]
    NulInReplacement,
    /// `:w {path}`.
    #[error("{0}: not supported yet")]
    Unsupported(String),
    /// A pattern fe-vim can't search, or Vim rejects.
    #[error("{0}")]
    Pattern(String),
}

/// `:d[elete]` and the abbreviations `:help :d` lists (the `l` and `p`
/// forms list or print the lines too, which a host has nowhere to show).
const DELETE: &[&str] = &[
    "d", "de", "del", "dele", "delet", "delete", "dl", "dell", "delel", "deletl", "deletel", "dp",
    "dep", "delp", "delep", "deletp", "deletep",
];

impl Ex {
    /// Parses a `:` line's text (leading `:`s and blanks skipped). Pure:
    /// what the range's lines are is found when it runs.
    pub fn parse(line: &str) -> Result<Self, ExError> {
        let (line, range, rest) = split(line);
        let command = command(line, &range, rest, |_| Ok(()))?;
        Ok(Self { range, command })
    }
}

/// A line's text from its range, the range, and the text from its command.
fn split(line: &str) -> (&str, Vec<Address>, &str) {
    let line = line.trim_start_matches([':', ' ', '\t']);
    let mut parser = Parser { line, at: 0 };
    let range = parser.range();
    parser.skip_blanks();
    (line, range, &line[parser.at..])
}

/// The command `rest` names, after `range`, on `line`. `check` is called
/// once the command is recognised, before its arguments are read, as nvim
/// checks the range there: with whether both of the range's ends (not just
/// its last) matter to the command.
fn command(
    line: &str,
    range: &[Address],
    rest: &str,
    check: impl FnOnce(bool) -> Result<(), ExError>,
) -> Result<ExCommand, ExError> {
    let not_a_command = || ExError::NotACommand(line.to_string());
    let name_len = rest
        .find(|ch: char| !ch.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at(name_len);
    if name.is_empty()
        && let Some(args) = after.strip_prefix('&')
    {
        check(true)?;
        return Ok(ExCommand::Substitute(Substitute::repeat(args, true)?));
    }
    if name.is_empty() {
        let ignored = |address: &Address| {
            address.base == Base::All && address.parts.iter().any(|part| *part != Part::Offset(0))
        };
        let after = after.trim_start_matches([' ', '\t']);
        if !(after.is_empty() || after.starts_with('"')) || range.iter().any(ignored) {
            return Err(not_a_command());
        }
        check(false)?;
        return Ok(ExCommand::Goto);
    }
    if let Some(command) = host(name) {
        return host_command(command, range, after);
    }
    if "substitute".starts_with(name) {
        check(true)?;
        return Ok(ExCommand::Substitute(Substitute::parse(after)?));
    }
    let delete = DELETE.contains(&name);
    let nohlsearch = name.len() >= 3 && "nohlsearch".starts_with(name);
    if !delete && !nohlsearch {
        return Err(not_a_command());
    }
    // A `"` starts a comment, wherever it is in the arguments.
    let after = after.find('"').map_or(after, |at| &after[..at]);
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    if delete {
        check(true)?;
    }
    let mut args = after.trim_start_matches([' ', '\t']);
    let command = if delete {
        // As in nvim, `#` writes `"` as `""dd` does.
        let register = args.chars().next().and_then(|ch| match ch {
            '#' => Some('"'),
            _ if !ch.is_ascii_digit() && Registers::check(ch).is_ok() => Some(ch),
            _ => None,
        });
        if register.is_some() {
            args = args[1..].trim_start_matches([' ', '\t']);
        }
        let digits = args
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(args.len());
        let count = match digits {
            0 => None,
            _ => Some(
                number(&args[..digits])
                    .and_then(|count| u32::try_from(count).ok())
                    .unwrap_or(u32::MAX),
            ),
        };
        if count == Some(0) {
            return Err(ExError::ZeroCount);
        }
        args = args[digits..].trim_start_matches([' ', '\t']);
        ExCommand::Delete { register, count }
    } else {
        if !range.is_empty() {
            return Err(ExError::NoRange);
        }
        ExCommand::NoHighlight
    };
    match args.is_empty() {
        true => Ok(command),
        false => Err(ExError::Trailing(args.to_string())),
    }
}

/// Builds a host command from its `!` and its argument.
type Build = fn(bool, &str) -> HostCommand;

/// The host command `name` abbreviates (`:help :w`, `:q`, `:x`, `:exi`,
/// `:e`, `:qa`, `:wa`), with its `!` still to read.
fn host(name: &str) -> Option<Build> {
    let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name);
    Some(match name {
        _ if is("write", 1) => |force, _| HostCommand::Write { force },
        _ if is("quit", 1) => |force, _| HostCommand::Quit { force },
        "wq" => |force, _| HostCommand::WriteQuit {
            force,
            only_if_modified: false,
        },
        _ if is("xit", 1) || is("exit", 3) => |force, _| HostCommand::WriteQuit {
            force,
            only_if_modified: true,
        },
        _ if is("edit", 1) || name == "ex" => |force, path| {
            let path = (!path.is_empty()).then(|| path.to_string());
            HostCommand::Edit { path, force }
        },
        _ if is("qall", 2) || is("quitall", 5) => |force, _| HostCommand::QuitAll { force },
        _ if is("wall", 2) => |_, _| HostCommand::WriteAll,
        _ => return None,
    })
}

/// A host command's line: no range, and an argument only `:e` takes
/// (`:w {path}`, saving as, isn't supported yet).
fn host_command(build: Build, range: &[Address], after: &str) -> Result<ExCommand, ExError> {
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    let (force, args) = match after.strip_prefix('!') {
        Some(args) => (true, args),
        None => (false, after),
    };
    let args = args.trim_start_matches([' ', '\t']);
    let args = trim_unescaped_end(args);
    let command = build(force, &unescape(args));
    match (&command, args.is_empty()) {
        (_, true) | (HostCommand::Edit { .. }, _) => Ok(ExCommand::Host(command)),
        (HostCommand::Write { .. }, false) => Err(ExError::Unsupported(format!(":w {args}"))),
        (_, false) => Err(ExError::Trailing(args.to_string())),
    }
}

/// `args` without trailing blanks that aren't escaped by a backslash.
fn trim_unescaped_end(args: &str) -> &str {
    let mut end = args.len();
    while let Some(blank) = args[..end].strip_suffix([' ', '\t']) {
        let slashes = blank.bytes().rev().take_while(|&b| b == b'\\').count();
        if slashes % 2 == 1 {
            break;
        }
        end = blank.len();
    }
    &args[..end]
}

/// A file name as nvim reads it: a backslash escapes the character after
/// it (a blank, another backslash or any other) and is itself dropped; a
/// lone backslash at the end stays.
fn unescape(args: &str) -> String {
    let mut name = String::with_capacity(args.len());
    let mut chars = args.chars();
    while let Some(ch) = chars.next() {
        match (ch, chars.clone().next()) {
            ('\\', Some(escaped)) => {
                name.push(escaped);
                chars.next();
            }
            _ => name.push(ch),
        }
    }
    name
}

/// Digits as a number, or `None` past 64 bits.
fn number(digits: &str) -> Option<i64> {
    digits.bytes().try_fold(0i64, |n, digit| {
        n.checked_mul(10)?.checked_add(i64::from(digit - b'0'))
    })
}

struct Parser<'a> {
    line: &'a str,
    at: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.line[self.at..].chars().next()
    }

    fn rest(&self) -> &'a str {
        &self.line[self.at..]
    }

    fn skip_blanks(&mut self) {
        let rest = self.rest();
        self.at += rest.len() - rest.trim_start_matches([' ', '\t']).len();
    }

    fn digits(&mut self) -> Option<i64> {
        let rest = self.rest();
        let len = rest
            .find(|ch: char| !ch.is_ascii_digit())
            .unwrap_or(rest.len());
        self.at += len;
        number(&rest[..len])
    }

    /// The line specifiers and their separators. A separator with nothing
    /// before or after it stands for `.` (`:,` is `.,.`).
    fn range(&mut self) -> Vec<Address> {
        let mut range = Vec::new();
        loop {
            let address = self.address();
            self.skip_blanks();
            let semicolon = match self.peek() {
                Some(',') => false,
                Some(';') => true,
                _ => {
                    match address {
                        Some(address) => range.push(address),
                        None if !range.is_empty() => range.push(Address::default()),
                        None => {}
                    }
                    return range;
                }
            };
            self.at += 1;
            range.push(Address {
                semicolon,
                ..address.unwrap_or_default()
            });
        }
    }

    /// An offset's size: its digits, or 1 with none.
    fn offset_by(&mut self) -> i64 {
        match self.peek() {
            Some(ch) if ch.is_ascii_digit() => self.digits().unwrap_or(i64::MAX),
            _ => 1,
        }
    }

    /// One line specifier, or `None` if the line has none here.
    fn address(&mut self) -> Option<Address> {
        self.skip_blanks();
        let start = self.at;
        let mut address = Address::default();
        match self.peek() {
            // nvim's line numbers are 32 bits.
            Some(ch) if ch.is_ascii_digit() => {
                address.base = Base::Line(self.digits().unwrap_or(0) as i32);
            }
            Some('.') => self.at += 1,
            Some('$') => {
                self.at += 1;
                address.base = Base::Last;
            }
            Some('%') => {
                self.at += 1;
                address.base = Base::All;
            }
            Some('\'') => {
                if let Some(mark) = self.rest()[1..].chars().next() {
                    self.at += 1 + mark.len_utf8();
                    address.base = Base::Mark(mark);
                }
            }
            _ => {}
        }
        loop {
            let before = self.at;
            self.skip_blanks();
            let rest = self.rest();
            let part = match rest.chars().next() {
                Some(delimiter @ ('/' | '?')) => {
                    let (pattern, after) = search::split_search(&rest[1..], delimiter);
                    self.at = self.line.len() - after.map_or(0, str::len);
                    Part::Search(LineSearch::Pattern {
                        pattern,
                        forward: delimiter == '/',
                    })
                }
                _ if rest.starts_with("\\/") || rest.starts_with("\\?") => {
                    self.at += 2;
                    Part::Search(LineSearch::LastSearch {
                        forward: rest.starts_with("\\/"),
                    })
                }
                _ if rest.starts_with("\\&") => {
                    self.at += 2;
                    Part::Search(LineSearch::LastSubstitute)
                }
                Some(sign @ ('+' | '-')) => {
                    self.at += 1;
                    let by = self.offset_by();
                    Part::Offset(if sign == '-' { -by } else { by })
                }
                // A number after the rest is added to it.
                Some(ch) if ch.is_ascii_digit() && before > start => Part::Offset(self.offset_by()),
                _ => {
                    self.at = before;
                    break;
                }
            };
            address.parts.push(part);
        }
        (self.at > start).then_some(address)
    }
}

type Handled = Result<Dispatched<TransactionId>, CommandError>;

/// `vim.command_line_submit` for an ex line: parses and runs it. A line
/// that can't run leaves its error in [`crate::VimState::message`] and
/// changes nothing, but where a `;` in its range had already moved the
/// cursor, as nvim does. The line closing is at least an
/// [`Effect::Notify`].
pub(crate) fn run(target: &mut VimTarget<'_>, actor: &Actor, line: &str) -> Handled {
    target.state.host_command = None;
    let before = target.model.selections();
    let mut dispatched = match execute(target, actor, line) {
        Ok(handled) => handled?,
        Err(error) => target.state.fail(error.to_string()),
    };
    if dispatched.effect != Effect::Changed && target.model.selections() != before {
        dispatched.effect = Effect::Changed;
    }
    if dispatched.effect == Effect::Nothing {
        dispatched.effect = Effect::Notify;
    }
    Ok(dispatched)
}

/// Runs `line`. As in nvim, its range is read (searching, and moving the
/// cursor at a `;`) before its command is looked up.
fn execute(target: &mut VimTarget<'_>, actor: &Actor, line: &str) -> Result<Handled, ExError> {
    let (line, range, rest) = split(line);
    target.model.sync_display();
    let last = i64::from(target.model.text().row_count());
    let lines = lines(target, &range)?;
    // As in nvim, a line before the first fails once the command is
    // recognised, before its arguments are read; and a command that goes to
    // the range's last line doesn't mind its first.
    let command = command(line, &range, rest, |both| match lines {
        Some((first, end)) if end < 0 || both && first < 0 => Err(ExError::InvalidRange),
        _ => Ok(()),
    })?;
    Ok(match command {
        ExCommand::Goto => {
            // nvim doesn't move to its largest line number.
            if let Some((_, line)) = lines.filter(|&(_, line)| line < i64::from(i32::MAX)) {
                goto(target, line.clamp(1, last));
            }
            Ok(Dispatched::new(Effect::Changed))
        }
        ExCommand::NoHighlight => {
            target.state.highlight = None;
            Ok(Dispatched::new(Effect::Notify))
        }
        ExCommand::Host(command) => {
            target.state.host_command = Some(command);
            Ok(Dispatched::new(Effect::Notify))
        }
        ExCommand::Delete { register, count } => {
            let (first, end) = counted_lines(target, lines, count, last)?;
            delete(target, actor, first, end, register)
        }
        ExCommand::Substitute(sub) => {
            let (first, end) = counted_lines(target, lines, sub.count, last)?;
            // Lines from 1 as rows from 0: they're within the text.
            let row = |line: i64| u32::try_from(line - 1).unwrap_or(u32::MAX);
            return substitute::run(target, actor, row(first)..row(end) + 1, &sub);
        }
    })
}

/// The lines a command taking a range and a count acts on, from 1: the
/// range's (the cursor's line with none), or with a count that many from
/// its last, within the text.
fn counted_lines(
    target: &VimTarget<'_>,
    lines: Option<(i64, i64)>,
    count: Option<u32>,
    last: i64,
) -> Result<(i64, i64), ExError> {
    let (first, end) = lines.unwrap_or_else(|| {
        let line = cursor_line(target);
        (line, line)
    });
    if end > last {
        return Err(ExError::InvalidRange);
    }
    if first > end {
        return Err(ExError::Backwards);
    }
    let end = end.max(1);
    Ok(match count {
        Some(count) => (end, end.saturating_add(i64::from(count) - 1).min(last)),
        None => (first.max(1), end),
    })
}

/// The primary caret's line, from 1.
fn cursor_line(target: &VimTarget<'_>) -> i64 {
    let model = &target.model;
    let row = model
        .text()
        .offset_to_point(model.primary_selection().head())
        .row;
    i64::from(row) + 1
}

/// The range's first and last lines, from 1 (0 is before the first), or
/// `None` with no range. Reading a `;` moves the cursor to its line, or the
/// nearest there is.
fn lines(target: &mut VimTarget<'_>, range: &[Address]) -> Result<Option<(i64, i64)>, ExError> {
    if range.is_empty() {
        return Ok(None);
    }
    let last = i64::from(target.model.text().row_count());
    let mut current = cursor_line(target);
    let (mut first, mut end) = (current, current);
    for address in range {
        let line = resolve(target, address, current, last)?;
        if address.semicolon {
            current = line.clamp(0, last);
            goto(target, line.clamp(1, last));
        }
        (first, end) = match address.base {
            Base::All => (1, line),
            _ => (end, line),
        };
    }
    if let [address] = range
        && address.base != Base::All
    {
        first = end;
    }
    Ok(Some((first, end)))
}

/// One line specifier's line, from `current`, the cursor's: before the
/// first if negative.
fn resolve(
    target: &mut VimTarget<'_>,
    address: &Address,
    current: i64,
    last: i64,
) -> Result<i64, ExError> {
    let mut line = match address.base {
        Base::Current => current,
        Base::Line(line) => i64::from(line),
        Base::Last => last,
        Base::All => return Ok(last),
        Base::Mark(name) => {
            let snapshot = target.model.snapshot();
            let mark = target.state.mark(name).and_then(|mark| mark.get(snapshot));
            i64::from(mark.ok_or(ExError::MarkNotSet)?.0) + 1
        }
    };
    for part in &address.parts {
        line = match part {
            Part::Search(search) => search_line(target, search, line.clamp(0, last))?,
            Part::Offset(by) => offset(line, *by)?,
        };
    }
    Ok(line)
}

/// `line` moved by `by`, as nvim does it in 32 bits: an offset of
/// `2^31 - 1` or more, or a sum past the last number, is E1247, but one
/// going below the first number wraps round.
fn offset(line: i64, by: i64) -> Result<i64, ExError> {
    let max = i64::from(i32::MAX);
    match by {
        0 => Ok(line),
        by if by <= -max || by >= max => Err(ExError::OutOfRange),
        by if by < 0 => Ok(i64::from((line + by) as i32)),
        by if line >= 0 && by >= max - line => Err(ExError::OutOfRange),
        by => Ok(line + by),
    }
}

/// The next line after `from` (or the last before it) that `search`
/// matches, wrapping round the text, `from` itself last.
fn search_line(target: &mut VimTarget<'_>, search: &LineSearch, from: i64) -> Result<i64, ExError> {
    let last_search = || {
        target
            .registers
            .last_search()
            .map(|(pattern, _)| pattern.to_string())
            .ok_or(ExError::NoPreviousSearch)
    };
    let (pattern, forward) = match search {
        LineSearch::LastSubstitute => match target.registers.substitute.pattern() {
            Some(pattern) => (pattern.to_string(), true),
            None => return Err(ExError::NoPreviousSubstitute),
        },
        LineSearch::LastSearch { forward } => (last_search()?, *forward),
        LineSearch::Pattern { pattern, forward } if pattern.is_empty() => {
            (last_search()?, *forward)
        }
        LineSearch::Pattern { pattern, forward } => (pattern.clone(), *forward),
    };
    // As `/` and `?` do: the pattern becomes the last search even if
    // nothing matches or Vim rejects it, but not one fe-vim can't search.
    let keep = matches!(
        search,
        LineSearch::Pattern { .. } | LineSearch::LastSubstitute
    );
    let parsed = match Pattern::parse(&pattern) {
        Ok(parsed) => parsed,
        Err(error) => {
            if keep && matches!(error, PatternError::Invalid(_)) {
                target.registers.set_search(&pattern, forward);
                target.state.highlight = None;
            }
            return Err(ExError::Pattern(error.to_string()));
        }
    };
    if keep {
        target.registers.set_search(&pattern, forward);
    }
    target.state.highlight = Some(parsed.clone());
    let text = target.model.text();
    let rows = text.row_count();
    // `from` counts from 1, so as a row it's the line after it.
    let from = u32::try_from(from).unwrap_or(rows).min(rows);
    let matches = |row: &u32| !search::matches_in(text, *row..*row + 1, &parsed, 1).is_empty();
    let found = match forward {
        true => (from..rows).chain(0..from).find(matches),
        false => {
            let at = from.max(1) - 1;
            (0..at).rev().chain((at..rows).rev()).find(matches)
        }
    };
    found
        .map(|row| i64::from(row) + 1)
        .ok_or(ExError::NotFound(pattern))
}

/// One caret on `line` (from 1), as `{line}G` puts it: at the goal column
/// it kept.
fn goto(target: &mut VimTarget<'_>, line: i64) {
    let VimTarget { model, state, .. } = target;
    let primary = model.primary_selection();
    if model.selections().len() > 1 {
        model.set_selections([operator::caret(primary.head())]);
    }
    let count = u32::try_from(line).unwrap_or(u32::MAX);
    motion::apply(
        model,
        state,
        MoveArgs {
            motion: Motion::DocumentEnd,
            extend: false,
            count: Some(count),
        },
    );
}

/// `:d`: lines `first` to `end` (from 1) deleted as `dd` deletes them, in
/// one transaction, leaving the caret on the first non-blank of the line
/// that took their place (or the new last). Unlike `dd`, `.` doesn't
/// repeat it.
fn delete(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    first: i64,
    end: i64,
    register: Option<char>,
) -> Handled {
    // Nothing to delete, so nothing for the registers; the row's marks go.
    if target.model.text().is_empty() {
        target.state.marks.deleting(target.model.snapshot(), 0, 0);
        return Ok(Dispatched::new(Effect::Notify));
    }
    insert::adopt_closing(target.state);
    // A jump, as in nvim: from the cursor, before it goes to the lines.
    marks::jump_here(target);
    goto(target, first);
    let last_change = (target.state.last_change.clone(), target.state.changes);
    let count = u32::try_from(end - first + 1).unwrap_or(u32::MAX);
    let dispatched = operator::operate(
        target,
        actor,
        OperateArgs {
            operator: Operator::Delete,
            target: Target::Motion(Motion::Line),
            count: Some(count),
            register,
        },
    );
    (target.state.last_change, target.state.changes) = last_change;
    let dispatched = dispatched?;
    let VimTarget { model, state, .. } = target;
    model.sync_display();
    let text = model.text();
    // The operator anchored the caret to the line that took the lines'
    // place, whatever another origin inserted meanwhile.
    let row = text.offset_to_point(model.primary_selection().head()).row;
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.ix = cursor.line.first_non_blank();
    let caret = cursor.offset();
    model.set_selections([operator::caret(caret)]);
    state.goals = None;
    Ok(Dispatched {
        effect: Effect::Changed,
        ..dispatched
    })
}
