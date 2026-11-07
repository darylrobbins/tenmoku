//! Which-key: the keys that go on from a prefix waiting, each with what to
//! call it, for a popup the host shows ([`VimState::which_key`]), after
//! which-key.nvim's model of triggers and their continuations.
//!
//! Two sources: while the reducer waits on `g`, `z`, `Z`, `[` or `]` in
//! normal or a visual mode, the keys of [`BUILTIN_KEYS`] that it takes there
//! now; and while the remap engine holds keys for a longer map (`<leader>`
//! of `<leader>ff`), the next key of every longer map of the mode, a group
//! where more keys go on, in insert, replace and command-line mode too. A
//! map's key wins over a built-in's: with `gx` mapped, `g` is held and
//! lists both, `x` as the map. With `g` itself mapped too, the held `g` is
//! the map's, so only the maps list.
//!
//! Nothing in operator-pending mode (`d`, `y`, `c` list no motions), nor
//! after a count or register alone, nor for another prefix (`f`, `"`, `m`,
//! `gs`, insert mode's `<C-r>`); keymap chords (`] d`, `<C-w>`) are the
//! keymap matcher's.

use crate::feature::replace_register;
use crate::remap::{Lookup, group_icon, group_name};
use crate::vimrc::notation;
use crate::{Mode, Pending, RemapMode, Step, VimKey, VimState};

/// What goes on from the prefix waiting ([`VimState::which_key`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhichKey {
    /// The prefix's keys, in vim notation: `g`, or the keys held (`\f`).
    pub prefix: String,
    /// Items first, then groups, each sorted by key, case aside, a
    /// lowercase one before its uppercase ([`WhichKey::sort_entries`]).
    pub entries: Vec<WhichKeyEntry>,
}

impl WhichKey {
    /// Puts the entries in the order a popup lists them: the keys that run
    /// something first, then the groups ([`WhichKeyEntry::group`]), each by
    /// key, case aside, a lowercase one before its uppercase.
    pub fn sort_entries(&mut self) {
        self.entries.sort_by_cached_key(|entry| {
            let upper = entry.key.chars().next().is_some_and(char::is_uppercase);
            (entry.group, entry.key.to_lowercase(), upper)
        });
    }
}

/// One key that goes on from a prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhichKeyEntry {
    /// In vim notation: `g`, `<C-a>`, `<CR>`.
    pub key: String,
    pub label: WhichKeyLabel,
    /// More keys go on after it: maps longer than one ending at it.
    pub group: bool,
    /// The name of an icon of the host's bundled set to draw beside it
    /// (`"bug"`): a named group's from its `:fegroup` or
    /// `[[vim.which_key_group]]` ([`crate::WhichKeyGroupName::icon`]). A
    /// host may give an item one by its command.
    pub icon: Option<String>,
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
    /// No map ends at the key: `count` longer ones go on past it. `name`
    /// is what a `:fegroup` or `[[vim.which_key_group]]` calls the
    /// prefix ending at the key in the mode, if one does; keymap chords
    /// (`ctrl-w`) have none.
    Group { count: usize, name: Option<String> },
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
            (ch('%'), "match backward"),
            (ch('&'), "repeat :s on all lines"),
            (ch('*'), "partial word forward"),
            (ch('+'), "newer text state"),
            (ch(','), "newer change"),
            (ch('-'), "older text state"),
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
            (ch('m'), "previous function start"),
            (ch('M'), "previous function end"),
            (ch('c'), "previous change"),
            (ch('p'), "put before, indented"),
            (ch('P'), "put before, indented"),
            (ch(' '), "blank line above"),
            (ch('e'), "move line up"),
        ],
    },
    BuiltinKeys {
        prefix: ']',
        keys: &[
            (ch(')'), "next unmatched )"),
            (ch('}'), "next unmatched }"),
            (ch(']'), "next section start"),
            (ch('['), "next section end"),
            (ch('m'), "next function start"),
            (ch('M'), "next function end"),
            (ch('c'), "next change"),
            (ch('p'), "put after, indented"),
            (ch('P'), "put before, indented"),
            (ch(' '), "blank line below"),
            (ch('e'), "move line down"),
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
        // The reducer's prefixes wait in normal and the visual modes, as
        // `Some` of the visual kind; insert, replace and the command line
        // have maps only.
        let (mode, visual) = match self.mode {
            Mode::Normal => (RemapMode::Normal, Some(None)),
            Mode::Visual(kind) => (RemapMode::Visual, Some(Some(kind))),
            // The key after `<C-r>` names a register, read unmapped.
            Mode::Insert | Mode::Replace if self.insert_register_waiting() => return None,
            Mode::Insert | Mode::Replace => (RemapMode::Insert, None),
            Mode::CmdLine(_) => (RemapMode::CmdLine, None),
        };
        let builtin = match visual {
            Some(visual) => {
                let pending = self.pending.clone().in_visual(visual);
                // No map is read after a prefix of the reducer's.
                if pending.has_operator() || (pending.has_prefix() && !keys.is_empty()) {
                    return None;
                }
                // Keys a map has whole are the map's, not the reducer's.
                let mapped = matches!(
                    self.remaps().lookup(mode, keys),
                    Lookup::Exact { .. } | Lookup::ExactAndPrefix { .. }
                );
                // The prefix the reducer waits on, or would once it reads
                // `keys`.
                let waiting = match keys {
                    [] => Some(pending),
                    [_] if mapped => None,
                    [key] => match crate::step(pending, *key) {
                        Step::Wait(pending) => Some(pending),
                        _ => None,
                    },
                    _ => None,
                };
                waiting.and_then(|pending| Some((pending.which_key_prefix()?, pending)))
            }
            None => None,
        };
        let mut entries = Vec::new();
        if let Some((prefix, pending)) = &builtin {
            let table = BUILTIN_KEYS.iter().find(|keys| keys.prefix == *prefix);
            for &(key, title) in table.map_or(&[][..], |table| table.keys) {
                if self.takes(pending, key) {
                    entries.push(WhichKeyEntry {
                        key: notation(key),
                        label: WhichKeyLabel::Builtin(title),
                        group: false,
                        icon: None,
                    });
                }
            }
        }
        if !keys.is_empty() {
            let mut prefix = keys.to_vec();
            for continuation in self.remaps().continuations(mode, keys) {
                let mut icon = None;
                let label = match (continuation.command, continuation.map) {
                    (Some(id), _) => WhichKeyLabel::Command(id.to_string()),
                    (None, Some(map)) => WhichKeyLabel::Keys(map.to.clone()),
                    (None, None) => {
                        prefix.push(continuation.key);
                        let groups = &self.which_key_groups;
                        let name = group_name(groups, mode, &prefix).map(str::to_string);
                        icon = group_icon(groups, mode, &prefix).map(str::to_string);
                        prefix.pop();
                        WhichKeyLabel::Group {
                            count: continuation.longer,
                            name,
                        }
                    }
                };
                let key = notation(continuation.key);
                entries.retain(|entry: &WhichKeyEntry| entry.key != key);
                entries.push(WhichKeyEntry {
                    key,
                    label,
                    group: continuation.longer > 0,
                    icon,
                });
            }
        }
        if entries.is_empty() {
            return None;
        }
        let prefix = match (keys, builtin) {
            ([], Some((prefix, _))) => prefix.to_string(),
            _ => keys.iter().map(|&key| notation(key)).collect(),
        };
        let mut which = WhichKey { prefix, entries };
        which.sort_entries();
        Some(which)
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{HeadlessVim, Remap, RemapMode, RemapSet, VimKey, WhichKey, parse_keys};

    fn vim(remaps: &[(RemapMode, &str, &str)]) -> HeadlessVim {
        let remaps: Vec<Remap> = (remaps.iter())
            .map(|&(mode, from, to)| Remap {
                modes: vec![mode],
                from: from.into(),
                to: to.into(),
                recursive: false,
            })
            .collect();
        let mut vim = HeadlessVim::new("one two\n", (0, 0));
        let (set, issues) = RemapSet::new(&remaps, "\\");
        assert!(issues.is_empty(), "{issues:?}");
        vim.state_mut().set_remaps(Arc::new(set));
        vim
    }

    fn keys(text: &str) -> Vec<VimKey> {
        parse_keys(text).unwrap()
    }

    fn listed(which: Option<WhichKey>) -> Option<(String, Vec<String>)> {
        let which = which?;
        let keys = which.entries.into_iter().map(|entry| entry.key).collect();
        Some((which.prefix, keys))
    }

    fn some(prefix: &str, keys: &[&str]) -> Option<(String, Vec<String>)> {
        Some((prefix.into(), keys.iter().map(|&key| key.into()).collect()))
    }

    #[test]
    fn a_mapped_prefix_shadows_the_builtin_keys_after_it() {
        let n = RemapMode::Normal;
        // `g` held is the map's: `gg` would run the mapped `g` then `g`.
        let mut held = vim(&[(n, "g", "x"), (n, "gx", "dd")]);
        held.feed("g").unwrap();
        assert!(held.state().remap_waiting());
        assert_eq!(listed(held.state().which_key()), some("g", &["x"]));
        let state = held.state();
        assert_eq!(listed(state.which_key_for(&keys("g"))), some("g", &["x"]));
        // A leaf map of `g` alone: nothing goes on from it.
        let leaf = vim(&[(n, "g", "x")]);
        assert_eq!(leaf.state().which_key_for(&keys("g")), None);
        // A map under `g` without `g` mapped still lists the built-ins.
        let under = vim(&[(n, "gx", "dd")]);
        let which = listed(under.state().which_key_for(&keys("g"))).unwrap();
        assert!(which.1.contains(&"g".to_string()) && which.1.contains(&"x".to_string()));
    }

    #[test]
    fn insert_replace_and_the_command_line_list_their_maps() {
        let (i, c) = (RemapMode::Insert, RemapMode::CmdLine);
        let maps = [(i, "jk", "<Esc>"), (i, "jj", "J"), (c, "zz", "s/a/b/")];
        let mut insert = vim(&maps);
        insert.feed("ij").unwrap();
        assert!(insert.state().remap_waiting());
        assert_eq!(listed(insert.state().which_key()), some("j", &["j", "k"]));
        let mut replace = vim(&maps);
        replace.feed("Rj").unwrap();
        assert_eq!(listed(replace.state().which_key()), some("j", &["j", "k"]));
        let mut line = vim(&maps);
        line.feed(":z").unwrap();
        assert!(line.state().remap_waiting());
        assert_eq!(listed(line.state().which_key()), some("z", &["z"]));
        // A host holding the keys itself.
        let mut host = vim(&maps);
        host.feed("i").unwrap();
        let state = host.state();
        assert_eq!(
            listed(state.which_key_for(&keys("j"))),
            some("j", &["j", "k"])
        );
        // No built-in table there: `g` lists nothing in insert mode.
        assert_eq!(state.which_key_for(&keys("g")), None);
        assert_eq!(state.which_key(), None);
        // The key after `<C-r>` names a register, unmapped.
        host.feed("<C-r>").unwrap();
        assert!(host.state().insert_register_waiting());
        assert_eq!(host.state().which_key_for(&keys("j")), None);
    }
}
