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
};

use fe_vim::{HeadlessVim, VimKey, parse_keys};
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
    pub(crate) expect: Outcome,
}

impl Recorded {
    pub(crate) fn new(case: Case, expect: Outcome) -> Self {
        let Case {
            name,
            buffer,
            cursor,
            keys,
        } = case;
        Self {
            name,
            buffer,
            cursor,
            keys,
            expect,
        }
    }

    pub(crate) fn case(&self) -> Case {
        Case {
            name: self.name.clone(),
            buffer: self.buffer.clone(),
            cursor: self.cursor,
            keys: self.keys.clone(),
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
    /// `'<` and `'>`, those that are set: a linewise `'>` is on its row's
    /// last byte.
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
}

impl NvimResult {
    pub(crate) fn outcome(self) -> Outcome {
        Outcome {
            buffer: self.buffer,
            cursor: self.cursor,
            mode: self.mode,
            registers: self.registers,
            visual: self.visual,
            marks: self.marks,
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
        let keys = parse_keys(&case.keys).map_err(|error| format!("{name}: {error}"))?;
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
    Ok(file.cases)
}

/// Whether `keys` may name register `+` or `*`: a `+` or `*` anywhere in
/// keys that also hold a key that can take a register name or run a command
/// (`"`, `@`, `q`, `<C-r>`, `:`, `=`). It over-rejects (`f"+` is out too):
/// telling a register name from a find's target or inserted text needs the
/// whole grammar, insert mode and macro recording included. The recorder
/// can't reach the real clipboard either way: `oracle.lua` turns off nvim's
/// clipboard provider.
fn names_clipboard(keys: &[VimKey]) -> bool {
    let clipboard = |key: &VimKey| matches!(key, VimKey::Char('+' | '*'));
    let takes_register = |key: &VimKey| {
        matches!(
            key,
            VimKey::Char('"' | '@' | 'q' | ':' | '=') | VimKey::Ctrl('r')
        )
    };
    keys.iter().any(clipboard) && keys.iter().any(takes_register)
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

/// `vim`'s `'<` and `'>`, those that are set.
fn marks(vim: &HeadlessVim) -> BTreeMap<String, (u32, usize)> {
    ['<', '>']
        .into_iter()
        .filter_map(|name| Some((name.to_string(), vim.mark(name)?)))
        .collect()
}

/// Drives `recorded`'s keys through [`HeadlessVim`] and compares the buffer,
/// the cursor, the mode, the registers, the visual anchor and the `'<` and
/// `'>` marks with nvim's.
pub(crate) fn check(recorded: &Recorded) -> Result<(), String> {
    let (case, expect) = (recorded.case(), &recorded.expect);
    let mut vim = HeadlessVim::new(&case.buffer, case.cursor);
    vim.feed(&case.keys)
        .map_err(|error| format!("{}: {error}", case.name))?;
    let got = (
        vim.text(),
        vim.cursor(),
        vim.mode(),
        registers(&vim),
        vim.visual_anchor(),
        marks(&vim),
    );
    let want = (
        expect.buffer.clone(),
        expect.cursor,
        mode_name(&expect.mode),
        expect.registers.clone(),
        expect.visual,
        expect.marks.clone(),
    );
    if got != want {
        return Err(format!(
            "{} ({:?} at {:?}, keys {:?}): got {got:?}, nvim {want:?}",
            case.name, case.buffer, case.cursor, case.keys
        ));
    }
    Ok(())
}
