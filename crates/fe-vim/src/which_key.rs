//! Which-key: the keys that go on from a prefix waiting, each with what to
//! call it, for a popup the host shows ([`VimState::which_key`]), after
//! which-key.nvim's model of triggers and their continuations.
//!
//! Two sources: while the reducer waits on `g`, `z`, `Z`, `[` or `]` in
//! normal or a visual mode, the keys of [`BUILTIN_KEYS`] that it takes there
//! now; and while the remap engine holds keys for a longer map (`<leader>`
//! of `<leader>ff`), the next key of every longer map of the mode, a group
//! where more keys go on. A map's key wins over a built-in's: with `gx`
//! mapped, `g` is held and lists both, `x` as the map.
//!
//! Nothing in operator-pending mode (`d`, `y`, `c` list no motions), nor
//! after a count or register alone, nor for another prefix (`f`, `"`, `m`,
//! `gs`); keymap chords (`] d`, `<C-w>`) are the keymap matcher's.

use crate::feature::replace_register;
use crate::vimrc::notation;
use crate::{Mode, Pending, RemapMode, Step, VimKey, VimState};

/// What goes on from the prefix waiting ([`VimState::which_key`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhichKey {
    /// The prefix's keys, in vim notation: `g`, or the keys held (`\f`).
    pub prefix: String,
    /// Sorted by key, case aside, a lowercase one before its uppercase.
    pub entries: Vec<WhichKeyEntry>,
}

/// One key that goes on from a prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhichKeyEntry {
    /// In vim notation: `g`, `<C-a>`, `<CR>`.
    pub key: String,
    pub label: WhichKeyLabel,
    /// More keys go on after it: maps longer than one ending at it.
    pub group: bool,
}

/// What to call a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WhichKeyLabel {
    /// A built-in key's title (`gg`: "first line").
    Builtin(&'static str),
    /// A map running the fastedit command of this id, alone
    /// (`<Cmd>fe:{id}<CR>`): the host's catalog has its title.
    Command(String),
    /// A map's right side, as written.
    Keys(String),
    /// No map ends at the key: this many longer ones go on past it.
    Group(usize),
}

/// A built-in prefix's keys, each with its title.
#[derive(Copy, Clone, Debug)]
pub struct BuiltinKeys {
    /// `g`, `z`, `Z`, `[` or `]`.
    pub prefix: char,
    /// Every key the reducer takes after it, in normal or a visual mode.
    pub keys: &'static [(VimKey, &'static str)],
}

const fn ch(ch: char) -> VimKey {
    VimKey::Char(ch)
}

/// The keys the reducer and the features take after each prefix: a key
/// lists where the reducer takes it now (`g&` not in a visual mode, `gr`
/// only with ReplaceWithRegister on).
pub const BUILTIN_KEYS: &[BuiltinKeys] = &[
    BuiltinKeys {
        prefix: 'g',
        keys: &[
            (ch('#'), "partial word backward"),
            (ch('$'), "end of screen line"),
            (ch('&'), "repeat :s on all lines"),
            (ch('*'), "partial word forward"),
            (ch(','), "newer change"),
            (ch(';'), "older change"),
            (ch('?'), "rot13"),
            (ch('_'), "last non-blank"),
            (ch('~'), "toggle case"),
            (ch('c'), "comment"),
            (ch('e'), "end of previous word"),
            (ch('E'), "end of previous WORD"),
            (ch('g'), "first line"),
            (ch('i'), "insert where insert stopped"),
            (ch('j'), "down a display line"),
            (ch('J'), "join without spaces"),
            (ch('k'), "up a display line"),
            (ch('m'), "middle of screen line"),
            (ch('p'), "put after, cursor after it"),
            (ch('P'), "put before, cursor after it"),
            (ch('q'), "format"),
            (ch('r'), "replace with register"),
            (ch('R'), "virtual replace mode"),
            (ch('s'), "jump forward"),
            (ch('S'), "jump backward"),
            (ch('u'), "lowercase"),
            (ch('U'), "uppercase"),
            (ch('v'), "reselect"),
            (ch('w'), "format, keep cursor"),
            (VimKey::Ctrl('a'), "increment progressively"),
            (VimKey::Ctrl('x'), "decrement progressively"),
        ],
    },
    BuiltinKeys {
        prefix: 'z',
        keys: &[
            (VimKey::Enter, "scroll top, first non-blank"),
            (ch('-'), "scroll bottom, first non-blank"),
            (ch('.'), "scroll center, first non-blank"),
            (ch('a'), "toggle fold"),
            (ch('A'), "toggle folds recursively"),
            (ch('b'), "scroll bottom"),
            (ch('c'), "close fold"),
            (ch('C'), "close folds recursively"),
            (ch('d'), "delete fold"),
            (ch('D'), "delete folds recursively"),
            (ch('E'), "delete all folds"),
            (ch('f'), "create fold"),
            (ch('F'), "create fold of lines"),
            (ch('j'), "next fold start"),
            (ch('k'), "previous fold end"),
            (ch('M'), "close all folds"),
            (ch('o'), "open fold"),
            (ch('O'), "open folds recursively"),
            (ch('R'), "open all folds"),
            (ch('t'), "scroll top"),
            (ch('v'), "open folds at cursor"),
            (ch('z'), "scroll center"),
        ],
    },
    BuiltinKeys {
        prefix: 'Z',
        keys: &[
            (ch('Q'), "quit without writing"),
            (ch('Z'), "write and quit"),
        ],
    },
    BuiltinKeys {
        prefix: '[',
        keys: &[
            (ch('('), "previous unmatched ("),
            (ch('{'), "previous unmatched {"),
            (ch('['), "previous section start"),
            (ch(']'), "previous section end"),
            (ch('c'), "previous change"),
            (ch('p'), "put before, indented"),
            (ch('P'), "put before, indented"),
        ],
    },
    BuiltinKeys {
        prefix: ']',
        keys: &[
            (ch(')'), "next unmatched )"),
            (ch('}'), "next unmatched }"),
            (ch(']'), "next section start"),
            (ch('['), "next section end"),
            (ch('c'), "next change"),
            (ch('p'), "put after, indented"),
            (ch('P'), "put before, indented"),
        ],
    },
];

impl VimState {
    /// The keys that go on from what waits now, the reducer's prefix or the
    /// keys the remap engine holds ([`VimState::remap_held`]); `None` if
    /// nothing goes on, or in operator-pending mode.
    pub fn which_key(&self) -> Option<WhichKey> {
        self.which_key_for(&self.remap_held())
    }

    /// [`VimState::which_key`], for a host that holds keys for a longer map
    /// itself: `keys` are those typed since nothing was pending but a count
    /// or a register.
    pub fn which_key_for(&self, keys: &[VimKey]) -> Option<WhichKey> {
        let (mode, visual) = match self.mode {
            Mode::Normal => (RemapMode::Normal, None),
            Mode::Visual(kind) => (RemapMode::Visual, Some(kind)),
            _ => return None,
        };
        let pending = self.pending.clone().in_visual(visual);
        // No map is read after a prefix of the reducer's.
        if pending.has_operator() || (pending.has_prefix() && !keys.is_empty()) {
            return None;
        }
        // The prefix the reducer waits on, or would once it reads `keys`.
        let waiting = match keys {
            [] => Some(pending),
            [key] => match crate::step(pending, *key) {
                Step::Wait(pending) => Some(pending),
                _ => None,
            },
            _ => None,
        };
        let builtin = waiting.and_then(|pending| Some((pending.which_key_prefix()?, pending)));
        let mut entries = Vec::new();
        if let Some((prefix, pending)) = &builtin {
            let table = BUILTIN_KEYS.iter().find(|keys| keys.prefix == *prefix);
            for &(key, title) in table.map_or(&[][..], |table| table.keys) {
                if self.takes(pending, key) {
                    entries.push(WhichKeyEntry {
                        key: notation(key),
                        label: WhichKeyLabel::Builtin(title),
                        group: false,
                    });
                }
            }
        }
        if !keys.is_empty() {
            for continuation in self.remaps().continuations(mode, keys) {
                let label = match (continuation.command, continuation.map) {
                    (Some(id), _) => WhichKeyLabel::Command(id.to_string()),
                    (None, Some(map)) => WhichKeyLabel::Keys(map.to.clone()),
                    (None, None) => WhichKeyLabel::Group(continuation.longer),
                };
                let key = notation(continuation.key);
                entries.retain(|entry: &WhichKeyEntry| entry.key != key);
                entries.push(WhichKeyEntry {
                    key,
                    label,
                    group: continuation.longer > 0,
                });
            }
        }
        if entries.is_empty() {
            return None;
        }
        entries.sort_by_cached_key(|entry| {
            let upper = entry.key.chars().next().is_some_and(char::is_uppercase);
            (entry.key.to_lowercase(), upper)
        });
        let prefix = match (keys, builtin) {
            ([], Some((prefix, _))) => prefix.to_string(),
            _ => keys.iter().map(|&key| notation(key)).collect(),
        };
        Some(WhichKey { prefix, entries })
    }

    /// Whether the reducer takes `key` after `pending`, as
    /// `VimState::reduce` reads a key after a prefix.
    fn takes(&self, pending: &Pending, key: VimKey) -> bool {
        match crate::step(pending.clone(), key) {
            Step::Cancel | Step::Forward => false,
            step => self.replace_with_register || !replace_register::takes(&step),
        }
    }
}
