//! The Neovim oracle's harness: corpus files, their recorded expectations,
//! and driving a case through [`HeadlessVim`].
//!
//! `tests/corpus/<area>.toml` holds hand-written `[[case]]` tables;
//! `<area>.expected.json` holds what nvim did with each, written only by the
//! ignored recorder (`oracle.lua` does the driving). The inputs are repeated
//! in the expected file, so a case edited without a re-record fails as
//! stale.

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

use fe_vim::{HeadlessVim, RemapSet, VimKey, parse_keys, parse_vimrc};
use serde::{Deserialize, Serialize};

/// The nvim the expectations were recorded with.
pub(crate) const NVIM: &str = "0.12.4";

/// One hand-written case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Case {
    pub(crate) name: String,
    /// Rows joined with `\n`, as nvim holds them: no trailing newline.
    pub(crate) buffer: String,
    /// A row and a byte column, both from 0.
    pub(crate) cursor: (u32, usize),
    /// Vim notation.
    pub(crate) keys: String,
    /// The window's height, which makes the case record the view's top
    /// row too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) window_rows: Option<u32>,
    /// What nvim is fed instead of `keys`, for a command it doesn't have
    /// (the jump, swapped for a mark motion to the same place).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) oracle_keys: Option<String>,
    /// Closed folds, a first and a last row from 0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) folds: Vec<(u32, u32)>,
    /// Soft wrap on, at nvim's 80 columns.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) soft_wrap: bool,
    /// Map commands (vimrc lines) run before the keys: in nvim with
    /// `vim.cmd`, here read by [`parse_vimrc`] into a [`RemapSet`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) mapping: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorpusFile {
    #[serde(rename = "case")]
    cases: Vec<Case>,
}

/// An expected file: the nvim version and each case with its result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expected {
    pub(crate) nvim: String,
    pub(crate) cases: Vec<Recorded>,
}

/// A case's inputs, repeated, and nvim's result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Recorded {
    pub(crate) name: String,
    pub(crate) buffer: String,
    pub(crate) cursor: (u32, usize),
    pub(crate) keys: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) window_rows: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) oracle_keys: Option<String>,
    /// Closed folds, a first and a last row from 0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) folds: Vec<(u32, u32)>,
    /// Soft wrap on, at nvim's 80 columns.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) soft_wrap: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) mapping: Vec<String>,
    pub(crate) expect: Expect,
}

impl Recorded {
    pub(crate) fn new(case: Case, expect: impl Into<Expect>) -> Self {
        let Case {
            name,
            buffer,
            cursor,
            keys,
            window_rows,
            oracle_keys,
            folds,
            soft_wrap,
            mapping,
        } = case;
        Self {
            name,
            buffer,
            cursor,
            keys,
            window_rows,
            oracle_keys,
            folds,
            soft_wrap,
            mapping,
            expect: expect.into(),
        }
    }

    pub(crate) fn case(&self) -> Case {
        Case {
            name: self.name.clone(),
            buffer: self.buffer.clone(),
            cursor: self.cursor,
            keys: self.keys.clone(),
            window_rows: self.window_rows,
            oracle_keys: self.oracle_keys.clone(),
            folds: self.folds.clone(),
            soft_wrap: self.soft_wrap,
            mapping: self.mapping.clone(),
        }
    }
}

/// What nvim was left with after the keys, as recorded: an [`Outcome`],
/// and the view's top row for a case with `window_rows`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Expect {
    pub(crate) buffer: String,
    pub(crate) cursor: (u32, usize),
    pub(crate) mode: String,
    pub(crate) registers: BTreeMap<String, Register>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) visual: Option<(u32, usize)>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) marks: BTreeMap<String, (u32, usize)>,
    /// The first row the window shows (`line('w0') - 1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) top: Option<u32>,
}

impl From<Outcome> for Expect {
    fn from(outcome: Outcome) -> Self {
        let Outcome {
            buffer,
            cursor,
            mode,
            registers,
            visual,
            marks,
        } = outcome;
        Self {
            buffer,
            cursor,
            mode,
            registers,
            visual,
            marks,
            top: None,
        }
    }
}

/// What nvim was left with after the keys.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Outcome {
    pub(crate) buffer: String,
    pub(crate) cursor: (u32, usize),
    /// `nvim_get_mode().mode`: `n` is normal.
    pub(crate) mode: String,
    /// Each non-empty register of `" 0-9 - a-z /`.
    pub(crate) registers: BTreeMap<String, Register>,
    /// The visual anchor (`getpos('v')`), if the keys end in a visual mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) visual: Option<(u32, usize)>,
    /// The marks among `'<`, `'>` and `'a` to `'z` that are set: a linewise
    /// `'>` is on its row's last byte.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) marks: BTreeMap<String, (u32, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Register {
    pub(crate) text: String,
    pub(crate) linewise: bool,
    /// A blockwise register's width (`getregtype()` is `<C-v>{width}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) block_width: Option<usize>,
}

/// What `oracle.lua` writes per case.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NvimResult {
    pub(crate) name: String,
    pub(crate) buffer: String,
    pub(crate) cursor: (u32, usize),
    pub(crate) mode: String,
    pub(crate) registers: BTreeMap<String, Register>,
    #[serde(default)]
    pub(crate) visual: Option<(u32, usize)>,
    #[serde(default)]
    pub(crate) marks: BTreeMap<String, (u32, usize)>,
    #[serde(default)]
    pub(crate) top: Option<u32>,
}

impl NvimResult {
    pub(crate) fn outcome(self) -> Expect {
        Expect {
            buffer: self.buffer,
            cursor: self.cursor,
            mode: self.mode,
            registers: self.registers,
            visual: self.visual,
            marks: self.marks,
            top: self.top,
        }
    }
}

pub(crate) fn crate_dir() -> PathBuf {
    // Read at run time: a test binary built in another worktree sharing the
    // target would otherwise carry that worktree's path.
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

pub(crate) fn corpus_dir() -> PathBuf {
    crate_dir().join("tests/corpus")
}

/// Corpus files that are fastedit's own spec, not recorded from nvim:
/// `jump_labels.toml`, which `tests/jump_spec.rs` reads.
const NOT_RECORDED: [&str; 1] = ["jump_labels"];

/// The corpus files recorded from nvim, `tests/corpus/<area>.toml`, sorted.
pub(crate) fn recorded_corpora() -> Vec<PathBuf> {
    let mut corpora: Vec<PathBuf> = std::fs::read_dir(corpus_dir())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .filter(|path| {
            let stem = path.file_stem().unwrap().to_string_lossy();
            !NOT_RECORDED.contains(&stem.as_ref())
        })
        .collect();
    corpora.sort();
    corpora
}

/// Reads a corpus file, rejecting cases the oracle can't record faithfully.
pub(crate) fn parse_corpus(source: &str) -> Result<Vec<Case>, String> {
    let file: CorpusFile = toml::from_str(source).map_err(|error| error.to_string())?;
    let mut names = HashSet::new();
    for case in &file.cases {
        let name = &case.name;
        if !names.insert(name.as_str()) {
            return Err(format!("{name}: duplicate case name"));
        }
        if case.buffer.ends_with('\n') {
            return Err(format!(
                "{name}: the buffer ends in a newline; rows are joined with \\n, as nvim holds them"
            ));
        }
        for keys in std::iter::once(&case.keys).chain(&case.oracle_keys) {
            let keys = parse_keys(keys).map_err(|error| format!("{name}: {error}"))?;
            if keys.first() == Some(&VimKey::Char('.')) {
                return Err(format!(
                    "{name}: keys start with `.`, and nvim's last change can't be reset between cases"
                ));
            }
            if names_clipboard(&keys) {
                return Err(format!(
                    "{name}: keys name register + or *, which headless nvim would take from the real clipboard"
                ));
            }
        }
    }
    Ok(file.cases)
}

/// Whether `keys` may name register `+` or `*`: a `+` or `*` anywhere in
/// keys that also hold a key that can take a register name or run a command
/// (`"`, `@`, `q`, `<C-r>`, `:`, `=`). It over-rejects (`f"+` is out too):
/// telling a register name from a find's target or inserted text needs the
/// whole grammar, insert mode and macro recording included. From a `:` to
/// the `<CR>` (or `<Esc>`) ending its line, `+` and `*` are allowed where
/// they can't name a register (`:.,+2d`, `:s/a*/b/`; see
/// [`line_names_clipboard`]). The recorder can't reach the real clipboard
/// either way: `oracle.lua` turns off nvim's clipboard provider.
fn names_clipboard(keys: &[VimKey]) -> bool {
    let clipboard = |key: &VimKey| matches!(key, VimKey::Char('+' | '*'));
    let takes_register = |key: &VimKey| {
        matches!(
            key,
            VimKey::Char('"' | '@' | 'q' | ':' | '=') | VimKey::Ctrl('r')
        )
    };
    let mut outside = Vec::new();
    let mut lines = Vec::new();
    let mut keys_left = keys.iter();
    while let Some(key) = keys_left.next() {
        // A `*` right after `g` is `g*`, or follows the register `g`: never
        // the clipboard's name.
        if *key == VimKey::Char('*') && outside.last() == Some(&VimKey::Char('g')) {
            continue;
        }
        if *key != VimKey::Char(':') {
            outside.push(*key);
            continue;
        }
        let line: Vec<VimKey> = keys_left
            .by_ref()
            .take_while(|key| !matches!(key, VimKey::Enter | VimKey::Esc))
            .copied()
            .collect();
        lines.push(line);
    }
    (outside.iter().any(clipboard) && keys.iter().any(takes_register))
        || lines.iter().any(|line| line_names_clipboard(line))
}

/// Whether a `+` or `*` on a `:` line's keys may name a register. Only two
/// places are safe: the range (`:.,+2d`, `:/a*/`), and the pattern and
/// replacement of a substitute (`:s/a*/b/`, `:&`, `:~`) on a line with no
/// `"`, `@`, `=`, `|` or `\\` (no register, expression or second command),
/// no `<C-r>` and no other special key. Anywhere else (`:d +`, `:pu +`,
/// `:sil d *`, `:g/a/y +`, `:call setreg('+', 'x')`) it may.
fn line_names_clipboard(line: &[VimKey]) -> bool {
    let mut text = String::new();
    for key in line {
        match key {
            VimKey::Char(ch) => text.push(*ch),
            _ => text.push('\0'),
        }
    }
    if !text.contains(['+', '*']) {
        return false;
    }
    if text.contains('\0') {
        return true;
    }
    let after = &text[skip_range(&text)..];
    if !after.contains(['+', '*']) {
        return false;
    }
    let name = after
        .find(|ch: char| !ch.is_ascii_alphabetic())
        .map_or(after, |end| &after[..end]);
    let substitute = (!name.is_empty() && "substitute".starts_with(name))
        || (name.is_empty() && after.starts_with(['&', '~']));
    !substitute || after.contains(['"', '@', '=', '|', '\\'])
}

/// Where a `:` line's command starts: past its blanks and line
/// specifiers, patterns included.
fn skip_range(line: &str) -> usize {
    let mut chars = line.char_indices().peekable();
    while let Some(&(at, ch)) = chars.peek() {
        match ch {
            ' ' | '\t' | ':' | '.' | '$' | '%' | '*' | ',' | ';' | '+' | '-' | '0'..='9' => {
                chars.next();
            }
            '\'' | '\\' => {
                chars.next();
                chars.next();
            }
            '/' | '?' => {
                chars.next();
                while let Some((_, next)) = chars.next() {
                    match next {
                        '\\' => {
                            chars.next();
                        }
                        _ if next == ch => break,
                        _ => {}
                    }
                }
            }
            _ => return at,
        }
    }
    line.len()
}

/// Reads an expected file and pairs it with `cases`: it must hold exactly
/// those cases, in order, with the same inputs, recorded by [`NVIM`].
pub(crate) fn parse_expected(source: &str, cases: &[Case]) -> Result<Vec<Recorded>, String> {
    let expected: Expected = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if expected.nvim != NVIM {
        return Err(format!(
            "recorded with nvim {}, not {NVIM}: re-record",
            expected.nvim
        ));
    }
    let recorded: HashSet<&str> = expected.cases.iter().map(|r| r.name.as_str()).collect();
    if let Some(missing) = cases
        .iter()
        .find(|case| !recorded.contains(case.name.as_str()))
    {
        return Err(format!("{}: not recorded: re-record", missing.name));
    }
    if recorded.len() != expected.cases.len() {
        return Err("a case is recorded twice: re-record".to_string());
    }
    let names: HashSet<&str> = cases.iter().map(|case| case.name.as_str()).collect();
    if let Some(extra) = expected
        .cases
        .iter()
        .find(|r| !names.contains(r.name.as_str()))
    {
        return Err(format!("{}: recorded but not in the corpus", extra.name));
    }
    for (case, recorded) in cases.iter().zip(&expected.cases) {
        if *case != recorded.case() {
            return Err(format!(
                "{}: the corpus case differs from the one recorded (or is out of order): re-record",
                case.name
            ));
        }
    }
    Ok(expected.cases)
}

/// Loads `<area>.toml` and its expected file.
pub(crate) fn load(area: &str) -> Result<Vec<Recorded>, String> {
    let read = |file: String| {
        std::fs::read_to_string(corpus_dir().join(&file))
            .map_err(|error| format!("{file}: {error}"))
    };
    let cases = parse_corpus(&read(format!("{area}.toml"))?)?;
    parse_expected(&read(format!("{area}.expected.json"))?, &cases)
}

/// nvim's mode name as `vim.mode` spells it.
fn mode_name(nvim: &str) -> &str {
    match nvim {
        "n" => "normal",
        "no" => "operator_pending",
        "i" => "insert",
        "R" => "replace",
        "v" => "visual",
        "V" => "visual_line",
        "\u{16}" => "visual_block",
        other => other,
    }
}

/// The registers the oracle records, `" 0-9 - a-z` and `/`, the last
/// search pattern.
const REGISTERS: &str = "\"0123456789-abcdefghijklmnopqrstuvwxyz/";

/// `vim`'s non-empty registers among [`REGISTERS`], as the oracle records
/// them.
fn registers(vim: &HeadlessVim) -> BTreeMap<String, Register> {
    REGISTERS
        .chars()
        .filter_map(|name| {
            let register = vim.registers().get(name)?;
            let register = Register {
                text: register.text.clone(),
                linewise: register.linewise,
                block_width: register.block_width,
            };
            (!register.text.is_empty()).then(|| (name.to_string(), register))
        })
        .collect()
}

/// The marks the oracle records: `<`, `>` and `a` to `z`.
const MARKS: &str = "<>abcdefghijklmnopqrstuvwxyz";

/// `vim`'s marks among [`MARKS`], those that are set.
fn marks(vim: &HeadlessVim) -> BTreeMap<String, (u32, usize)> {
    MARKS
        .chars()
        .filter_map(|name| Some((name.to_string(), vim.mark(name)?)))
        .collect()
}

/// Drives `recorded`'s keys through [`HeadlessVim`] and compares the buffer,
/// the cursor, the mode, the registers, the visual anchor and the marks
/// ([`MARKS`]) with nvim's, and with `window_rows` (the view that many rows
/// high) the view's top row. A case's `mapping` is read by [`parse_vimrc`]
/// first, and any issue fails the case.
pub(crate) fn check(recorded: &Recorded) -> Result<(), String> {
    let (case, expect) = (recorded.case(), &recorded.expect);
    let mut vim = HeadlessVim::new(&case.buffer, case.cursor);
    for &(first, last) in &case.folds {
        vim.model_mut().fold_rows(first..=last);
    }
    if case.soft_wrap {
        let model = vim.model_mut();
        model.set_soft_wrap(true);
        model.set_wrap_width(80.);
        while model.wrap_pending(0) {}
    }
    if let Some(rows) = case.window_rows {
        vim.set_window_rows(rows);
    }
    if !case.mapping.is_empty() {
        let vimrc = parse_vimrc(&case.mapping.join("\n"));
        if !vimrc.issues.is_empty() {
            return Err(format!("{}: mapping: {:?}", case.name, vimrc.issues));
        }
        let (set, issues) = RemapSet::new(&vimrc.remaps, "\\");
        if !issues.is_empty() {
            return Err(format!("{}: mapping: {issues:?}", case.name));
        }
        vim.state_mut().set_remaps(Arc::new(set));
    }
    vim.feed(&case.keys)
        .map_err(|error| format!("{}: {error}", case.name))?;
    let got = (
        vim.text(),
        vim.cursor(),
        vim.mode(),
        registers(&vim),
        vim.visual_anchor(),
        marks(&vim),
        case.window_rows.map(|_| vim.top_row()),
    );
    let want = (
        expect.buffer.clone(),
        expect.cursor,
        mode_name(&expect.mode),
        expect.registers.clone(),
        expect.visual,
        expect.marks.clone(),
        expect.top,
    );
    if got != want {
        return Err(format!(
            "{} ({:?} at {:?}, keys {:?}): got {got:?}, nvim {want:?}",
            case.name, case.buffer, case.cursor, case.keys
        ));
    }
    Ok(())
}
