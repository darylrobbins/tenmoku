//! Key-to-key remaps as written (`Remap`), and the set built from them
//! (`RemapSet`) that the engine matches typed keys against. See
//! `docs/decisions/vim-remaps.md`; nothing here runs a remap.

use crate::key::{VimKey, parse_keys};

/// nvim's `n`, `x` and `o` map modes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RemapMode {
    Normal,
    Visual,
    OperatorPending,
}

/// One map as written, in vim notation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remap {
    pub modes: Vec<RemapMode>,
    pub from: String,
    pub to: String,
    pub recursive: bool,
}

/// An entry that can't load, by its index in the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemapIssue {
    pub index: usize,
    pub message: String,
}

/// What typed keys mean against a mode's maps (§4 rule 3).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Lookup<'a> {
    None,
    /// A longer map starts with the keys.
    Prefix,
    Exact {
        to: &'a [VimKey],
        recursive: bool,
    },
    /// An exact map that is also the start of a longer one.
    ExactAndPrefix {
        to: &'a [VimKey],
        recursive: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    mode: RemapMode,
    text: String,
    from: Vec<VimKey>,
    to: Vec<VimKey>,
    recursive: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemapSet {
    entries: Vec<Entry>,
}

impl RemapSet {
    /// Builds the set. `leader` is in vim notation (`"\\"`, `"<Space>"`).
    /// An entry that can't load is left out alone, with one issue.
    pub fn new(remaps: &[Remap], leader: &str) -> (RemapSet, Vec<RemapIssue>) {
        let mut set = RemapSet::default();
        let mut issues = Vec::new();
        for (index, remap) in remaps.iter().enumerate() {
            match load(remap, leader) {
                Ok((from, to)) => {
                    for &mode in &remap.modes {
                        set.entries.retain(|e| !(e.mode == mode && e.from == from));
                        set.entries.push(Entry {
                            mode,
                            text: remap.from.clone(),
                            from: from.clone(),
                            to: to.clone(),
                            recursive: remap.recursive,
                        });
                    }
                }
                Err(message) => issues.push(RemapIssue { index, message }),
            }
        }
        (set, issues)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn lookup(&self, mode: RemapMode, keys: &[VimKey]) -> Lookup<'_> {
        if keys.is_empty() {
            return Lookup::None;
        }
        let mut exact = None;
        let mut prefix = false;
        for entry in self.entries.iter().filter(|e| e.mode == mode) {
            if entry.from == keys {
                exact = Some((entry.to.as_slice(), entry.recursive));
            } else if entry.from.len() > keys.len() && entry.from.starts_with(keys) {
                prefix = true;
            }
        }
        match (exact, prefix) {
            (None, false) => Lookup::None,
            (None, true) => Lookup::Prefix,
            (Some((to, recursive)), false) => Lookup::Exact { to, recursive },
            (Some((to, recursive)), true) => Lookup::ExactAndPrefix { to, recursive },
        }
    }

    /// The `from`s, as written, that are one key which starts a built-in
    /// multi-key command in `mode`, so the map hides that command (§4
    /// rule 4).
    pub fn hides_builtin(&self, mode: RemapMode) -> Vec<String> {
        const PREFIXES: &str = "gzZ[]\"fFtTrm'`q@";
        const OPERATORS: &str = "cdy<>!=";
        self.entries
            .iter()
            .filter(|e| e.mode == mode)
            .filter(|e| match e.from.as_slice() {
                [VimKey::Char(c)] => match mode {
                    RemapMode::Normal => PREFIXES.contains(*c) || OPERATORS.contains(*c),
                    RemapMode::Visual => PREFIXES.contains(*c),
                    RemapMode::OperatorPending => "gz[]'`fFtTia".contains(*c),
                },
                // Two-key operators: `gU` hides `gUU` and `gUw`.
                [VimKey::Char(a), VimKey::Char(b)] => {
                    mode == RemapMode::Normal
                        && matches!(
                            (a, b),
                            ('g', 'u' | 'U' | '~' | '?' | 'q' | 'w' | '@') | ('z', 'f')
                        )
                }
                _ => false,
            })
            .map(|e| e.text.clone())
            .collect()
    }
}

fn load(remap: &Remap, leader: &str) -> Result<(Vec<VimKey>, Vec<VimKey>), String> {
    let leader_keys = parse_keys(leader).map_err(|e| e.to_string())?;
    let from = side(&remap.from, &leader_keys, true)?;
    if from.is_empty() {
        return Err("empty left side".to_string());
    }
    let to = if remap.to.eq_ignore_ascii_case("<nop>") {
        Vec::new()
    } else {
        side(&remap.to, &leader_keys, false)?
    };
    Ok((from, to))
}

/// One side's keys, with each `<leader>` replaced by the leader's keys
/// after the text around it is read, so the leader can't merge into a
/// neighbouring `<name>`.
fn side(text: &str, leader: &[VimKey], is_from: bool) -> Result<Vec<VimKey>, String> {
    let mut keys = Vec::new();
    for (i, piece) in split_leader(text).into_iter().enumerate() {
        if i > 0 {
            keys.extend_from_slice(leader);
        }
        for name in names(piece) {
            match name.to_ascii_lowercase().as_str() {
                "nop" if is_from => return Err("<Nop> can't be a left side".to_string()),
                "cmd" if is_from => return Err("<Cmd> in a left side is not supported".to_string()),
                "cmd" => return Err("<Cmd> is not supported yet".to_string()),
                other if unsupported(other) => return Err(format!("<{name}> is not supported")),
                _ => {}
            }
        }
        keys.extend(parse_keys(piece).map_err(|e| e.to_string())?);
    }
    Ok(keys)
}

fn unsupported(lower_name: &str) -> bool {
    matches!(lower_name, "plug" | "sid" | "snr" | "localleader")
}

/// Each `<name>` in vim notation, by the rule `parse_keys` reads them.
fn names(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        let name = rest
            .strip_prefix('<')
            .and_then(|after| after.split_once('>'))
            .map(|(name, _)| name)
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            });
        match name {
            Some(name) => {
                found.push(name);
                rest = &rest[name.len() + 2..];
            }
            None => rest = &rest[ch.len_utf8()..],
        }
    }
    found
}

/// The text between each `<leader>` (any case).
fn split_leader(text: &str) -> Vec<&str> {
    const TAG: &str = "<leader>";
    let lower = text.to_ascii_lowercase();
    let mut pieces = Vec::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find(TAG) {
        pieces.push(&text[at..at + found]);
        at += found + TAG.len();
    }
    pieces.push(&text[at..]);
    pieces
}
