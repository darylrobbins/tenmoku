//! Key-to-key remaps as written (`Remap`), and the set built from them
//! (`RemapSet`) that the engine matches typed keys against. See
//! `docs/decisions/vim-remaps.md`; and the engine that runs them in front
//! of the reducer (`RemapState`, §4 and §5), which [`VimState::feed`]
//! passes each key through.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::key::{VimKey, parse_keys};
use crate::macros::REPLAY_CAP;
use crate::{Mode, Pending, PendingKind, Step, VimState};

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

    /// The `from`s, as written, that are the start of a longer built-in
    /// command in `mode`, so the map hides that command (§4 rule 4).
    pub fn hides_builtin(&self, mode: RemapMode) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.mode == mode && hides(mode, &e.from))
            .map(|e| e.text.clone())
            .collect()
    }
}

/// Whether a map of `from` in `mode` is complete where a built-in goes on.
fn hides(mode: RemapMode, from: &[VimKey]) -> bool {
    const PREFIXES: &str = "gzZ[]\"fFtTrm'`q@";
    const OPERATORS: &str = "cdy<>!=";
    match from {
        [VimKey::Char(c)] => match mode {
            RemapMode::Normal => PREFIXES.contains(*c) || OPERATORS.contains(*c),
            RemapMode::Visual => PREFIXES.contains(*c),
            RemapMode::OperatorPending => "gz[]'`fFtTia".contains(*c),
        },
        // A jump's trigger, alone or with its first char, in every mode:
        // `gs` and `gsa` hide `gs{c1}{c2}`.
        [VimKey::Char('g'), VimKey::Char('s' | 'S')]
        | [VimKey::Char('g'), VimKey::Char('s' | 'S'), VimKey::Char(_)] => true,
        // Two-key operators: `gU` hides `gUU` and `gUw`.
        [VimKey::Char(a), VimKey::Char(b)] => {
            mode == RemapMode::Normal
                && matches!(
                    (a, b),
                    ('g', 'u' | 'U' | '~' | '?' | 'q' | 'w' | '@') | ('z', 'f')
                )
        }
        _ => false,
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

/// The most maps that run with no key between them that wasn't a map's,
/// before the engine stops with E223 (nvim's 'maxmapdepth' is 1000).
const MAX_DEPTH: u32 = 100;

/// Where a key the engine reads came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// Typed, or `replayed` by a `@`: the macros have counted it.
    Typed { replayed: bool },
    /// A map's right side: not recorded, and not a replay's.
    Expansion,
}

impl Origin {
    /// Whether a recording gets it: typed, not a replay's or a map's.
    pub(crate) fn recorded(self) -> bool {
        self == Origin::Typed { replayed: false }
    }
}

/// What the engine knows of a key it handed the host to feed back.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tag {
    /// `None` until the macros count it: a typed key or a `@`'s.
    pub(crate) origin: Option<Origin>,
    /// It may be matched against the maps.
    pub(crate) remap: bool,
}

/// A key the host feeds without a tag (typed), or a `@`'s.
pub(crate) const TYPED: Tag = Tag {
    origin: None,
    remap: true,
};

/// The engine: the set, the keys held for a longer match, and the keys it
/// resolved for the host to feed ([`VimState::take_replay`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct RemapState {
    set: Arc<RemapSet>,
    /// Keys read for a longer match, in order, and the map mode they're
    /// matched in.
    held: Vec<(VimKey, Origin)>,
    mode: Option<RemapMode>,
    /// Keys resolved and not yet taken by the host, in order.
    queued: Vec<(VimKey, Tag)>,
    /// The tags of the keys the host took and hasn't fed back, in the
    /// order it feeds them.
    taken: VecDeque<Tag>,
    /// Keys skip the maps, as under `:normal!`.
    pub(crate) off: bool,
    /// Maps run since a key that wasn't a map's.
    depth: u32,
    /// Keys handed out since a key was typed or the host fed all it took,
    /// for [`REPLAY_CAP`]: a `@` a map runs starts the macros' own count
    /// again.
    handed: usize,
}

/// What the remap stage left [`VimState::feed`] to do.
pub(crate) enum Staged {
    /// Read this key as typed: the one fed, or the first of those held.
    Key(VimKey, Origin),
    /// The key was taken: return this.
    Done(Step),
}

impl RemapState {
    pub(crate) fn set(&self) -> &RemapSet {
        &self.set
    }

    /// Whether keys are held for a longer match.
    pub(crate) fn waiting(&self) -> bool {
        !self.held.is_empty()
    }

    /// The tag of the key the host feeds now, or `None` if it isn't one
    /// handed out, but typed.
    pub(crate) fn next_tag(&mut self) -> Option<Tag> {
        let tag = self.taken.pop_front();
        if tag.is_none() {
            self.handed = 0;
        }
        tag
    }

    /// Whether the keys handed out since a key was typed are past
    /// [`REPLAY_CAP`], as a `@` and a map that plays it again would go on.
    pub(crate) fn over_cap(&self) -> bool {
        self.handed > REPLAY_CAP
    }

    /// A key that wasn't a map's was read: maps may run again.
    pub(crate) fn read(&mut self, origin: Origin) {
        if origin != Origin::Expansion {
            self.depth = 0;
        }
    }

    /// Hands out `replay`, a `@`'s keys, and then the keys resolved, which
    /// the host feeds in that order before what it holds; their tags go in
    /// front of the tags taken before.
    pub(crate) fn take(&mut self, mut replay: Vec<VimKey>) -> Vec<VimKey> {
        let mut tags = vec![TYPED; replay.len()];
        for (key, tag) in self.queued.drain(..) {
            replay.push(key);
            tags.push(tag);
        }
        // Nothing handed out and none left to feed: what was is over.
        self.handed = match replay.is_empty() && self.taken.is_empty() {
            true => 0,
            false => self.handed.saturating_add(replay.len()),
        };
        for tag in tags.into_iter().rev() {
            self.taken.push_front(tag);
        }
        replay
    }

    /// The host dropped what it held of a replay or an expansion.
    pub(crate) fn stop(&mut self) {
        self.queued.clear();
        self.taken.clear();
        self.depth = 0;
        self.handed = 0;
    }

    /// Drops the keys held, resolved and taken.
    pub(crate) fn held_clear(&mut self) {
        self.held.clear();
        self.mode = None;
        self.stop();
    }

    /// Sets aside the keys held, resolved and taken, and the counters, for
    /// a `:normal` to have a stream of its own; [`RemapState::restore`]
    /// puts them back. The set stays.
    pub(crate) fn save(&mut self) -> RemapState {
        let set = Arc::clone(&self.set);
        let off = self.off;
        let saved = std::mem::take(self);
        self.set = set;
        self.off = off;
        saved
    }

    /// Puts back what [`RemapState::save`] set aside, dropping what the
    /// `:normal` left, and keeping the set it ends with.
    pub(crate) fn restore(&mut self, saved: RemapState) {
        let (set, off) = (Arc::clone(&self.set), self.off);
        *self = saved;
        self.set = set;
        self.off = off;
    }

    /// Replaces the set: keys held for a longer match go on as typed,
    /// unmapped.
    pub(crate) fn replace(&mut self, set: Arc<RemapSet>) {
        self.set = set;
        self.mode = None;
        for (key, origin) in self.held.drain(..) {
            let tag = Tag {
                origin: Some(origin),
                remap: false,
            };
            self.queued.push((key, tag));
        }
    }

    fn queue(&mut self, keys: impl IntoIterator<Item = (VimKey, Origin)>, remap: bool) {
        self.queued.extend(keys.into_iter().map(|(key, origin)| {
            let tag = Tag {
                origin: Some(origin),
                remap,
            };
            (key, tag)
        }));
    }
}

/// The map mode `key` is matched in, if it's matched at all: Normal or
/// Visual, with nothing pending or only a count, a register or an
/// operator; not the key after a prefix (`f`, `"`, `g`, ...), which nvim
/// reads unmapped, nor a `0` that goes on a count.
fn map_mode(state: &VimState, key: VimKey) -> Option<RemapMode> {
    if state.remap.set.is_empty() || state.pending.has_prefix() {
        return None;
    }
    if key == VimKey::Char('0') && state.pending.kind() == Some(PendingKind::Count) {
        return None;
    }
    match state.mode {
        Mode::Normal if state.pending.has_operator() => Some(RemapMode::OperatorPending),
        Mode::Normal => Some(RemapMode::Normal),
        Mode::Visual(_) => Some(RemapMode::Visual),
        _ => None,
    }
}

/// The remap stage of [`VimState::feed`] for `key`, which came from
/// `origin` and may be matched if `remap` (§4 rule 3).
pub(crate) fn stage(state: &mut VimState, key: VimKey, origin: Origin, remap: bool) -> Staged {
    state.remap.read(origin);
    if !state.remap.queued.is_empty() {
        // Keys a timeout or a new set resolved wait for the host: this
        // one goes after them.
        let tag = Tag {
            origin: Some(origin),
            remap,
        };
        state.remap.queued.push((key, tag));
        return Staged::Done(Step::Wait(state.pending.clone()));
    }
    let mode = match remap.then(|| map_mode(state, key)).flatten() {
        Some(mode) if state.remap.mode.is_none_or(|held| held == mode) => mode,
        _ if state.remap.held.is_empty() => return Staged::Key(key, origin),
        // It breaks the match, and goes on after what it resolves to.
        _ => {
            let staged = resolve(state, true);
            if !state.failed {
                let tag = Tag {
                    origin: Some(origin),
                    remap,
                };
                state.remap.queued.push((key, tag));
            }
            return staged;
        }
    };
    state.remap.held.push((key, origin));
    state.remap.mode = Some(mode);
    let keys: Vec<VimKey> = state.remap.held.iter().map(|&(key, _)| key).collect();
    match state.remap.set.lookup(mode, &keys) {
        Lookup::Prefix | Lookup::ExactAndPrefix { .. } => {
            Staged::Done(Step::Wait(state.pending.clone()))
        }
        Lookup::Exact { .. } => {
            let held = std::mem::take(&mut state.remap.held);
            state.remap.mode = None;
            Staged::Done(run(state, mode, &held))
        }
        Lookup::None if keys.len() == 1 => {
            state.remap.held.clear();
            state.remap.mode = None;
            Staged::Key(key, origin)
        }
        Lookup::None => resolve(state, true),
    }
}

/// `VimState::remap_timeout`: the held keys resolve as if a key broke
/// their match, all of them to the queue.
pub(crate) fn timeout(state: &mut VimState) {
    if !state.remap.held.is_empty() {
        resolve(state, false);
    }
}

/// Resolves the held keys: the longest complete map among them runs, and
/// the keys after it are queued to be matched again from the start; with
/// none complete, the first key goes on as typed, and the rest are queued
/// to be matched in the mode it leaves. The first goes on at once, if
/// `inline` and it was typed: queued, a replay's or a map's key that fails
/// is the host's to stop the rest at, as a typed key's isn't.
fn resolve(state: &mut VimState, inline: bool) -> Staged {
    let held = std::mem::take(&mut state.remap.held);
    let Some(mode) = state.remap.mode.take() else {
        unreachable!("held keys have a mode")
    };
    let keys: Vec<VimKey> = held.iter().map(|&(key, _)| key).collect();
    let complete = (1..=keys.len()).rev().find(|&n| {
        matches!(
            state.remap.set.lookup(mode, &keys[..n]),
            Lookup::Exact { .. } | Lookup::ExactAndPrefix { .. }
        )
    });
    match complete {
        Some(n) => {
            let step = run(state, mode, &held[..n]);
            if !state.failed {
                state.remap.queue(held[n..].iter().copied(), true);
            }
            Staged::Done(step)
        }
        None if inline && held[0].1.recorded() => {
            state.remap.queue(held[1..].iter().copied(), true);
            let (key, origin) = held[0];
            Staged::Key(key, origin)
        }
        None => {
            state.remap.queue(held[..1].iter().copied(), false);
            state.remap.queue(held[1..].iter().copied(), true);
            Staged::Done(Step::Wait(state.pending.clone()))
        }
    }
}

/// Runs the map of `from`'s keys in `mode`: they're recorded, as typed,
/// and its right side is queued, matched again if it's recursive (but for
/// a first key that starts the left side). A map after [`MAX_DEPTH`] in a
/// row fails with E223 instead, dropping the queued keys.
fn run(state: &mut VimState, mode: RemapMode, from: &[(VimKey, Origin)]) -> Step {
    if state.remap.depth >= MAX_DEPTH {
        state.remap.queued.clear();
        state.remap.depth = 0;
        state.pending = Pending::new();
        state.fail("E223: recursive mapping");
        return Step::Cancel;
    }
    state.remap.depth += 1;
    if state.pending.kind().is_none() {
        state.macros.command_start();
    }
    for &(key, origin) in from {
        if origin.recorded() {
            state.macros.record(key);
        }
    }
    let set = Arc::clone(&state.remap.set);
    let keys: Vec<VimKey> = from.iter().map(|&(key, _)| key).collect();
    let (Lookup::Exact { to, recursive } | Lookup::ExactAndPrefix { to, recursive }) =
        set.lookup(mode, &keys)
    else {
        unreachable!("only a complete map runs")
    };
    let exempt = recursive && to.starts_with(&keys);
    for (i, &key) in to.iter().enumerate() {
        let tag = Tag {
            origin: Some(Origin::Expansion),
            remap: recursive && !(exempt && i == 0),
        };
        state.remap.queued.push((key, tag));
    }
    Step::Wait(state.pending.clone())
}
