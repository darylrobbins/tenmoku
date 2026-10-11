//! Ex command lines, what `:` opens: a line is `[range] command [!] [args]`
//! (`:help cmdline-ranges`), parsed by [`Ex::parse`] without the buffer and
//! then run against it. The crate docs describe the ranges, `:d`, `:s` and
//! the host's commands. The commands ([`ExCommand`]):
//!
//! - `:{range}` alone, `:d[elete]`, `:noh[lsearch]`, `:s[ubstitute]`, `:&`
//!   and `:&&`, and the host's `:w :q :wq :x :e :qa :wa`.
//! - `:y[ank] [x] [count]`: the range's lines (the cursor's by default; with
//!   a count, that many from the range's last) yanked linewise into
//!   register `x` (`"` and `0` without one); the cursor stays.
//! - `:j[oin][!] [count]`: the range's lines joined as `J` joins them, or as
//!   they are with `!`. With no range, or one line and no count, a line
//!   joins the next; with a count, that many from the range's last. The
//!   caret goes to the joined line's first non-blank; with one line to join
//!   (the last, or a range of one with a count), nothing is joined and it
//!   only goes to that line, keeping its byte column.
//! - `:m[ove] {address}`: the range's lines moved below the address's line
//!   (`0` for the top; E134 for a line among them but the last, where
//!   nothing moves). The caret goes to the last line moved, keeping its goal
//!   column.
//! - `:t` and `:co[py] {address}`: the range's lines copied below the
//!   address's line; the caret goes to the last copy, keeping its goal
//!   column. For both, a `/pat/` or `?pat?` anywhere in the line's
//!   addresses makes where the caret lands its goal column, as in nvim, and
//!   what follows the address is ignored.
//! - `:>` and `:<`, repeated (`:>>>`) for more shifts, `[count]`: the
//!   range's lines (with a count, that many from its last) shifted as `>>`
//!   and `<<` shift them. A jump; the caret goes to the last line, at the
//!   first non-blank's column on the last line that changed (or is empty),
//!   its goal column left as it was.
//! - `:sor[t][!] [i] [n] [u] [r] [/pattern/]`: the range's lines (all by
//!   default) sorted, stably, as nvim sorts them: `!` the other way round,
//!   `i` ignoring the case of ASCII letters, `n` by the first decimal
//!   number (a `-` before it makes it negative; lines with none first), `u`
//!   keeping the first of a run of equal lines (equal ignoring case with
//!   `i`), and with a pattern (empty: the last search's) by what follows its
//!   first match on each line, or with `r` by the match (lines without one
//!   by nothing). The pattern doesn't become the last search. The caret
//!   goes to the first line's first non-blank. `n` twice is E474, a second
//!   pattern E475; `x`, `o`, `b`, `f` and `l` are refused, unsupported. Of
//!   one line, it does nothing, whatever its arguments.
//!
//! `:j`, `:>` and `:<` take the print flags `l`, `p` and `#` after their
//! count, and ignore them. Each command but `:y` (and a `:m` to where its
//! lines are, or a `:sort` of one line) is one transaction, even one that
//! leaves the text as it was, as in nvim; one `u` undoes it and `<C-r>`
//! redoes it, putting the caret where nvim's do (for `:m` and `:t`, redo's
//! place is noted apart from undo's). `.` doesn't repeat it. Only `:y` writes
//! registers.
//!
//! Known limits: marks on rows `:m` moves stay behind on their rows and
//! columns, `'<` and `'>` too (nvim's go with the rows): `:m` rewrites in
//! place each row from its range to its destination, and `:sort` each row
//! it reorders (there nvim's marks stay too). `'<` and `'>` on rows `:j`
//! joins stay where `J` leaves them.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched, Effect};

use crate::{
    Motion, MoveArgs, OperateArgs, Operator, Registers, Target, VimTarget, insert, marks,
    motion::{self, Cursor},
    operator,
    search::{self, Pattern, PatternError},
    substitute::{self, Substitute},
};

mod lines;

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
    /// `:y[ank] [x] [count]`: the range's lines (the count's from its
    /// last) yanked linewise into register `x`; the cursor stays.
    Yank {
        register: Option<char>,
        count: Option<u32>,
    },
    /// `:j[oin][!] [count]`: the range's lines joined (with one line, it
    /// and the next; with a count, that many from the range's last), as
    /// `J` joins them, or with `raw` (`!`) as they are.
    Join { raw: bool, count: Option<u32> },
    /// `:m[ove] {address}`: the range's lines moved below the address's
    /// line (`0` for the top).
    Move { to: Address },
    /// `:t` and `:co[py] {address}`: the range's lines copied below the
    /// address's line.
    Copy { to: Address },
    /// `:>` and `:<` (`left`), `times` of them (`:>>>`), `[count]`: the
    /// range's lines (the count's from its last) shifted as `>>` and `<<`
    /// shift them, `times` over.
    Shift {
        left: bool,
        times: u32,
        count: Option<u32>,
    },
    /// `:sor[t][!] [i] [n] [u] [r] [/pattern/]`: the range's lines (by
    /// default all) sorted, stably: `reverse` (`!`) the other way round,
    /// `ignore_case` (`i`) by ASCII letters' lower case, `numeric` (`n`)
    /// by the first decimal number in each (lines with none first);
    /// `unique` (`u`) keeps the first of a run of equal lines. With a
    /// pattern (empty: the last search's), each line sorts by what follows
    /// its first match, or with `by_match` (`r`) by the match (by nothing
    /// if none).
    Sort {
        reverse: bool,
        ignore_case: bool,
        numeric: bool,
        unique: bool,
        by_match: bool,
        pattern: Option<String>,
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

impl crate::VimState {
    /// Leaves the host's answer to a [`HostCommand`] (an error such as
    /// `E37`, or a report) as [`crate::VimState::message`], cleared as any
    /// message is. The key isn't failed: the line ran.
    pub fn set_host_message(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }
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
    /// `:m` to a line within the lines it moves.
    #[error("E134: Cannot move a range of lines into itself")]
    IntoItself,
    /// A letter `:sort` doesn't take, or its second pattern, and what
    /// follows it.
    #[error("E475: Invalid argument: {0}")]
    InvalidArgument(String),
    /// `:sort`'s second number kind (`n` twice).
    #[error("E474: Invalid argument")]
    TwoNumberKinds,
    /// `:sort`'s pattern with no closing delimiter: what follows the
    /// opening one.
    #[error("E654: Missing delimiter after search pattern: {0}")]
    MissingDelimiter(String),
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
        if args.starts_with('!') {
            return Err(ExError::NoBang);
        }
        check(true)?;
        return Ok(ExCommand::Substitute(Substitute::repeat(args, true)?));
    }
    if name.is_empty() && after.starts_with(['>', '<']) {
        return lines::shift(after, check);
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
        return lines::command(name, after, check)?.ok_or_else(not_a_command);
    }
    let after = uncommented(after);
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    if delete {
        check(true)?;
    }
    let args = after.trim_start_matches([' ', '\t']);
    let (command, args) = if delete {
        let (register, count, args) = register_and_count(args, true)?;
        (ExCommand::Delete { register, count }, args)
    } else {
        if !range.is_empty() {
            return Err(ExError::NoRange);
        }
        (ExCommand::NoHighlight, args)
    };
    trailing(command, args)
}

/// `args` without a comment: a `"` starts one, wherever it is in them.
fn uncommented(args: &str) -> &str {
    args.find('"').map_or(args, |at| &args[..at])
}

/// `command`, unless `args` holds more than blanks: E488.
fn trailing(command: ExCommand, args: &str) -> Result<ExCommand, ExError> {
    match args.trim_start_matches([' ', '\t']) {
        "" => Ok(command),
        args => Err(ExError::Trailing(args.to_string())),
    }
}

/// A `[x] [count]` (the register only if `register`, as `:d` and `:y` take
/// them) at the start of `args`, and what follows them.
fn register_and_count(
    args: &str,
    register: bool,
) -> Result<(Option<char>, Option<u32>, &str), ExError> {
    let mut args = args.trim_start_matches([' ', '\t']);
    // As in nvim, `#` writes `"` as `""dd` does.
    let name = args
        .chars()
        .next()
        .filter(|_| register)
        .and_then(|ch| match ch {
            '#' => Some('"'),
            _ if !ch.is_ascii_digit() && Registers::check(ch).is_ok() => Some(ch),
            _ => None,
        });
    if name.is_some() {
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
    Ok((name, count, &args[digits..]))
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

/// `args` without trailing blanks, except one right after a backslash
/// (whatever the backslash's parity, as nvim does).
fn trim_unescaped_end(args: &str) -> &str {
    let mut end = args.len();
    while let Some(blank) = args[..end].strip_suffix([' ', '\t']) {
        if blank.ends_with('\\') {
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
    });
    // As in nvim, `:sort` of one line ignores its arguments.
    let command = match command {
        Err(
            ExError::InvalidArgument(_)
            | ExError::TwoNumberKinds
            | ExError::MissingDelimiter(_)
            | ExError::Unsupported(_),
        ) if lines::is_sort(rest)
            && lines.map_or(last, |(first, end)| end - first.max(1) + 1) == 1 =>
        {
            return Ok(Ok(Dispatched::new(Effect::Notify)));
        }
        command => command?,
    };
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
            return substitute::run(target, actor, row(first)..row(end) + 1, &sub);
        }
        ExCommand::Yank { register, count } => {
            let (first, end) = counted_lines(target, lines, count, last)?;
            lines::yank(target, row(first)..=row(end), register, searched(&range))
        }
        ExCommand::Join { raw, count } => {
            let (first, end) = counted_lines(target, lines, count, last)?;
            // One line joins the next if no range was given, or one line
            // specifier and no count, as in nvim.
            let next = match range.as_slice() {
                [] => true,
                [one] => one.base != Base::All && count.is_none(),
                _ => false,
            };
            let end = match next && first == end {
                true => (end + 1).min(last),
                false => end,
            };
            lines::join(target, actor, row(first)..=row(end), raw)
        }
        ExCommand::Move { to: to_address } => {
            let (first, end) = counted_lines(target, lines, None, last)?;
            let to = destination(target, &to_address, last)?;
            if (first..end).contains(&i64::from(to)) {
                return Err(ExError::IntoItself);
            }
            let searched = searched(range.iter().chain([&to_address]));
            lines::move_rows(target, actor, row(first)..=row(end), to, searched)
        }
        ExCommand::Copy { to: to_address } => {
            let (first, end) = counted_lines(target, lines, None, last)?;
            let to = destination(target, &to_address, last)?;
            let searched = searched(range.iter().chain([&to_address]));
            lines::copy(target, actor, row(first)..=row(end), to, searched)
        }
        ExCommand::Shift { left, times, count } => {
            let (first, end) = counted_lines(target, lines, count, last)?;
            let rows = row(first)..=row(end);
            lines::shift_rows(target, actor, rows, left, times, searched(&range))
        }
        ExCommand::Sort { .. } => {
            let (first, end) = match lines {
                Some(_) => counted_lines(target, lines, None, last)?,
                None => (1, last),
            };
            return lines::sort(target, actor, row(first)..=row(end), &command);
        }
    })
}

/// Whether any of `addresses` searches for a pattern, which leaves the
/// caret of `:y`, `:m`, `:t`, `:>` and `:<` its own goal column, as in nvim.
fn searched<'a>(addresses: impl IntoIterator<Item = &'a Address>) -> bool {
    addresses.into_iter().any(|address| {
        address
            .parts
            .iter()
            .any(|part| matches!(part, Part::Search(_)))
    })
}

/// A line from 1 as a row from 0 (it's within the text).
fn row(line: i64) -> u32 {
    u32::try_from(line - 1).unwrap_or(u32::MAX)
}

/// The line `to` names (`:m` and `:t`'s, below which lines go), from 0
/// (before the first) to the last: E16 past them.
fn destination(target: &mut VimTarget<'_>, to: &Address, last: i64) -> Result<u32, ExError> {
    let line = resolve(target, to, cursor_line(target), last)?;
    match (0..=last).contains(&line) {
        true => Ok(u32::try_from(line).unwrap_or(u32::MAX)),
        false => Err(ExError::InvalidRange),
    }
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
