//! Shell integration: the working directory a shell reports with OSC 7 and
//! the prompts it marks with OSC 133 (docs/terminal-shell-integration.md).
//!
//! alacritty's parser ignores both, so the pty loop runs a [`Scanner`] over
//! the bytes it feeds the parser and hands what it finds to the session's
//! [`ShellState`], with the `Term` still locked (lock the `Term`, then the
//! shell state).
//!
//! A prompt mark names its row by an absolute number that never changes
//! while the row exists: the rows trimmed off the top of history so far
//! (`dropped`), plus the history's size, plus the row's grid line. alacritty
//! counts neither, and once its history is full it rotates rows out without
//! saying so, so the `Term` keeps [`HEADROOM`] more rows of history than the
//! session's scrollback, and [`ShellState::advanced`] trims it back after
//! every parser call, counting what it trims.

use std::collections::VecDeque;
#[cfg(unix)]
use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Grid, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Term, TermMode};

/// Rows of history the `Term` keeps beyond the scrollback, so that what one
/// parser call scrolls into history doesn't rotate rows out uncounted.
pub(crate) const HEADROOM: usize = 1_024;

/// The most bytes the loop feeds the parser in one call: each scrolls at
/// most one row, so a call never overruns [`HEADROOM`] (but `CSI n S`).
pub(crate) const MAX_ADVANCE: usize = 1_024;

/// A history limit no reflow reaches, set while the grid resizes.
pub(crate) const UNBOUNDED_HISTORY: usize = usize::MAX / 4;

/// The longest OSC 7 or 133 kept: a longer one is dropped.
const MAX_OSC: usize = 4_096;

/// What a recognised sequence reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShellEvent {
    /// OSC 7: the shell's working directory.
    Cwd(PathBuf),
    /// OSC 133;A: a prompt starts at the cursor.
    PromptStart,
    /// OSC 133;B: the command line starts.
    CommandStart,
    /// OSC 133;C: the command's output starts.
    OutputStart,
    /// OSC 133;D, with the command's exit code if the shell gave one.
    #[allow(dead_code, reason = "the code is for the card that marks output")]
    CommandEnd(Option<i32>),
}

/// What [`Scanner::next`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Found {
    /// An OSC 7 or 133, ended just before the offset returned with it.
    Shell(ShellEvent),
    /// A sequence that clears history (`CSI 3 J`, `ESC c`), whose final
    /// byte is at the offset returned with it. The loop feeds the parser
    /// that byte on its own, so the history's fall is all the clear's.
    ClearsHistory,
    /// `CSI ? 1049 h`, which hides the primary grid, with its final byte at
    /// the offset returned with it: the loop notes where the marks are
    /// first ([`ShellState::freeze`]). `sync` if the same CSI starts a
    /// synchronized update (`CSI ? 1049 ; 2026 h`), which starts after it.
    EntersAlternateScreen { sync: bool },
    /// `CSI ? 1049 l`, which shows the primary grid again, with its final
    /// byte at the offset returned with it: the loop feeds that byte on its
    /// own, so the parser call after it starts on the primary screen, its
    /// history in step with a resize made while it was hidden.
    LeavesAlternateScreen,
    /// `CSI ? 2026 h`, the start of a synchronized update, ended just
    /// before the offset returned with it.
    StartsSync,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    /// After an `ESC`.
    Escape,
    /// In a CSI's parameters.
    Csi,
    /// In an OSC's payload.
    Osc,
    /// After an `ESC` in an OSC: `\` ends it.
    OscEscape,
}

/// The most parameters and sub-parameters a CSI has, as alacritty's parser
/// (vte) counts them: it ignores a CSI with more, and so does the scanner.
const MAX_CSI_PARAMS: usize = 32;

/// Finds OSC 7 and OSC 133 sequences, ended by BEL or ST, and the CSIs
/// [`Found`] names, in a byte stream fed in slices that may split a sequence
/// anywhere.
#[derive(Debug, Default)]
pub(crate) struct Scanner {
    state: State,
    /// The OSC's payload.
    payload: Vec<u8>,
    /// The CSI's parameters' first values, as the parser reads them: digits
    /// accumulate, saturating, and sub-parameters (after `:`) are left out.
    params: Vec<u16>,
    /// The CSI's parameters and sub-parameters ended so far, which the
    /// parser counts up to [`MAX_CSI_PARAMS`].
    slots: usize,
    /// The CSI's parameter or sub-parameter being read.
    param: u16,
    /// Whether `param` is a sub-parameter.
    sub: bool,
    /// Whether the CSI has had a parameter byte.
    has_params: bool,
    /// Whether the CSI started with the private marker `?`.
    private: bool,
    /// Whether the OSC outgrew its limit, or the CSI has more parameters
    /// than the parser keeps, intermediates or another marker.
    overflow: bool,
}

impl Scanner {
    /// Scans `bytes` up to the next sequence found: what it is and its offset
    /// ([`Found`]), or `None` when there's none in `bytes` (all of which are
    /// scanned). Call again with the bytes after the offset.
    pub(crate) fn next(&mut self, bytes: &[u8]) -> Option<(usize, Found)> {
        for (ix, &byte) in bytes.iter().enumerate() {
            if let Some(found) = self.byte(byte) {
                let end = match found {
                    Found::Shell(_) | Found::StartsSync => ix + 1,
                    Found::ClearsHistory
                    | Found::EntersAlternateScreen { .. }
                    | Found::LeavesAlternateScreen => ix,
                };
                return Some((end, found));
            }
        }
        None
    }

    fn start(&mut self, state: State) {
        self.state = state;
        self.payload.clear();
        self.params.clear();
        self.slots = 0;
        self.param = 0;
        self.sub = false;
        self.has_params = false;
        self.private = false;
        self.overflow = false;
    }

    /// Ends the CSI parameter or sub-parameter being read, as the parser
    /// does at `;`, `:` and the final byte; past the parser's limit, the CSI
    /// is ignored.
    fn end_param(&mut self) {
        if self.slots == MAX_CSI_PARAMS {
            self.overflow = true;
            return;
        }
        if !self.sub {
            self.params.push(self.param);
        }
        self.slots += 1;
        self.param = 0;
    }

    fn byte(&mut self, byte: u8) -> Option<Found> {
        match (self.state, byte) {
            // CAN and SUB cancel any sequence; `ESC` starts another.
            (_, 0x18 | 0x1a) => self.state = State::Ground,
            (State::Osc, 0x1b) => self.state = State::OscEscape,
            (State::OscEscape, b'\\') => {
                self.state = State::Ground;
                return self.osc_end();
            }
            (_, 0x1b) => self.state = State::Escape,
            (State::Ground, _) => {}
            (State::Escape, b']') => self.start(State::Osc),
            (State::Escape, b'[') => self.start(State::Csi),
            (State::Escape, b'c') => {
                self.state = State::Ground;
                return Some(Found::ClearsHistory);
            }
            (State::Escape, _) => self.state = State::Ground,
            // The parser runs C0 controls in a CSI, and goes on with it.
            (State::Csi, 0x00..=0x1f) => {}
            (State::Csi, b'0'..=b'9') => {
                self.has_params = true;
                if self.slots == MAX_CSI_PARAMS {
                    self.overflow = true;
                } else {
                    let digit = u16::from(byte - b'0');
                    self.param = self.param.saturating_mul(10).saturating_add(digit);
                }
            }
            (State::Csi, b';' | b':') => {
                self.has_params = true;
                self.end_param();
                self.sub = byte == b':';
            }
            (State::Csi, b'?') if !self.has_params && !self.private => self.private = true,
            // Intermediates and other markers.
            (State::Csi, 0x20..=0x2f | 0x3c..=0x3f) => self.overflow = true,
            (State::Csi, _) => {
                self.state = State::Ground;
                self.end_param();
                return self.csi_end(byte);
            }
            (State::Osc, 0x07) => {
                self.state = State::Ground;
                return self.osc_end();
            }
            // The parser ignores other C0 controls in an OSC.
            (State::Osc, 0x00..=0x1f) => {}
            (State::Osc, _) => {
                if self.payload.len() < MAX_OSC {
                    self.payload.push(byte);
                } else {
                    self.overflow = true;
                }
            }
            // Not ST: the OSC is over, and this byte follows an `ESC`.
            (State::OscEscape, _) => {
                self.state = State::Escape;
                return self.byte(byte);
            }
        }
        None
    }

    /// What the CSI ending in `last` is, if it's one [`Found`] names.
    fn csi_end(&self, last: u8) -> Option<Found> {
        if self.overflow {
            return None;
        }
        let params = &self.params;
        match (self.private, last) {
            (false, b'J') => {
                (params.first().copied().unwrap_or(0) == 3).then_some(Found::ClearsHistory)
            }
            (true, b'h') => {
                let sync = params.contains(&2026);
                if params.contains(&1049) {
                    Some(Found::EntersAlternateScreen { sync })
                } else if sync {
                    Some(Found::StartsSync)
                } else {
                    None
                }
            }
            (true, b'l') => params
                .contains(&1049)
                .then_some(Found::LeavesAlternateScreen),
            _ => None,
        }
    }

    fn osc_end(&self) -> Option<Found> {
        if self.overflow {
            return None;
        }
        parse(&self.payload).map(Found::Shell)
    }
}

/// What an OSC payload (between `ESC ]` and the terminator) reports, if it
/// is a well-formed OSC 7 or 133.
fn parse(payload: &[u8]) -> Option<ShellEvent> {
    let mut fields = payload.splitn(2, |&byte| byte == b';');
    let (command, rest) = (fields.next()?, fields.next()?);
    match command {
        b"7" => cwd(rest).map(ShellEvent::Cwd),
        b"133" => {
            let mut fields = rest.split(|&byte| byte == b';');
            match fields.next()? {
                b"A" => Some(ShellEvent::PromptStart),
                b"B" => Some(ShellEvent::CommandStart),
                b"C" => Some(ShellEvent::OutputStart),
                b"D" => {
                    let code = fields
                        .next()
                        .and_then(|code| std::str::from_utf8(code).ok()?.parse().ok());
                    Some(ShellEvent::CommandEnd(code))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The path of a `file://<host>/<percent-encoded path>` URL. The host isn't
/// checked: the panel uses the path only if it's a directory here.
fn cwd(url: &[u8]) -> Option<PathBuf> {
    const SCHEME: &[u8] = b"file://";
    if url.len() < SCHEME.len() || !url[..SCHEME.len()].eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let rest = &url[SCHEME.len()..];
    let path = &rest[rest.iter().position(|&byte| byte == b'/')?..];
    let path = percent_decode(path)?;
    if path.contains(&0) {
        return None;
    }
    path_from_url_bytes(path)
}

#[cfg(unix)]
fn path_from_url_bytes(path: Vec<u8>) -> Option<PathBuf> {
    Some(PathBuf::from(OsStr::from_bytes(&path)))
}

/// A URL's path on Windows: UTF-8, its `/C:/…` a drive path, `C:/…`.
#[cfg(windows)]
fn path_from_url_bytes(path: Vec<u8>) -> Option<PathBuf> {
    let path = String::from_utf8(path).ok()?;
    let bytes = path.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
    Some(PathBuf::from(if drive { &path[1..] } else { &path[..] }))
}

/// `bytes` with each `%XX` decoded, or `None` if a `%` isn't followed by two
/// hex digits.
fn percent_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    fn hex(byte: u8) -> Option<u8> {
        char::from(byte).to_digit(16).map(|digit| digit as u8)
    }
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut bytes = bytes.iter();
    while let Some(&byte) = bytes.next() {
        if byte == b'%' {
            let high = hex(*bytes.next()?)?;
            let low = hex(*bytes.next()?)?;
            decoded.push(high << 4 | low);
        } else {
            decoded.push(byte);
        }
    }
    Some(decoded)
}

/// What a session knows of its shell, and the prompt marks' bookkeeping.
#[derive(Debug)]
pub(crate) struct ShellState {
    /// The rows of history the session keeps; the `Term` keeps
    /// [`HEADROOM`] more until trimmed.
    scrollback: usize,
    /// The directory OSC 7 last reported.
    cwd: Option<PathBuf>,
    /// The prompt marks' absolute rows, ascending.
    marks: VecDeque<u64>,
    /// The mark the view last scrolled to, until it returns to the bottom.
    remembered: Option<u64>,
    /// Rows trimmed off the top of history, or lost to clearing it.
    dropped: u64,
    /// The history's size after the last parser call.
    history: usize,
    /// Whether the next look at the history takes its size as it is, after
    /// a resize on the alternate screen, which hides the primary's.
    resync: bool,
    /// The marks' logical lines as the alternate screen hid the primary
    /// grid, for placing them again if it resizes meanwhile.
    frozen: Option<Anchors>,
    /// Whether the grid resized since the alternate screen hid the primary.
    resized_hidden: bool,
    /// The rows scrolled into history so far: `dropped` plus the history's
    /// size, the absolute row of grid line 0, as of the last parser call.
    pub(crate) scrolled: u64,
}

/// The marks' places by logical line, counted from the top of history:
/// what a reflow keeps.
#[derive(Debug)]
pub(crate) struct Anchors {
    columns: usize,
    /// On the alternate screen the primary grid is out of reach: no marks
    /// are anchored.
    alt: bool,
    /// How far back the view was scrolled.
    offset: usize,
    /// Each mark's logical line.
    marks: Vec<Option<usize>>,
    remembered: Option<usize>,
}

impl ShellState {
    pub(crate) fn new(scrollback: usize) -> ShellState {
        ShellState {
            scrollback,
            cwd: None,
            marks: VecDeque::new(),
            remembered: None,
            dropped: 0,
            history: 0,
            resync: false,
            frozen: None,
            resized_hidden: false,
            scrolled: 0,
        }
    }

    /// The history the `Term` is built to keep.
    pub(crate) fn term_history(scrollback: usize) -> usize {
        scrollback.saturating_add(HEADROOM)
    }

    pub(crate) fn cwd(&self) -> Option<PathBuf> {
        self.cwd.clone()
    }

    /// The absolute row of grid line 0.
    fn base<T>(&self, term: &Term<T>) -> u64 {
        self.dropped + term.grid().history_size() as u64
    }

    /// Takes in `event`, which the parser has just been fed to its end.
    pub(crate) fn event<T>(&mut self, event: ShellEvent, term: &mut Term<T>) {
        match event {
            ShellEvent::Cwd(path) => self.cwd = Some(path),
            ShellEvent::PromptStart => self.mark(term),
            ShellEvent::CommandStart | ShellEvent::OutputStart | ShellEvent::CommandEnd(_) => {}
        }
    }

    /// Marks a prompt at the cursor's row, unless on the alternate screen.
    /// Marks below it are of rows the shell has since written over.
    fn mark<T>(&mut self, term: &Term<T>) {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let row = self.base(term) + term.grid().cursor.point.line.0.max(0) as u64;
        while self.marks.back().is_some_and(|&mark| mark >= row) {
            self.marks.pop_back();
        }
        self.marks.push_back(row);
    }

    /// Accounts for a parser call: places the marks again if the grid
    /// resized while the alternate screen hid them, trims history back to
    /// the scrollback, and drops the marks of rows that went.
    pub(crate) fn advanced<T>(&mut self, term: &mut Term<T>) {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let frozen = self.frozen.take();
        if std::mem::take(&mut self.resized_hidden) {
            match frozen {
                Some(frozen) => {
                    let offset = frozen.offset;
                    self.place(term, frozen);
                    self.view_moved(offset, term);
                }
                // Where the hidden view went is unknown.
                None => self.forget(),
            }
        }
        let history = term.grid().history_size();
        if std::mem::take(&mut self.resync) {
            self.history = history;
        } else if history < self.history {
            // History was cleared (CSI 3 J, a reset), which puts the view
            // at the bottom: every row above the screen went, then rows may
            // have scrolled in again.
            self.dropped += self.history as u64;
            self.remembered = None;
        }
        self.trim(term);
    }

    /// Notes the marks' logical lines as the alternate screen is about to
    /// hide the primary grid.
    pub(crate) fn freeze<T>(&mut self, term: &Term<T>) {
        if !term.mode().contains(TermMode::ALT_SCREEN) {
            self.frozen = Some(self.anchors(term));
        }
    }

    /// Trims the primary grid's history to the scrollback, forgets the
    /// marks above it, and limits the history to [`HEADROOM`] more again.
    fn trim<T>(&mut self, term: &mut Term<T>) {
        let grid = term.grid_mut();
        let history = grid.history_size();
        if history > self.scrollback {
            grid.update_history(self.scrollback);
            self.dropped += (history - self.scrollback) as u64;
        }
        grid.update_history(Self::term_history(self.scrollback));
        self.history = grid.history_size();
        self.scrolled = self.dropped + self.history as u64;
        let dropped = self.dropped;
        while self.marks.front().is_some_and(|&mark| mark < dropped) {
            self.marks.pop_front();
        }
    }

    /// The marks' grid lines (negative in history), oldest first: none on
    /// the alternate screen.
    pub(crate) fn prompt_lines<T>(&self, term: &Term<T>) -> Vec<i32> {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return Vec::new();
        }
        let base = self.base(term) as i64;
        let (top, bottom) = (term.topmost_line().0, term.bottommost_line().0);
        (self.marks.iter())
            .map(|&mark| (mark as i64 - base) as i32)
            .filter(|line| (top..=bottom).contains(line))
            .collect()
    }

    /// Scrolls to the previous or next prompt (`forward`), VS Code's way:
    /// from the mark last scrolled to, or back from the shell's cursor; on
    /// past the last, to the bottom (forward from none does nothing). The
    /// mark's row goes to the top of the view, as far as history allows.
    /// Whether there was a mark to go to.
    pub(crate) fn scroll_to_prompt<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        forward: bool,
    ) -> bool {
        let lines = self.prompt_lines(term);
        let base = self.base(term) as i64;
        let from = self.remembered.map(|mark| mark as i64 - base);
        let target = match (forward, from) {
            (true, Some(from)) => lines.iter().find(|&&line| i64::from(line) > from),
            // Not scrolled to a prompt: the view is where the user left it.
            (true, None) => return false,
            (false, Some(from)) => lines.iter().rev().find(|&&line| i64::from(line) < from),
            (false, None) => {
                let cursor = term.grid().cursor.point.line.0;
                lines.iter().rev().find(|&&line| line < cursor)
            }
        };
        let Some(&line) = target else {
            if forward {
                // `forget`, not just the live mark: the alternate screen
                // froze a copy, and a hidden resize would restore it.
                self.forget();
                term.scroll_display(Scroll::Bottom);
            }
            return false;
        };
        self.remembered = Some((base + i64::from(line)) as u64);
        let grid = term.grid();
        let offset = (-line).clamp(0, grid.history_size() as i32);
        let delta = offset - grid.display_offset() as i32;
        term.scroll_display(Scroll::Delta(delta));
        true
    }

    /// Forgets the mark last scrolled to: the view is back at the bottom.
    pub(crate) fn forget(&mut self) {
        self.remembered = None;
        if let Some(frozen) = &mut self.frozen {
            frozen.remembered = None;
        }
    }

    /// Forgets the mark last scrolled to if the view, `from` rows back
    /// before something other than a prompt jump moved it, is now at the
    /// bottom.
    pub(crate) fn view_moved<T>(&mut self, from: usize, term: &Term<T>) {
        if from > 0 && term.grid().display_offset() == 0 {
            self.forget();
        }
    }

    /// Where the marks are by logical line.
    fn anchors<T>(&self, term: &Term<T>) -> Anchors {
        let alt = term.mode().contains(TermMode::ALT_SCREEN);
        let mut anchors = Anchors {
            columns: term.columns(),
            alt,
            offset: term.grid().display_offset(),
            marks: Vec::new(),
            remembered: None,
        };
        if alt || self.marks.is_empty() {
            return anchors;
        }
        let starts = logical_starts(term.grid());
        let base = self.base(term) as i64;
        let logical = |mark: u64| {
            let line = mark as i64 - base;
            starts
                .partition_point(|&start| i64::from(start) <= line)
                .checked_sub(1)
        };
        anchors.marks = self.marks.iter().map(|&mark| logical(mark)).collect();
        anchors.remembered = self.remembered.and_then(logical);
        anchors
    }

    /// Where the marks are by logical line, before `term` resizes. The
    /// caller lifts the history's limit for the resize, so a reflow loses no
    /// rows ([`Terminal::resize`](crate::Terminal::resize)): the trim after
    /// counts what goes.
    pub(crate) fn before_resize<T>(&self, term: &Term<T>) -> Anchors {
        self.anchors(term)
    }

    /// Moves the marks to the first rows of their logical lines after
    /// `term` resized, which reflows lines when the column count changes.
    pub(crate) fn after_resize<T>(&mut self, term: &mut Term<T>, anchors: Anchors) {
        if anchors.alt {
            // The primary grid resized out of sight: place the marks from
            // where they were as it was hidden, once it shows again (the
            // next `advanced`, which trims it).
            if self.frozen.is_none() && term.columns() != anchors.columns {
                self.marks.clear();
            }
            self.resized_hidden = true;
            self.resync = true;
            return;
        }
        self.place(term, anchors);
        self.trim(term);
    }

    /// Puts the marks on the first rows of the logical lines `anchors`
    /// names.
    fn place<T>(&mut self, term: &Term<T>, anchors: Anchors) {
        if self.marks.is_empty() {
            return;
        }
        let starts = logical_starts(term.grid());
        let base = self.base(term) as i64;
        let row = |logical: Option<usize>| {
            let start = starts.get(logical?)?;
            Some((base + i64::from(*start)) as u64)
        };
        let mut marks: Vec<u64> = anchors.marks.into_iter().filter_map(row).collect();
        marks.dedup();
        self.marks = marks.into();
        self.remembered = row(anchors.remembered);
    }
}

/// The grid line of each logical line's first row, top to bottom: a
/// logical line is a row and the rows its `WRAPLINE` runs on to.
fn logical_starts(grid: &Grid<Cell>) -> Vec<i32> {
    let last = Column(grid.columns() - 1);
    let mut starts = Vec::new();
    let mut continues = false;
    for line in grid.topmost_line().0..=grid.bottommost_line().0 {
        if !continues {
            starts.push(line);
        }
        continues = grid[Line(line)][last].flags.contains(Flags::WRAPLINE);
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything found in `bytes`, scanned in slices of `split` bytes.
    fn found(bytes: &[u8], split: usize) -> Vec<Found> {
        let mut scanner = Scanner::default();
        let mut found = Vec::new();
        for mut chunk in bytes.chunks(split) {
            while let Some((end, next)) = scanner.next(chunk) {
                // As the loop does, past a final byte fed alone.
                let alone = matches!(
                    next,
                    Found::ClearsHistory
                        | Found::EntersAlternateScreen { .. }
                        | Found::LeavesAlternateScreen
                );
                let end = if alone { end + 1 } else { end };
                found.push(next);
                chunk = &chunk[end..];
            }
        }
        found
    }

    /// The OSC 7 and 133 found in `bytes`.
    fn scan(bytes: &[u8], split: usize) -> Vec<ShellEvent> {
        (found(bytes, split).into_iter())
            .filter_map(|found| match found {
                Found::Shell(event) => Some(event),
                _ => None,
            })
            .collect()
    }

    fn cwd_event(path: &str) -> ShellEvent {
        ShellEvent::Cwd(PathBuf::from(path))
    }

    #[test]
    fn finds_sequences_split_anywhere() {
        let bytes = b"out\x1b]7;file://host/a%20b\x07\x1b[1mx\x1b]133;A\x1b\\$ \x1b]133;D;2\x07";
        let expected = vec![
            cwd_event("/a b"),
            ShellEvent::PromptStart,
            ShellEvent::CommandEnd(Some(2)),
        ];
        for split in 1..=bytes.len() {
            assert_eq!(scan(bytes, split), expected, "in slices of {split}");
        }
    }

    #[test]
    fn reports_the_terminators_end() {
        let mut scanner = Scanner::default();
        let bytes = b"ab\x1b]133;B\x1b\\cd";
        let found = Found::Shell(ShellEvent::CommandStart);
        assert_eq!(scanner.next(bytes), Some((11, found)));
        assert_eq!(scanner.next(&bytes[11..]), None);
    }

    #[test]
    fn finds_history_clears_at_their_final_byte() {
        let mut scanner = Scanner::default();
        let bytes = b"a\x1b[2J\x1b[3Jb";
        assert_eq!(scanner.next(bytes), Some((8, Found::ClearsHistory)));
        assert_eq!(scanner.next(&bytes[9..]), None);
        let bytes = b"\x1b[3J\x1bc\x1b[03;1J\x1b[?3J\x1b[3 J\x1b[13J\x1b[3\x18J\x1b[3\nJ";
        let clears = vec![Found::ClearsHistory; 4];
        for split in 1..=bytes.len() {
            assert_eq!(found(bytes, split), clears, "in slices of {split}");
        }
    }

    #[test]
    fn finds_screen_switches_and_synchronized_updates() {
        let bytes = b"\x1b[?1049h\x1b[?1;1049h\x1b[?2026h\x1b[?2004;2026h\
            \x1b[1049h\x1b[?1049l\x1b[?2026l\x1b[>2026h\x1b[?1047h\
            \x1b[?1049;2026h\x1b[?2026;1049h\x1b[?47l\x1b[?1;1049l";
        let alt = |sync| Found::EntersAlternateScreen { sync };
        let expected = vec![
            alt(false),
            alt(false),
            Found::StartsSync,
            Found::StartsSync,
            Found::LeavesAlternateScreen,
            alt(true),
            alt(true),
            Found::LeavesAlternateScreen,
        ];
        for split in 1..=bytes.len() {
            assert_eq!(found(bytes, split), expected, "in slices of {split}");
        }
        let mut scanner = Scanner::default();
        assert_eq!(
            scanner.next(b"ab\x1b[?2026hc"),
            Some((10, Found::StartsSync))
        );
        assert_eq!(
            scanner.next(b"\x1b[?1049h"),
            Some((7, Found::EntersAlternateScreen { sync: false }))
        );
        assert_eq!(
            scanner.next(b"x\x1b[?1049ly"),
            Some((8, Found::LeavesAlternateScreen))
        );
    }

    #[test]
    fn counts_csi_parameters_as_the_parser_does() {
        // More than 32 bytes of parameters, but 9 of them: the parser reads
        // it, and so does the scanner.
        let long = b"\x1b[?1;1000;1002;1003;1004;1005;1006;2004;1049l";
        for split in 1..=long.len() {
            assert_eq!(
                found(long, split),
                [Found::LeavesAlternateScreen],
                "in slices of {split}"
            );
        }
        // Leading zeros and sub-parameters, as the parser takes them.
        let mut scanner = Scanner::default();
        let bytes = b"\x1b[?000001049:5:6;2026h";
        assert_eq!(
            scanner.next(bytes),
            Some((bytes.len() - 1, Found::EntersAlternateScreen { sync: true }))
        );
        // 33 parameters and sub-parameters: the parser ignores the CSI.
        let mut many = b"\x1b[?".to_vec();
        for _ in 0..16 {
            many.extend_from_slice(b"1:2;");
        }
        many.extend_from_slice(b"1049l");
        assert_eq!(Scanner::default().next(&many), None);
        let mut most = b"\x1b[?".to_vec();
        for _ in 0..31 {
            most.extend_from_slice(b"1;");
        }
        most.extend_from_slice(b"1049l");
        assert_eq!(
            Scanner::default().next(&most),
            Some((most.len() - 1, Found::LeavesAlternateScreen))
        );
    }

    #[test]
    fn ignores_other_and_malformed_sequences() {
        let bytes = b"\x1b]0;title\x07\x1b]8;;http://x\x07\x1b]133;Z\x07\x1b]1337;A\x07\
            \x1b]7;/plain\x07\x1b]7;file:relative\x07\x1b]7;file://host\x07\
            \x1b]7;file://h/bad%zz\x07\x1b]7;file://h/cut%2\x07\x1b]7;file://h/nul%00\x07\
            \x1b]7;file://h/can\x18\x1b]7;file://h/esc\x1bx\x07";
        assert_eq!(scan(bytes, bytes.len()), Vec::new());
    }

    #[test]
    fn an_escape_in_a_sequence_can_start_the_next() {
        let bytes = b"\x1b]7;file://h/dropped\x1b\x1b]133;C\x07";
        assert_eq!(scan(bytes, 1), vec![ShellEvent::OutputStart]);
        let bytes = b"\x1b]7;file://h/dropped\x1b]133;C\x07";
        assert_eq!(scan(bytes, 1), vec![ShellEvent::OutputStart]);
    }

    #[test]
    fn drops_overlong_sequences() {
        let mut long = b"\x1b]7;file://h/".to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX_OSC));
        long.extend(b"\x07\x1b]7;file://h/ok\x07");
        assert_eq!(scan(&long, 100), vec![cwd_event("/ok")]);
    }

    #[cfg(unix)]
    #[test]
    fn decodes_paths_that_are_not_utf8() {
        let events = scan(b"\x1b]7;FILE://h/x/%FF%2f\x07", 3);
        let path = PathBuf::from(OsStr::from_bytes(b"/x/\xff/"));
        assert_eq!(events, vec![ShellEvent::Cwd(path)]);
    }

    #[cfg(windows)]
    #[test]
    fn decodes_drive_paths() {
        let events = scan(b"\x1b]7;file://h/C:/Users/x%20y\x07", 3);
        assert_eq!(events, vec![ShellEvent::Cwd(PathBuf::from("C:/Users/x y"))]);
    }

    #[test]
    fn exit_codes_are_optional() {
        let events = scan(b"\x1b]133;D\x07\x1b]133;D;x\x07\x1b]133;A;cl=m\x07", 4);
        let expected = vec![
            ShellEvent::CommandEnd(None),
            ShellEvent::CommandEnd(None),
            ShellEvent::PromptStart,
        ];
        assert_eq!(events, expected);
    }
}
