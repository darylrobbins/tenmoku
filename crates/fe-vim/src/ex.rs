//! Ex command lines, what `:` opens: a line is `[range] command [!] [args]`
//! (`:help cmdline-ranges`), parsed by [`Ex::parse`] without the buffer and
//! then run against it. The crate docs describe the ranges, `:d`, `:s` and
//! the host's commands. The commands ([`ExCommand`]):
//!
//! - `:{range}` alone, `:d[elete]`, `:noh[lsearch]`, `:s[ubstitute]`, `:&`
//!   and `:&&`, the host's `:w :q :wq :x :e :qa :wa`, its buffer list's
//!   `:b :bn :bp :bd :bf :bl` (`buffers.rs`), and its windows' `:sp :vs
//!   :new :vne :on :clo` (`windows.rs`).
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
//! - `:g[lobal][!]/{pattern}/[cmd]` and `:v[global]/{pattern}/[cmd]`: `cmd`
//!   (any of these; by default `:p`, which only moves the cursor to the
//!   last row's first non-blank) run with the cursor at the start of each
//!   row of the range (by default all) that matches (with `!` and `:v`,
//!   doesn't) and still exists, in order (`global.rs`). The delimiter is any
//!   character but a letter (E146); `\/`, `\?` and `\&` stand for the last
//!   search's and substitute's patterns, and an empty pattern is the last
//!   search's. The pattern becomes the last search, its direction kept. A
//!   `:g` inside it is E147, even without a range (nvim runs one on the one
//!   row). A command that fails stops it there with its error; `:s` finding
//!   nothing on a row doesn't. Nothing matching (or with `:v`, everything)
//!   is a message, not an error. The jump mark goes where the cursor was
//!   before (the commands it runs don't set it).
//! - `:norm[al][!] {keys}`: `keys` run as typed keys on the cursor's row, or
//!   on each line of the range, from its start (`normal.rs`).
//! - `:{range}!{cmd}` and `:[line]r[ead] !{cmd}`: the range's rows filtered
//!   through a shell command, or its output put below the line, which the
//!   host runs ([`HostCommand::Filter`], [`HostCommand::ReadCommand`]), only
//!   for the user's own keys (`filter.rs`).
//!
//! `:j`, `:>` and `:<` take the print flags `l`, `p` and `#` after their
//! count, and ignore them. Each command but `:y` (and a `:m` to where its
//! lines are, or a `:sort` of one line) is one transaction (a `:g` or
//! `:normal`, with what it runs, if it changes anything), even one that
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

use std::ops::Range;

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched, Effect};

use crate::{
    KeyError, Mode, Motion, MoveArgs, OperateArgs, Operator, Registers, SearchOffset, Target,
    VimKey, VimTarget,
    feature::filter,
    history, insert, marks,
    motion::{self, Cursor},
    operator,
    search::{self, Deadline, Pattern, PatternError},
    substitute::{self, Substitute},
};

mod buffers;
pub use buffers::{BufferAddress, BufferCommand, BufferTarget};
mod complete;
pub(crate) mod global;
mod lines;
pub(crate) mod normal;
mod pick;
pub use pick::{BufferRow, Pick, PickRow, PickThen};
mod table;
mod tabs;
pub use tabs::TabCommand;
mod windows;
pub(crate) use table::filter::refusal as shell_refusal;
pub use table::filter::{REFUSED as SHELL_REFUSED, ShellCommand, ShellPart};
pub use windows::WindowCommand;

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
    /// `/` or `?` ending the line: the last search's pattern, which becomes
    /// the last search with this direction as `//` does, but keeping the
    /// last search's offset, as in nvim.
    Repeat { forward: bool },
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
    /// `:g[lobal][!]/{pattern}/[cmd]` and `:v[global]/{pattern}/[cmd]`:
    /// `command` run on each row of the range (by default all) that
    /// `pattern` matches (with `invert`, as `!` and `:v` make it, doesn't).
    /// A [`LineSearch::Pattern`]'s direction means nothing here.
    Global {
        invert: bool,
        pattern: LineSearch,
        command: String,
    },
    /// `:norm[al][!] {keys}`: `keys` run on the cursor's row, or on each
    /// line of the range. `remap` is false for `:normal!`: its keys skip
    /// the maps.
    Normal { keys: Vec<VimKey>, remap: bool },
    /// `:noh[lsearch]`: no highlight until the next search.
    NoHighlight,
    /// `:s[ubstitute]`, `:&` and `:&&`: see [`Substitute`].
    Substitute(Substitute),
    /// A command of the table (`ex/table.rs`).
    Table(TableCommand),
    /// A file or window command, which the host runs.
    Host(HostCommand),
}

/// A command of the table, parsed: its entry's full name and arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCommand {
    name: &'static str,
    args: String,
}

impl TableCommand {
    /// Whether the command can edit the buffer.
    pub fn edits(&self) -> bool {
        table::named(self.name).is_some_and(|entry| entry.edits)
    }
}

/// A file or window command fe-vim parses but leaves to the host (it knows
/// no files, tabs or windows): [`crate::VimState::take_host_command`] hands
/// over the last one submitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostCommand {
    /// `:w[rite] [path]`: save the buffer to its file, or with a `path`
    /// write a copy there; `force` is `!`.
    Write { force: bool, path: Option<String> },
    /// `:sav[eas] {path}`: save the buffer as `path`, which it takes for
    /// its file.
    SaveAs { force: bool, path: String },
    /// `:q[uit]`: close the window; `force` (`!`) discards changes.
    Quit { force: bool },
    /// `:wq` (`only_if_modified` false) and `:x[it]`, `:exi[t]` (true):
    /// save, then close; `:x` writes only a modified buffer.
    WriteQuit { force: bool, only_if_modified: bool },
    /// `:e[dit] [path]`: open `path`, or with none reload the file, which
    /// with `force` (`!`) discards changes.
    Edit { path: Option<String>, force: bool },
    /// `:[line]r[ead] [path]`: put the lines of `path` (none: the buffer's
    /// own file) below line `below`, from 1, the range's last line (the
    /// cursor's with none); `0` puts them above the first line, as `:0r`.
    Read { path: Option<String>, below: u32 },
    /// `:qa[ll]`, `:quita[ll]`: close every window.
    QuitAll { force: bool },
    /// `:wa[ll]`: save every modified buffer (`:wa!` is the same).
    WriteAll,
    /// A command named with a capital (`:help user-commands`) that fe-vim
    /// has none of its own by: `:Gitsigns stage_hunk`, `:Git! commit`.
    /// `range` is the buffer rows the line's range names (`None` with no
    /// range, and `0..0`, the empty range at row 0, for exactly the address
    /// `0` as in `:0Gclog`); `args` the rest of the line, trimmed. The host answers `E492`
    /// for one it doesn't know.
    User {
        name: String,
        bang: bool,
        range: Option<Range<u32>>,
        args: String,
    },
    /// `:diffg[et]`: take the other buffer's side of `rows` (the cursor's
    /// row with no range).
    DiffGet { rows: Range<u32> },
    /// `:diffpu[t]`: give `rows` to the other buffer.
    DiffPut { rows: Range<u32> },
    /// `:diffs[plit] {file}`: diff against `path` in a split.
    DiffSplit { path: String },
    /// `:Ex[plore]`: the project tree, focused, with the buffer's file
    /// revealed and selected, as netrw opens the file's folder.
    Explore,
    /// `:ol[dfiles]` and `:bro[wse] ol[dfiles]`: the host's recent files,
    /// to choose one from, rather than a list printed.
    OldFiles,
    /// `:cope[n]` and `:lop[en]`: the problems list, shown and focused, as
    /// the quickfix and location windows are.
    OpenProblems,
    /// `:ccl[ose]` and `:lcl[ose]`: the problems list hidden, if shown.
    CloseProblems,
    /// `:Replace {text}`, capitalised as Vim's user commands are: the
    /// project search's matches replaced with `text` in every file, as
    /// its Replace All does and with its question. `text` is the rest of
    /// the line, trimmed; none is `E471`.
    ReplaceInFiles { text: String },
    /// A list the host shows to pick a row from (`:marks`); choosing one
    /// runs its `then` in the editor the line was typed in.
    Pick(Pick),
    /// `:b`, `:bn`, `:bp`, `:bd`, `:bf` and `:bl`: the host's buffers,
    /// its open files, switched or deleted (`ex/buffers.rs`).
    Buffer(BufferCommand),
    /// `:sp`, `:vs`, `:new`, `:vne`, `:on` and `:clo`: the host's windows,
    /// its panes, split or closed (`ex/windows.rs`).
    Window(WindowCommand),
    /// `:tabnew`, `:tabn`, `:tabp`, `:tabc`, `:tabo`, `:tabfir` and
    /// `:tabl`: the host's editor tabs, as VSCodeVim maps them
    /// (`ex/tabs.rs`).
    Tab(TabCommand),
    /// `:{range}!{cmd}`: `rows` (from 0) run through `command`, whose
    /// output takes their place ([`crate::VimState::apply_filter`]); asked
    /// for by `actor`, which fe-vim lets only [`Actor::User`] be, for the
    /// host to check again. Parsed, its rows and actor are filled in as it
    /// runs.
    Filter {
        rows: Range<u32>,
        command: ShellCommand,
        actor: Actor,
    },
    /// `:[line]r[ead] !{cmd}` (and `:r!{cmd}`): `command`'s output put below
    /// line `below` as [`HostCommand::Read`] puts a file's
    /// ([`crate::VimState::apply_read_command`]), for `actor`, as
    /// [`HostCommand::Filter`].
    ReadCommand {
        below: u32,
        command: ShellCommand,
        actor: Actor,
    },
}

impl HostCommand {
    /// Whether it runs a shell command ([`HostCommand::Filter`],
    /// [`HostCommand::ReadCommand`]).
    pub fn runs_shell(&self) -> bool {
        matches!(self, Self::Filter { .. } | Self::ReadCommand { .. })
    }
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
    /// `:g` with a backslash before anything but `/`, `?` or `&`.
    #[error("E10: \\ should be followed by /, ? or &")]
    BackslashDelimiter,
    #[error("E148: Regular expression missing from global")]
    NoGlobalPattern,
    /// A `:g` inside a `:g`.
    #[error("E147: Cannot do :global recursive with a range")]
    Recursive,
    #[error("E471: Argument required")]
    ArgumentRequired,
    /// `:normal`s running inside each other past a limit.
    #[error("E192: Recursive use of :normal too deep")]
    NormalTooDeep,
    /// `:normal`'s keys name a key there isn't.
    #[error(transparent)]
    Keys(#[from] KeyError),
    /// `\n` in a replacement, which in Vim puts in a NUL.
    #[error("\\n in a replacement (a NUL): not supported")]
    NulInReplacement,
    /// `:w {path}`.
    #[error("{0}: not supported yet")]
    Unsupported(String),
    /// A pattern fe-vim can't search, or Vim rejects.
    #[error("{0}")]
    Pattern(String),
    /// A shell command asked for by anyone but the user, or run by `:g` or
    /// `:normal` (`ex/filter.rs`).
    #[error("Shell commands run only from your own keys")]
    ShellRefused,
    /// `:{range}!` repeating the last shell command, with none.
    #[error("E34: No previous command")]
    NoPreviousCommand,
    /// `:!{cmd}` with no range, whose output has nowhere to go here.
    #[error(":!{{cmd}} without a range isn't run here: use the terminal panel")]
    NoRangeShell,
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
        // Run, a `:g` inside one fails there (as nvim's does with a range).
        if let ExCommand::Global { command, .. } = &command
            && global::nested(command)
        {
            return Err(ExError::Recursive);
        }
        Ok(Self { range, command })
    }
}

impl Ex {
    /// The commands `line` holds, cut where nvim cuts them (`:help :bar`):
    /// at a `|` that isn't escaped (`\|` stays), but not in the argument of
    /// a command that takes the rest of the line (`:g`, `:v`, `:normal`,
    /// the host's and the capitalised ones), nor in `:s`'s pattern and
    /// replacement, and not after a `"` comment, which runs to the end. A
    /// line with no bar is its one command, as it was given.
    pub fn commands(line: &str) -> Vec<&str> {
        let mut pieces = Vec::new();
        let mut rest = line;
        while let Some(bar) = command_end(rest) {
            pieces.push(rest[..bar].trim_end_matches([' ', '\t']));
            rest = &rest[bar + 1..];
        }
        pieces.push(rest);
        pieces
    }
}

/// Where the first command of `piece` ends: the byte of the `|` that
/// separates it from the next, if there is one.
fn command_end(piece: &str) -> Option<usize> {
    let (_, _, rest) = split(piece);
    let start = piece.len() - rest.len();
    let name_len = rest
        .find(|ch: char| !ch.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at(name_len);
    // A filter's command is the shell's, `|` and all.
    let filter = name.is_empty() && after.starts_with('!');
    let takes_the_rest = filter
        || !name.is_empty()
            && (host(name).is_some()
                || buffers::kind(name).is_some()
                || windows::kind(name).is_some()
                || tabs::kind(name).is_some()
                || name == "Replace"
                || is_diff(name)
                || global::kind(name).is_some()
                || normal::is_normal(name)
                || rest.starts_with(|ch: char| ch.is_ascii_uppercase()));
    if takes_the_rest {
        return None;
    }
    // A modifier: the command after it decides.
    if name.len() >= 3 && "browse".starts_with(name) {
        let command = after.trim_start_matches([' ', '\t']);
        if command.len() == after.len() {
            return None;
        }
        return command_end(command).map(|at| piece.len() - command.len() + at);
    }
    let substitute =
        !is_substitute_with_flags(name) && !name.is_empty() && "substitute".starts_with(name);
    let scan = match name {
        _ if substitute => after_substitute_body(after),
        _ if lines::is_sort(name) => after_sort_pattern(after),
        _ => after,
    };
    let mut chars = scan.char_indices();
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '"' => return None,
            '|' => return Some(piece.len() - scan.len() + at).filter(|&at| at >= start),
            _ => {}
        }
    }
    None
}

/// What follows `:s`'s pattern and replacement in `after` (what follows its
/// name), which are its own up to their delimiters, read as
/// [`Substitute::parse`] reads them; `after` without its leading blanks if
/// it has none (flags and a count for the last substitute).
fn after_substitute_body(after: &str) -> &str {
    let args = after.trim_start_matches([' ', '\t']);
    let Some(delimiter) = args.chars().next() else {
        return args;
    };
    let (delimiter, tail) = match delimiter {
        '\\' => match args[1..].chars().next() {
            Some(delimiter @ ('/' | '?' | '&')) => (delimiter, &args[2..]),
            _ => return args,
        },
        _ if delimiter.is_ascii_alphanumeric() || matches!(delimiter, '"' | '|') => return args,
        _ => {
            let rest = &args[delimiter.len_utf8()..];
            match search::split_search(rest, delimiter) {
                (_, Some(tail)) => (delimiter, tail),
                (_, None) => return "",
            }
        }
    };
    let mut chars = tail.char_indices();
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            _ if ch == delimiter => return &tail[at + ch.len_utf8()..],
            _ => {}
        }
    }
    ""
}

/// What follows `:sort`'s flags and pattern in `after`.
fn after_sort_pattern(after: &str) -> &str {
    // Only a `!` right after the name is a bang; later it may delimit the
    // pattern.
    let mut args = after.strip_prefix('!').unwrap_or(after);
    loop {
        let flags = args.trim_start_matches([' ', '\t']);
        match flags.chars().next() {
            None | Some('"' | '|') => return flags,
            Some(ch) if ch.is_ascii_alphabetic() => args = &flags[1..],
            Some(delimiter) => {
                match search::split_search(&flags[delimiter.len_utf8()..], delimiter).1 {
                    Some(tail) => args = tail,
                    None => return "",
                }
            }
        }
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

/// Whether `name` is `:s` with flags (`:sg`, `:sIe`), by Vim's rule in
/// `find_ex_command`: its second letter is `g` or `I`, `c` (but not `scs`,
/// `scr`, `sc?ip`), `i` (but not `sim`, `sil`, `sig`) or `r` (but not `sre`).
fn is_substitute_with_flags(name: &str) -> bool {
    let b = name.as_bytes();
    let third = b.get(2).copied();
    match b {
        [b's', b'g' | b'I', ..] => true,
        [b's', b'c', ..] => {
            !matches!(third, Some(b's' | b'r'))
                && !(b.get(3) == Some(&b'i') && b.get(4) == Some(&b'p'))
        }
        [b's', b'i', ..] => !matches!(third, Some(b'm' | b'l' | b'g')),
        [b's', b'r', ..] => !name.starts_with("sre"),
        _ => false,
    }
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
    if name.is_empty()
        && let Some(args) = after.strip_prefix('~')
    {
        if args.starts_with('!') {
            return Err(ExError::NoBang);
        }
        check(true)?;
        let mut sub = Substitute::repeat(args, true)?;
        sub.flags.search_pattern = true;
        return Ok(ExCommand::Substitute(sub));
    }
    if name.is_empty() && after.starts_with(['>', '<']) {
        return lines::shift(after, check);
    }
    if name.is_empty()
        && let Some(command) = after.strip_prefix('!')
    {
        return table::filter::parse(range, command, check);
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
    if name.len() >= 3 && "browse".starts_with(name) {
        // A modifier, which a terminal ignores but for `:oldfiles`, whose
        // list is the host's picker either way.
        let command = after.trim_start_matches([' ', '\t']);
        return match command.len() < after.len() {
            _ if command.is_empty() => Err(ExError::ArgumentRequired),
            true => self::command(line, range, command, check),
            false => Err(not_a_command()),
        };
    }
    // Shared by the commands that take it, one of which (`lines`) may find
    // the name isn't its own. Wrapped after `:browse`, which recurses.
    let mut check = Some(check);
    let mut check = |both| check.take().map_or(Ok(()), |check| check(both));
    if let Some(kind) = buffers::kind(name) {
        return buffers::parse(kind, range, after);
    }
    if let Some(kind) = windows::kind(name) {
        return windows::parse(kind, range, after);
    }
    if let Some(kind) = tabs::kind(name) {
        return tabs::parse(kind, range, after);
    }
    if let Some(command) = host(name) {
        return host_command(command, range, after);
    }
    if name == "Replace" && !after.starts_with(|ch: char| ch.is_ascii_alphanumeric()) {
        return replace_in_files(range, after);
    }
    if is_diff(name) {
        return diff_command(name, range, after, check);
    }
    if let Some(vglobal) = global::kind(name) {
        return global::parse(vglobal, after, check);
    }
    if normal::is_normal(name) {
        return normal::parse(after, check);
    }
    if is_substitute_with_flags(name) {
        check(true)?;
        let flags = format!("{}{after}", &name[1..]);
        return Ok(ExCommand::Substitute(Substitute::repeat(&flags, false)?));
    }
    if "substitute".starts_with(name) {
        check(true)?;
        // A comment ends it, wherever its pattern and replacement end.
        let tail = after_substitute_body(after);
        let end = after.len() - tail.len() + tail.find('"').unwrap_or(tail.len());
        return Ok(ExCommand::Substitute(Substitute::parse(&after[..end])?));
    }
    // After fe-vim's own capitalised commands, which would match above.
    if rest.starts_with(|ch: char| ch.is_ascii_uppercase()) {
        return user_command(rest, check);
    }
    let delete = DELETE.contains(&name);
    let nohlsearch = name.len() >= 3 && "nohlsearch".starts_with(name);
    if !delete && !nohlsearch {
        if let Some(command) = lines::command(name, after, &mut check)? {
            return Ok(command);
        }
        let entry = table::find(name).ok_or_else(not_a_command)?;
        let args = (entry.parse)(after, &mut check)?;
        return Ok(ExCommand::Table(TableCommand {
            name: entry.name,
            args,
        }));
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
/// `:e`, `:qa`, `:wa`, `:ol`, `:cope`, `:ccl`, `:lop`, `:lcl`, and netrw's
/// `:Ex`), with its `!` still to read.
fn host(name: &str) -> Option<Build> {
    let is = |full: &str, min: usize| name.len() >= min && full.starts_with(name);
    Some(match name {
        _ if is("write", 1) => |force, path| {
            let path = (!path.is_empty()).then(|| path.to_string());
            HostCommand::Write { force, path }
        },
        _ if is("saveas", 3) => |force, path| HostCommand::SaveAs {
            force,
            path: path.to_string(),
        },
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
        _ if is("read", 1) => |_, path| {
            let path = (!path.is_empty()).then(|| path.to_string());
            HostCommand::Read { path, below: 0 }
        },
        _ if is("qall", 2) || is("quitall", 5) => |force, _| HostCommand::QuitAll { force },
        _ if is("wall", 2) => |_, _| HostCommand::WriteAll,
        _ if is("Explore", 2) => |_, _| HostCommand::Explore,
        _ if is("oldfiles", 2) => |_, _| HostCommand::OldFiles,
        _ if is("copen", 4) || is("lopen", 3) => |_, _| HostCommand::OpenProblems,
        _ if is("cclose", 3) || is("lclose", 3) => |_, _| HostCommand::CloseProblems,
        _ => return None,
    })
}

/// `:diffget`, `:diffput` and `:diffsplit`; the rows are filled in when
/// they run, from the range.
fn diff_command(
    name: &str,
    range: &[Address],
    after: &str,
    check: impl FnOnce(bool) -> Result<(), ExError>,
) -> Result<ExCommand, ExError> {
    if is_diff_name(name, "diffsplit", 5) && !range.is_empty() {
        return Err(ExError::NoRange);
    }
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    let args = trim_unescaped_end(after.trim_start_matches([' ', '\t']));
    if is_diff_name(name, "diffsplit", 5) {
        if args.is_empty() {
            return Err(ExError::ArgumentRequired);
        }
        let path = unescape(args);
        return Ok(ExCommand::Host(HostCommand::DiffSplit { path }));
    }
    check(true)?;
    if !args.is_empty() {
        return Err(ExError::Trailing(args.to_string()));
    }
    let rows = 0..0;
    Ok(ExCommand::Host(match is_diff_name(name, "diffget", 5) {
        true => HostCommand::DiffGet { rows },
        false => HostCommand::DiffPut { rows },
    }))
}

/// Whether `name` abbreviates `full` to at least `min` letters.
fn is_diff_name(name: &str, full: &str, min: usize) -> bool {
    name.len() >= min && full.starts_with(name)
}

/// Whether `name` is `:diffget`, `:diffput` or `:diffsplit`.
fn is_diff(name: &str) -> bool {
    is_diff_name(name, "diffget", 5)
        || is_diff_name(name, "diffput", 6)
        || is_diff_name(name, "diffsplit", 5)
}

/// A user command (`:help user-commands`) at the start of `rest`: a capital
/// then letters and digits.
fn user_command(
    rest: &str,
    check: impl FnOnce(bool) -> Result<(), ExError>,
) -> Result<ExCommand, ExError> {
    let end = rest
        .find(|ch: char| !ch.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at(end);
    let (bang, args) = match after.strip_prefix('!') {
        Some(args) => (true, args),
        None => (false, after),
    };
    check(true)?;
    Ok(ExCommand::Host(HostCommand::User {
        name: name.to_string(),
        bang,
        range: None,
        args: args.trim_matches([' ', '\t']).to_string(),
    }))
}

/// A host command's line: no range, and an argument only `:e`, `:w` and
/// `:sav` take (`:sav` needs one). The day-one commands (`:Ex`, `:ol`,
/// `:cope`, `:ccl`) take no `!`.
fn host_command(build: Build, range: &[Address], after: &str) -> Result<ExCommand, ExError> {
    let (force, args) = match after.strip_prefix('!') {
        Some(args) => (true, args),
        None => (false, after),
    };
    let args = args.trim_start_matches([' ', '\t']);
    let shell = args;
    let args = trim_unescaped_end(args);
    let command = build(force, &unescape(args));
    // `:r` alone takes a range, as does the shell's output (`:r !cmd`,
    // `:r! cmd`).
    if let HostCommand::Read { .. } = command {
        return match (force, shell.strip_prefix('!')) {
            (true, _) => table::filter::parse_read(shell),
            (false, Some(shell)) => table::filter::parse_read(shell),
            (false, None) => Ok(ExCommand::Host(command)),
        };
    }
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    let bangless = matches!(
        command,
        HostCommand::Explore
            | HostCommand::OldFiles
            | HostCommand::OpenProblems
            | HostCommand::CloseProblems
    );
    if force && bangless {
        return Err(ExError::NoBang);
    }
    match (&command, args.is_empty()) {
        (HostCommand::SaveAs { .. }, true) => Err(ExError::ArgumentRequired),
        (_, true)
        | (HostCommand::Edit { .. } | HostCommand::Write { .. } | HostCommand::SaveAs { .. }, _) => {
            Ok(ExCommand::Host(command))
        }
        (_, false) => Err(ExError::Trailing(args.to_string())),
    }
}

/// `:Replace {text}`: no range or `!`, and some text.
fn replace_in_files(range: &[Address], after: &str) -> Result<ExCommand, ExError> {
    if !range.is_empty() {
        return Err(ExError::NoRange);
    }
    if after.starts_with('!') {
        return Err(ExError::NoBang);
    }
    match after.trim_matches([' ', '\t']) {
        "" => Err(ExError::ArgumentRequired),
        text => Ok(ExCommand::Host(HostCommand::ReplaceInFiles {
            text: text.to_string(),
        })),
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
            // `*` is `'<,'>`, the last visual area, as with nvim's default
            // 'cpoptions'. It is the whole range: what came before is
            // dropped, and what follows is the command.
            if self.peek() == Some('*') {
                self.at += 1;
                return ['<', '>']
                    .map(|mark| Address {
                        base: Base::Mark(mark),
                        ..Address::default()
                    })
                    .to_vec();
            }
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
                    let forward = delimiter == '/';
                    match (pattern.is_empty(), after) {
                        (true, None) => Part::Search(LineSearch::Repeat { forward }),
                        _ => Part::Search(LineSearch::Pattern { pattern, forward }),
                    }
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
    let row = filter::caret_row(target);
    filter::submitting(target.state, line, row);
    let before = target.model.selections();
    let executed = execute(target, actor, line);
    filter::ran(target.state);
    let mut dispatched = match executed {
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

thread_local! {
    /// How many `:g` are running: their commands' `:s` finding nothing
    /// doesn't stop the commands after it.
    static IN_GLOBAL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Counts a `:g` running until dropped.
struct InGlobal;

impl InGlobal {
    fn enter() -> Self {
        IN_GLOBAL.set(IN_GLOBAL.get() + 1);
        Self
    }
}

impl Drop for InGlobal {
    fn drop(&mut self) {
        IN_GLOBAL.set(IN_GLOBAL.get() - 1);
    }
}

/// Runs `line`: its commands in turn (`|` separates them), as one undo
/// step, stopping at the first that fails, which leaves its error.
fn execute(target: &mut VimTarget<'_>, actor: &Actor, line: &str) -> Result<Handled, ExError> {
    let pieces = Ex::commands(line);
    if let [piece] = pieces.as_slice() {
        return execute_one(target, actor, piece);
    }
    let group = global::Group::begin(target, actor);
    let mut outcome = Ok(());
    for piece in pieces {
        let before = history::pairs(target.model);
        target.state.failed = false;
        let ran = execute_one(target, actor, piece);
        global::Group::note(target, before);
        match ran {
            Ok(Ok(_)) if !target.state.failed => {}
            // A failed command leaves its message, and the rest don't run.
            Ok(Ok(_)) => break,
            Ok(Err(error)) => {
                outcome = Err(error);
                break;
            }
            // `:s` finding nothing isn't an error under `:g`, as in nvim.
            Err(ExError::NotFound(_))
                if IN_GLOBAL.get() > 0
                    && matches!(
                        Ex::parse(piece),
                        Ok(Ex {
                            command: ExCommand::Substitute(_),
                            ..
                        })
                    ) => {}
            Err(error) => {
                target.state.fail(error.to_string());
                break;
            }
        }
    }
    let transactions: Vec<TransactionId> = group.end(target).into_iter().collect();
    if let Err(error) = outcome {
        return Ok(Err(error));
    }
    Ok(Ok(
        Dispatched::new(Effect::Changed).with_transactions(transactions)
    ))
}

/// Runs `line`, one command. As in nvim, its range is read (searching, and moving the
/// cursor at a `;`) before its command is looked up.
fn execute_one(target: &mut VimTarget<'_>, actor: &Actor, line: &str) -> Result<Handled, ExError> {
    let (line, range, rest) = split(line);
    target.model.sync_display();
    let last = i64::from(target.model.text().row_count());
    // A search in the range that finds nothing fails the line even under
    // `:g`, where `:s` finding nothing is not an error.
    let lines = match lines(target, &range) {
        Err(error @ ExError::NotFound(_)) => return Ok(Ok(target.state.fail(error.to_string()))),
        lines => lines?,
    };
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
            error @ (ExError::InvalidArgument(_)
            | ExError::TwoNumberKinds
            | ExError::MissingDelimiter(_)
            | ExError::Unsupported(_)),
        ) if lines::is_sort(rest) => {
            // The range is validated and normalized (line 0 is the first)
            // as `:sort`'s own would be; no range sorts the whole text.
            let (first, end) = match lines {
                Some(_) => counted_lines(target, lines, None, last)?,
                None => (1, last),
            };
            if end - first + 1 != 1 {
                return Err(error);
            }
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
        ExCommand::Table(TableCommand { name, args }) => {
            let entry = table::named(name).ok_or_else(|| ExError::NotACommand(line.to_string()))?;
            return (entry.run)(target, actor, lines, &range, &args);
        }
        ExCommand::NoHighlight => {
            target.state.highlight = None;
            Ok(Dispatched::new(Effect::Notify))
        }
        ExCommand::Host(HostCommand::Buffer(_))
            if global::running() || target.state.normal_suspended.is_some() =>
        {
            // The host switches once the keys have run, so the rest would
            // run in the buffer left (`ex/buffers.rs`).
            return Err(ExError::Unsupported(
                "a buffer command in :g or :normal".into(),
            ));
        }
        ExCommand::Host(HostCommand::Window(_) | HostCommand::Tab(_))
            if global::running() || target.state.normal_suspended.is_some() =>
        {
            // As a buffer command: the rest would run in the window left
            // (`ex/windows.rs`).
            return Err(ExError::Unsupported(
                "a window command in :g or :normal".into(),
            ));
        }
        ExCommand::Host(mut command) => {
            let rows = |target: &VimTarget<'_>| {
                counted_lines(target, lines, None, last)
                    .map(|(first, end)| row(first)..row(end) + 1)
            };
            match &mut command {
                // `:0Gclog`: the single address 0 stays the empty range at
                // row 0, which `rows` would clamp to line 1.
                HostCommand::User { range, .. } if lines == Some((0, 0)) => *range = Some(0..0),
                HostCommand::User { range, .. } if lines.is_some() => *range = Some(rows(target)?),
                HostCommand::Read { below, .. } => *below = read_below(target, lines, last)?,
                HostCommand::ReadCommand {
                    below,
                    command,
                    actor: by,
                } => {
                    shell_refusal(target.state, actor)?;
                    *below = read_below(target, lines, last)?;
                    table::filter::fill(target.state, command)?;
                    *by = actor.clone();
                }
                HostCommand::Filter {
                    rows: at,
                    command,
                    actor: by,
                } => {
                    shell_refusal(target.state, actor)?;
                    *at = rows(target)?;
                    table::filter::fill(target.state, command)?;
                    *by = actor.clone();
                    filter::submitted(target.state, command);
                }
                HostCommand::DiffGet { rows: at } | HostCommand::DiffPut { rows: at } => {
                    *at = rows(target)?;
                }
                _ => {}
            }
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
            let from = target.model.buffer().version();
            let joined = lines::join(target, actor, row(first)..=row(end), raw);
            let until = target.model.buffer().version();
            global::noted(from, until, global::Change::Joined);
            joined
        }
        ExCommand::Move { to: to_address } => {
            let (first, end) = counted_lines(target, lines, None, last)?;
            let to = destination(target, &to_address, last)?;
            if (first..end).contains(&i64::from(to)) {
                return Err(ExError::IntoItself);
            }
            let searched = searched(range.iter().chain([&to_address]));
            let from = target.model.buffer().version();
            let moved = lines::move_rows(target, actor, row(first)..=row(end), to, searched);
            let until = target.model.buffer().version();
            let rows = row(first)..=row(end);
            global::noted(from, until, global::Change::Moved { rows, to });
            moved
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
        ExCommand::Global {
            invert,
            pattern,
            command,
        } => {
            let (first, end) = match lines {
                Some(_) => counted_lines(target, lines, None, last)?,
                None => (1, last),
            };
            let _running = InGlobal::enter();
            return global::run(
                target,
                actor,
                row(first)..=row(end),
                invert,
                &pattern,
                &command,
            );
        }
        ExCommand::Normal { keys, remap } => {
            let rows = match lines {
                Some(_) => {
                    let (first, end) = counted_lines(target, lines, None, last)?;
                    Some(row(first)..=row(end))
                }
                None => None,
            };
            return normal::run(target, actor, rows, &keys, remap);
        }
        ExCommand::Sort { .. } => {
            let (first, end) = match lines {
                Some(_) => counted_lines(target, lines, None, last)?,
                None => (1, last),
            };
            let (from, rows_before) = (target.model.buffer().version(), last);
            let sorted = lines::sort(target, actor, row(first)..=row(end), &command);
            let buffer = target.model.buffer();
            let by = i64::from(buffer.text().row_count()) - rows_before;
            let rows = row(first)..=row(end);
            global::noted(from, buffer.version(), global::Change::Sorted { rows, by });
            return sorted;
        }
    })
}

/// The line `:r` puts its rows below, from `lines`, the range's: its last
/// (the cursor's with none), or `0` for the top. A line before the first,
/// or a backwards range, fails even where the last line is 0.
fn read_below(
    target: &VimTarget<'_>,
    lines: Option<(i64, i64)>,
    last: i64,
) -> Result<u32, ExError> {
    if lines.is_some_and(|(first, end)| first < 0 || end < 0) {
        return Err(ExError::InvalidRange);
    }
    let (_, end) = counted_lines(target, lines, None, last)?;
    Ok(match lines {
        Some((_, 0)) => 0,
        _ => end as u32,
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

/// Whether `search`'s pattern is `*`'s or `#`'s, which 'smartcase'
/// doesn't apply to: one taken from the last search or the last substitute
/// keeps theirs, as nvim's does.
pub(crate) fn search_word(registers: &Registers, search: &LineSearch) -> bool {
    match search {
        LineSearch::LastSubstitute => registers.substitute.word(),
        LineSearch::LastSearch { .. } | LineSearch::Repeat { .. } => registers.last_search_word(),
        LineSearch::Pattern { pattern, .. } => pattern.is_empty() && registers.last_search_word(),
    }
}

/// An address's own pattern, now the last search, goes into the search
/// history; the repeats of the last search and `\&` don't.
pub(crate) fn note_address_pattern(registers: &mut Registers, search: &LineSearch) {
    if let LineSearch::Pattern { pattern, .. } = search {
        registers.note_pattern(pattern);
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
        LineSearch::LastSearch { forward } | LineSearch::Repeat { forward } => {
            (last_search()?, *forward)
        }
        LineSearch::Pattern { pattern, forward } if pattern.is_empty() => {
            (last_search()?, *forward)
        }
        LineSearch::Pattern { pattern, forward } => (pattern.clone(), *forward),
    };
    // As `/` and `?` do: the pattern becomes the last search even if
    // nothing matches or Vim rejects it, but not one fe-vim can't search.
    let keep = !matches!(search, LineSearch::LastSearch { .. });
    // The address's own pattern drops the last search's offset (`//` too).
    let drops_offset = matches!(search, LineSearch::Pattern { .. });
    let word = search_word(target.registers, search);
    let parsed = match Pattern::parse_with(&pattern, target.registers.ignores_case(&pattern, word))
    {
        Ok(parsed) => parsed,
        Err(error) => {
            if keep && matches!(error, PatternError::Invalid(_)) {
                target.registers.set_search(&pattern, forward);
                target.registers.set_search_word(word);
                note_address_pattern(target.registers, search);
                if drops_offset {
                    target.registers.set_search_offset(SearchOffset::None);
                }
                target.state.highlight = None;
            }
            return Err(ExError::Pattern(error.to_string()));
        }
    };
    if keep {
        target.registers.set_search(&pattern, forward);
        target.registers.set_search_word(word);
        note_address_pattern(target.registers, search);
        if drops_offset {
            target.registers.set_search_offset(SearchOffset::None);
        }
    }
    target.state.highlight = Some(parsed.clone());
    let text = target.model.text();
    let rows = text.row_count();
    // `from` counts from 1, so as a row it's the line after it.
    let from = u32::try_from(from).unwrap_or(rows).min(rows);
    let order: Box<dyn Iterator<Item = u32>> = match forward {
        true => Box::new((from..rows).chain(0..from)),
        false => {
            let at = from.max(1) - 1;
            Box::new((0..at).rev().chain((at..rows).rev()))
        }
    };
    // Past the time limit, `Interrupted`, not a line further on.
    let deadline = Deadline::new();
    for row in order {
        match search::first_in_row(text, row, &parsed, &deadline) {
            Ok(Some(_)) => return Ok(i64::from(row) + 1),
            Ok(None) => {}
            Err(interrupted) => return Err(ExError::Pattern(interrupted.to_string())),
        }
    }
    Err(ExError::NotFound(pattern))
}

/// One caret at the start of `row` (or the last, past it), as nvim's `:g`
/// and `:normal` move the cursor: in a visual mode, the vim cursor. It
/// keeps the primary caret's goal column, as nvim keeps its own, unless a
/// change left that to be taken from where the caret is (fe-vim keeps none
/// then) and `settle` is false: then it's taken at the row's start.
fn to_row_start(target: &mut VimTarget<'_>, row: u32, settle: bool) {
    target.model.sync_display();
    let VimTarget { model, state, .. } = target;
    let text = model.text();
    let start = text.row_start(row.min(text.row_count() - 1));
    if let Mode::Visual(kind) = state.mode() {
        let anchor = state.visual_anchor(model).unwrap_or(start);
        state.select_visual(model, kind, anchor, start);
        return;
    }
    let head = model.primary_selection().head();
    let goals = motion::valid_goals(state, model.snapshot().version(), &model.selections());
    let kept = goals.and_then(|(_, goals)| goals.iter().find(|(at, _)| *at == head));
    let goal = match kept {
        Some(&(_, Some(goal))) => Some(goal),
        _ if kept.is_none() && !settle => None,
        _ => {
            let cursor = Cursor::at(model.text(), head);
            Some(cursor.line.goal_column(cursor.ix, model.tab_size().max(1)))
        }
    };
    model.set_selections([operator::caret(start)]);
    state.goals = goal.map(|goal| (model.snapshot().version(), vec![(start, Some(goal))]));
}

/// The first non-blank on `row` of `text`, as an offset.
fn first_non_blank(text: &fe_text::Text, row: u32) -> usize {
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.go_to_first_non_blank();
    cursor.offset()
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
    cursor.go_to_first_non_blank();
    let caret = cursor.offset();
    model.set_selections([operator::caret(caret)]);
    state.goals = None;
    Ok(Dispatched {
        effect: Effect::Changed,
        ..dispatched
    })
}
