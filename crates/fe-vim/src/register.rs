//! Vim's registers, by `:help registers`. One [`Registers`] is shared by
//! every editor in a host, and non-vim commands may reach it too.

use std::{borrow::Cow, collections::VecDeque};

use fe_editor::commands::EditorHost;

use crate::{CommandLineKind, SearchOffset, substitute::LastSubstitute};

/// How many lines each command-line history keeps: nvim's 'history'
/// default.
pub const HISTORY_MAX: usize = 10000;

/// A register's text, and whether it holds whole lines (its text then ends
/// in `\n`) or a block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
    /// A block's width in display columns (nvim's `getregtype()` is
    /// `<C-v>{width}`): its text is its rows joined by `\n`.
    pub block_width: Option<usize>,
}

impl Register {
    pub fn new(text: impl Into<String>, linewise: bool) -> Self {
        Self {
            text: text.into(),
            linewise,
            block_width: None,
        }
    }

    /// A block `width` columns wide, of `rows`.
    pub fn block(rows: &[String], width: usize) -> Self {
        Self {
            text: rows.join("\n"),
            linewise: false,
            block_width: Some(width),
        }
    }
}

/// What writes a register.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Write {
    Yank,
    /// A delete or change. `small` if it was charwise within one row: it
    /// writes `-`, unless a register was named. `numbered` if it shifts `1`
    /// to `2` … `9` and writes `1`, named register or not: a linewise or
    /// multi-row delete, or one over `%` (even within a row).
    Delete {
        small: bool,
        numbered: bool,
    },
}

/// A name that isn't a register.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} isn't a register")]
pub struct RegisterError(pub char);

/// `" 0 1-9 - a-z`, plus `_` (which discards) and `+ *` (one slot: the host's
/// clipboard). `A-Z` append to `a-z`. The read-only `/` holds the last
/// search pattern, which with its direction is the host's last search, as
/// every editor shares it; the read-only `.` holds the last inserted text
/// and `:` the last command line run.
///
/// It keeps the command-line histories too (`:help cmdline-history`), which
/// are as global as the last search: one for search lines, which `/` and
/// `?` share, and one for ex lines ([`Registers::add_history`],
/// [`Registers::recall`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    numbered: [Option<Register>; 10],
    small: Option<Register>,
    named: [Option<Register>; 26],
    /// The register `"` reads: the last one written, `+` for the clipboard.
    unnamed: Option<char>,
    /// `/`: the last search pattern, as characters.
    search: Option<Register>,
    /// `.`: the text the last insert or replace session typed, charwise.
    inserted: Option<Register>,
    /// The order of the session `inserted` came from, so a session closed
    /// late (by a clone of an editor's state) can't replace a newer one.
    inserted_order: u64,
    /// `:`: the last command line run, charwise.
    command_line: Option<Register>,
    /// Whether the last search went forward (`/`, not `?`).
    search_forward: bool,
    /// The last search's offset, which `n` and `N` keep.
    search_offset: SearchOffset,
    /// Whether the last search's pattern is `*`'s or `#`'s, which
    /// 'smartcase' doesn't apply to, nor to `n`, `N` and `/<CR>` repeating
    /// it, as in nvim.
    search_word: bool,
    /// 'ignorecase' and 'smartcase', global as nvim's are.
    ignore_case: bool,
    smart_case: bool,
    /// 'incsearch' is off: on by default, as in nvim, and global like the
    /// case options. Each editor's [`crate::VimState`] takes it from here
    /// before a preview ([`crate::VimState::set_incsearch`]).
    incsearch_off: bool,
    /// The last `:s`, which `:&` repeats; shared like the last search.
    pub(crate) substitute: LastSubstitute,
    /// `clipboard=unnamedplus`: registers named by no command are `+`.
    clipboard_unnamed: bool,
    /// What was last written to the clipboard, with its kind, so a read of
    /// the same text keeps it.
    clipboard_last: Option<Register>,
    /// The search lines submitted, oldest first, each with the delimiter
    /// of the line it was typed on (`/` or `?`, [`NO_DELIMITER`] for a
    /// pattern set by `:s`, `:g`, an ex address or `*`).
    search_history: VecDeque<(String, char)>,
    /// The ex lines submitted, oldest first, each with `:`.
    ex_history: VecDeque<(String, char)>,
    /// The last pattern a running ex line set, for the search history.
    pending_pattern: Option<String>,
    /// How many ex lines are running (`:normal` nests them).
    ex_depth: u32,
}

/// The clipboard's name in [`Registers::unnamed`].
const CLIPBOARD: char = '+';

impl Registers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `clipboard=unnamedplus`: a yank, delete or change that names no
    /// register also writes the clipboard, and a put that names none reads
    /// it (`:help clipboard-unnamedplus`).
    pub fn set_clipboard_unnamed(&mut self, on: bool) {
        self.clipboard_unnamed = on;
    }

    /// Whether `clipboard=unnamedplus` is on ([`Registers::set_clipboard_unnamed`]).
    pub fn clipboard_unnamed(&self) -> bool {
        self.clipboard_unnamed
    }

    /// Sets 'ignorecase' and 'smartcase' (`:help 'ignorecase'`): a search
    /// with `/ ? n N` ignores case when `ignore_case` is on, unless
    /// `smart_case` is on too and its pattern has an uppercase letter
    /// ([`crate::search::has_uppercase`]); `* #` (and `n N` after them) take
    /// `ignore_case` alone. `\c` and `\C` in a pattern override both.
    pub fn set_search_case(&mut self, ignore_case: bool, smart_case: bool) {
        self.ignore_case = ignore_case;
        self.smart_case = smart_case;
    }

    /// 'ignorecase' and 'smartcase' ([`Registers::set_search_case`]).
    pub fn search_case(&self) -> (bool, bool) {
        (self.ignore_case, self.smart_case)
    }

    /// Turns 'incsearch' on or off for every editor sharing these registers.
    pub fn set_incsearch(&mut self, on: bool) {
        self.incsearch_off = !on;
    }

    /// Whether 'incsearch' is on ([`Registers::set_incsearch`]).
    pub fn incsearch(&self) -> bool {
        !self.incsearch_off
    }

    /// Whether a search for `pattern` ignores case, by 'ignorecase' and,
    /// unless it's a `word`'s (`*`, `#`), 'smartcase'. `\c` and `\C` in
    /// the pattern still override it.
    pub fn ignores_case(&self, pattern: &str, word: bool) -> bool {
        self.ignore_case && (word || !self.smart_case || !crate::search::has_uppercase(pattern))
    }

    /// Whether `name` is a register a command may write: `" 0-9 - a-z A-Z _
    /// + *`. Not the read-only `/ . :`, which only [`Registers::read`] and
    /// [`Registers::get`] reach: a yank, delete or change naming one fails.
    pub fn check(name: char) -> Result<(), RegisterError> {
        match name {
            '"' | '-' | '_' | '+' | '*' => Ok(()),
            _ if name.is_ascii_alphanumeric() => Ok(()),
            _ => Err(RegisterError(name)),
        }
    }

    /// A register's content, `"` following the last write and `A-Z` reading
    /// `a-z`, `/` the last search pattern, `.` the last inserted text and `:`
    /// the last command line, the last two charwise. `None` if it's empty, or for `_ +
    /// *` (and `"` pointing at the clipboard), which only
    /// [`Registers::read`] reaches.
    pub fn get(&self, name: char) -> Option<&Register> {
        match name {
            '/' => self.search.as_ref(),
            '.' => self.inserted.as_ref(),
            ':' => self.command_line.as_ref(),
            '"' => self.get(self.unnamed.filter(|&name| name != CLIPBOARD)?),
            '-' => self.small.as_ref(),
            '0'..='9' => self.numbered[name as usize - '0' as usize].as_ref(),
            'a'..='z' | 'A'..='Z' => {
                let ix = name.to_ascii_lowercase() as usize - 'a' as usize;
                self.named[ix].as_ref()
            }
            _ => None,
        }
    }

    /// What a put of register `name` (`"` if `None`) inserts. The clipboard
    /// holds lines when its text ends in a line break, and never a block; its
    /// `\r\n` and `\r` read as `\n`. A block written to it is its rows
    /// joined by `\n`. `/` is the last search pattern, an error until a
    /// search sets it (nvim's E35). `.` and `:` are the last inserted text
    /// and command line, charwise, empty until there is one; they can be read
    /// but not written ([`Registers::check`]).
    pub fn read(
        &self,
        name: Option<char>,
        host: &mut dyn EditorHost,
    ) -> Result<Option<Register>, RegisterError> {
        let name = name.unwrap_or(if self.clipboard_unnamed { '+' } else { '"' });
        if name == '/' {
            return self.search.clone().map(Some).ok_or(RegisterError(name));
        }
        if matches!(name, '.' | ':') {
            return Ok(self.get(name).cloned());
        }
        Self::check(name)?;
        let clipboard = match name {
            '+' | '*' => true,
            '"' => self.unnamed == Some(CLIPBOARD),
            _ => false,
        };
        if clipboard {
            let raw = host.read_clipboard().unwrap_or_default();
            let text = raw.replace("\r\n", "\n").replace('\r', "\n");
            // Text fe-vim wrote keeps its kind (compared before newline
            // normalization, as the clipboard holds it verbatim); another
            // app's is linewise when it ends in a line break.
            if let Some(last) = self.clipboard_last.as_ref().filter(|last| last.text == raw) {
                return Ok(Some(Register {
                    text,
                    ..last.clone()
                }));
            }
            let linewise = text.ends_with('\n');
            return Ok(Some(Register::new(text, linewise)));
        }
        Ok(self.get(name).cloned())
    }

    /// Stores `value` as `write` does, into `name` if one was given (`"`
    /// names `0`, as `:help quote_quote` says). `_` writes nothing and
    /// leaves `"` alone.
    pub fn write(
        &mut self,
        name: Option<char>,
        write: Write,
        value: Register,
        host: &mut dyn EditorHost,
    ) -> Result<(), RegisterError> {
        if let Some(name) = name {
            Self::check(name)?;
        }
        let name = name.map(|name| if name == '"' { '0' } else { name });
        let unnamed = match name {
            Some('_') => return Ok(()),
            Some('+' | '*') => {
                self.write_clipboard(&value, host);
                Some(CLIPBOARD)
            }
            Some(name) => Some(self.store(name, value.clone())),
            None => None,
        };
        if name.is_none() && self.clipboard_unnamed {
            self.write_clipboard(&value, host);
        }
        self.unnamed = match write {
            Write::Yank => {
                if name.is_none() {
                    self.numbered[0] = Some(value);
                }
                unnamed.or(Some('0'))
            }
            Write::Delete { small, numbered } => {
                if small && name.is_none() {
                    self.small = Some(value.clone());
                }
                if numbered {
                    self.numbered[1..].rotate_right(1);
                    self.numbered[1] = Some(value);
                }
                // A named digit or `-` was shifted along: `"` reads `1`.
                match unnamed {
                    Some(name) if !numbered || !matches!(name, '0'..='9' | '-') => Some(name),
                    _ if numbered => Some('1'),
                    _ => Some('-'),
                }
            }
        };
        Ok(())
    }

    /// The last search: its pattern (the `/` register) and whether it went
    /// forward.
    pub fn last_search(&self) -> Option<(&str, bool)> {
        let search = self.search.as_ref()?;
        Some((search.text.as_str(), self.search_forward))
    }

    /// The last search's offset (`/foo/e`), which `n` and `N` reuse.
    pub fn last_search_offset(&self) -> SearchOffset {
        self.search_offset
    }

    /// Makes `pattern` the last search, as `/` (`forward`) or `?` does. Its
    /// offset stays, as `:s` and `:g` leave it. 'smartcase' applies to it
    /// until [`Registers::set_search_word`] says otherwise; setting the
    /// pattern already there (`:s//x/`, an empty `:g` or address pattern
    /// reusing the last search) keeps whether it was `*`'s or `#`'s.
    pub(crate) fn set_search(&mut self, pattern: &str, forward: bool) {
        let same = self
            .search
            .as_ref()
            .is_some_and(|last| last.text == pattern);
        self.search = Some(Register::new(pattern, false));
        self.search_forward = forward;
        if !same {
            self.search_word = false;
        }
    }

    /// Whether the last search's pattern is `*`'s or `#`'s, which
    /// 'smartcase' doesn't apply to.
    pub(crate) fn last_search_word(&self) -> bool {
        self.search_word
    }

    /// Marks the last search's pattern as `*`'s or `#`'s (`word`), or not.
    pub(crate) fn set_search_word(&mut self, word: bool) {
        self.search_word = word;
    }

    /// Makes `offset` the last search's, as `/` and `?` do (`*`, `#` and an
    /// ex address's pattern drop it).
    pub(crate) fn set_search_offset(&mut self, offset: SearchOffset) {
        self.search_offset = offset;
    }

    /// Writes a recorded macro's `text` to `name`, `a-z` charwise, as
    /// `q{a-z}` does; `A-Z` append, as `q{A-Z}` does: to the last line of
    /// lines (still lines), or to a block's text (then charwise). `"` and
    /// the clipboard are left alone.
    pub(crate) fn record(&mut self, name: char, text: String) {
        let slot = &mut self.named[name.to_ascii_lowercase() as usize - 'a' as usize];
        *slot = Some(match slot.take().filter(|_| name.is_ascii_uppercase()) {
            Some(old) if old.linewise => {
                let mut lines = old.text;
                if lines.ends_with('\n') {
                    lines.pop();
                }
                Register::new(lines + &text + "\n", true)
            }
            Some(old) => Register::new(old.text + &text, false),
            None => Register::new(text, false),
        });
    }

    /// Notes the text the last insert or replace session typed, for `.`.
    pub(crate) fn set_inserted(&mut self, text: &str) {
        self.inserted = Some(Register::new(text, false));
    }

    /// Notes the last command line run, for `:`.
    pub(crate) fn set_command_line(&mut self, line: &str) {
        self.command_line = Some(Register::new(line, false));
    }

    /// Takes the inserted text a session of `state` left, if any, for `.`.
    pub(crate) fn adopt_inserted(&mut self, state: &mut crate::state::VimState) {
        if let Some((order, text)) = state.pending_inserted.take()
            && order > self.inserted_order
        {
            self.inserted_order = order;
            self.set_inserted(&text);
        }
    }

    /// Adds a line submitted on a command line of `kind` to its history,
    /// as the newest entry: an earlier identical one (for a search, typed
    /// with the same delimiter) is taken out, and past [`HISTORY_MAX`]
    /// entries the oldest goes. An empty line adds nothing.
    pub fn add_history(&mut self, kind: CommandLineKind, text: &str) {
        self.add_entry(kind, text, delimiter(kind));
    }

    /// Notes `pattern`, which `:s`, `:g`, an ex address or `*` made the last
    /// search, for the search history: nvim adds the last one such a line
    /// set when the line ends, or at once outside an ex line
    /// ([`Registers::enter_ex`]). It's recalled with no delimiter of its
    /// own, so only the other delimiter is escaped.
    pub(crate) fn note_pattern(&mut self, pattern: &str) {
        if pattern.is_empty() {
            return;
        }
        self.pending_pattern = Some(pattern.to_owned());
        if self.ex_depth == 0 {
            self.flush_pattern();
        }
    }

    /// A search line submitted inside an ex line (`:normal`): the pattern it
    /// sets replaces any the line set before.
    pub(crate) fn drop_pending_pattern(&mut self) {
        self.pending_pattern = None;
    }

    /// An ex line starts running: patterns it sets are noted until it ends.
    pub(crate) fn enter_ex(&mut self) {
        self.ex_depth += 1;
    }

    /// The ex line ends: the last pattern it set goes into the search
    /// history.
    pub(crate) fn leave_ex(&mut self) {
        self.ex_depth = self.ex_depth.saturating_sub(1);
        if self.ex_depth == 0 {
            self.flush_pattern();
        }
    }

    fn flush_pattern(&mut self) {
        if let Some(pattern) = self.pending_pattern.take() {
            self.add_entry(CommandLineKind::SearchForward, &pattern, NO_DELIMITER);
        }
    }

    fn add_entry(&mut self, kind: CommandLineKind, text: &str, delimiter: char) {
        if text.is_empty() {
            return;
        }
        let history = self.history_mut(kind);
        history.retain(|(entry, typed)| !(entry == text && *typed == delimiter));
        if history.len() >= HISTORY_MAX {
            history.pop_front();
        }
        history.push_back((text.to_owned(), delimiter));
    }

    /// The history a line of `kind` recalls from, oldest first, each entry
    /// as it was typed.
    pub fn history(&self, kind: CommandLineKind) -> impl Iterator<Item = &str> {
        let history = match kind {
            CommandLineKind::Ex => &self.ex_history,
            _ => &self.search_history,
        };
        history.iter().map(|(entry, _)| entry.as_str())
    }

    /// The next entry older (`older`) or newer than entry `from` (an index
    /// into [`Registers::history`]; one past the newest or more is the
    /// line as typed) that starts with `typed`, with its index, as `<Up>`
    /// and `<Down>` recall it on a line of `kind`. A search entry typed on
    /// the other kind of search line comes with its delimiters swapped:
    /// `foo/e` recalled on a `?` line is `foo?e`, and `a?b` is `a\?b`.
    pub fn recall(
        &self,
        kind: CommandLineKind,
        typed: &str,
        from: usize,
        older: bool,
    ) -> Option<(usize, Cow<'_, str>)> {
        let history = match kind {
            CommandLineKind::Ex => &self.ex_history,
            _ => &self.search_history,
        };
        let matches = |&ix: &usize| history[ix].0.starts_with(typed);
        let found = match older {
            true => (0..from.min(history.len())).rev().find(matches),
            false => (from.saturating_add(1)..history.len()).find(matches),
        }?;
        let (entry, typed_on) = &history[found];
        Some((found, retarget(entry, *typed_on, delimiter(kind))))
    }

    fn history_mut(&mut self, kind: CommandLineKind) -> &mut VecDeque<(String, char)> {
        match kind {
            CommandLineKind::Ex => &mut self.ex_history,
            _ => &mut self.search_history,
        }
    }

    fn write_clipboard(&mut self, value: &Register, host: &mut dyn EditorHost) {
        host.write_clipboard(value.text.clone());
        self.clipboard_last = Some(value.clone());
    }

    /// Writes register `name` itself, `A-Z` appending; returns the name
    /// `"` then reads.
    fn store(&mut self, name: char, value: Register) -> char {
        match name {
            '-' => self.small = Some(value),
            '0'..='9' => self.numbered[name as usize - '0' as usize] = Some(value),
            _ => {
                let lower = name.to_ascii_lowercase();
                let slot = &mut self.named[lower as usize - 'a' as usize];
                *slot = Some(match (name.is_ascii_uppercase(), slot.take()) {
                    (true, Some(old)) => append(old, value),
                    (_, _) => value,
                });
                return lower;
            }
        }
        name
    }
}

/// Where an open command line is in its history as `<Up>` and `<Down>`
/// step through it, for a host's line: the entry it shows, and the text
/// entries must start with, which is what the line held at the first step
/// after it was opened or last edited.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryRecall {
    /// The entry shown, `None` for the line as typed.
    at: Option<usize>,
    typed: Option<String>,
}

impl HistoryRecall {
    /// `<Up>` (`older`) or `<Down>` on a line of `kind` holding `line`: the
    /// text the line should hold now, `None` to leave it. `<Down>` past the
    /// newest entry puts back what was typed; `<Up>` past the oldest does
    /// nothing.
    pub fn step(
        &mut self,
        registers: &Registers,
        kind: CommandLineKind,
        line: &str,
        older: bool,
    ) -> Option<String> {
        let typed = self.typed.get_or_insert_with(|| line.to_owned());
        let from = self.at.unwrap_or(usize::MAX);
        match registers.recall(kind, typed, from, older) {
            Some((at, entry)) => {
                self.at = Some(at);
                Some(entry.into_owned())
            }
            None if older => None,
            None => {
                self.at = None;
                Some(typed.clone())
            }
        }
    }

    /// The line was edited: the next step recalls entries starting with
    /// what it then holds, from the entry it shows.
    pub fn edited(&mut self) {
        self.typed = None;
    }
}

/// The delimiter of a search history entry no line was typed for.
const NO_DELIMITER: char = '\0';

/// The delimiter a line of `kind` is typed after.
fn delimiter(kind: CommandLineKind) -> char {
    match kind {
        CommandLineKind::SearchForward => '/',
        CommandLineKind::SearchBackward => '?',
        CommandLineKind::Ex => ':',
    }
}

/// A search line typed after `from`, as recalled after `to`, as nvim
/// recalls it: an unescaped `from` becomes `to`, and an unescaped `to` is
/// escaped. What a backslash escapes stays as it is, even `\from`, and a
/// collection gets no exception (`a[/]b` becomes `a[?]b`). A `from` of
/// [`NO_DELIMITER`] only escapes `to`.
fn retarget(text: &str, from: char, to: char) -> Cow<'_, str> {
    if from == to || !text.contains([from, to]) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 1);
    let mut previous = None;
    for ch in text.chars() {
        // As nvim does, only the character just before decides.
        let escaped = previous == Some('\\');
        match ch {
            _ if escaped => out.push(ch),
            _ if ch == from => out.push(to),
            _ if ch == to => out.extend(['\\', to]),
            _ => out.push(ch),
        }
        previous = Some(ch);
    }
    Cow::Owned(out)
}

/// `old` with `new` appended, as nvim appends: whole lines if either is; to
/// a block, `new`'s rows as more rows, the block keeping its width; to
/// characters, its text (a block's rows joined by `\n`).
fn append(old: Register, new: Register) -> Register {
    if let (Some(width), false) = (old.block_width, new.linewise) {
        let rows: Vec<String> = [old.text, new.text].into();
        return Register::block(&rows, width);
    }
    if !old.linewise && !new.linewise {
        return Register::new(old.text + &new.text, false);
    }
    // Each row becomes a line, a block's or characters' empty last one too.
    let line = |register: Register| {
        let mut text = register.text;
        if !register.linewise || !text.ends_with('\n') {
            text.push('\n');
        }
        text
    };
    Register::new(line(old) + &line(new), true)
}

#[cfg(test)]
mod inserted_order_tests {
    use super::*;
    use crate::state::VimState;

    #[test]
    fn a_session_closed_late_does_not_replace_a_newer_one() {
        let mut registers = Registers::new();
        let mut newer = VimState::new();
        newer.offer_inserted(5, "y".into());
        registers.adopt_inserted(&mut newer);
        // A clone that ended its session before it closes it now.
        let mut older = VimState::new();
        older.offer_inserted(3, "x".into());
        registers.adopt_inserted(&mut older);
        assert_eq!(registers.get('.'), Some(&Register::new("y", false)));
        // Nor does the same session published twice.
        older.offer_inserted(5, "z".into());
        registers.adopt_inserted(&mut older);
        assert_eq!(registers.get('.'), Some(&Register::new("y", false)));
    }
}

#[cfg(test)]
mod search_provenance_tests {
    use super::*;

    #[test]
    fn reusing_the_last_pattern_keeps_its_word_provenance() {
        let mut registers = Registers::new();
        registers.set_search("Foo", true);
        registers.set_search_word(true);
        registers.set_search("Foo", true);
        assert!(registers.last_search_word(), "same pattern reused");
        registers.set_search("Bar", true);
        assert!(!registers.last_search_word(), "a new pattern");
    }

    #[test]
    fn incsearch_is_on_until_switched_off() {
        let mut registers = Registers::new();
        assert!(registers.incsearch());
        registers.set_incsearch(false);
        assert!(!registers.incsearch());
    }
}
