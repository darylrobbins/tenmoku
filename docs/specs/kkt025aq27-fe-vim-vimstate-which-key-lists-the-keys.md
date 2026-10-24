---
task_uid: "01m5qphyws4zfrcfkkt025aq27"
task: "T224"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test which_key_lists_builtin_prefixes (top level, crates/fe-vim/tests/which_key.rs): after g, z, Z, [ and ] in normal and visual mode, which_key() names the prefix and lists the keys the reducer takes there (g: g, e, E, j, k, s, v, ...), each with a non-empty title"
    required: true
    manual: false
  - id: "C2"
    text: "which_key_lists_remap_continuations: with nnoremap \u003cleader\u003eff, \u003cleader\u003efr and \u003cleader\u003ew typed up to \u003cleader\u003e, which_key() lists f (a group of 2) and w (labelled by its fe: command id or keys); after \u003cleader\u003ef it lists f and r; a user map wins over a built-in title for the same key"
    required: true
    manual: false
  - id: "C3"
    text: "which_key_is_none_without_continuations: None with nothing pending, after a count or register alone, in operator-pending (d, y, c), and after a complete leaf map"
    required: true
    manual: false
  - id: "C4"
    text: "which_key_table_keys_are_real: every key in the built-in table is accepted by the reducer through HeadlessVim (not Cancel) after its prefix"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim and cargo clippy -p fe-vim --all-targets pass; no existing test file is edited"
    required: true
    manual: false
---

# fe-vim: VimState::which_key lists the keys that continue a pending prefix (g z Z [ ], a remap's leader or other prefix) with their titles (Mac T1134)

## Problem

The user asked on 2026-10-24 for a generic which-key popup in vim mode: after a prefix with continuations (`<leader>`, `g`, `z`, `[`, `]`, `<C-w>`, any user remap prefix) show the next keys with their titles. This card is the model only: which continuations exist after a prefix, and what to call each. The popup and its delay are the next card. The model is a function of the state, so one popup serves every preset and every user remap.

## Current state

- `PendingKind` (`crates/fe-vim/src/reducer.rs:84`) already says "for `vim.pending_kind` and which-key", and `Pending::kind()` (reducer.rs:190) names what waits: `G`, `Z`, `BigZ`, `Bracket`. Nothing lists the keys each takes; they are spread over `step` and the features' hooks (`crates/fe-vim/src/feature.rs:133-141`).
- `RemapSet::lookup` (`crates/fe-vim/src/remap.rs:136`) answers Prefix / Exact / ExactAndPrefix for typed keys but cannot enumerate the longer maps. `VimState::remap_waiting` (`crates/fe-vim/src/state.rs:684`) says keys are held, not which.
- A map's right side `<Cmd>fe:{id}<CR>` names a fastedit command (`remap.rs:66`), the title of which only the host's catalog knows.

## Change

- New `crates/fe-vim/src/which_key.rs` (declared in `lib.rs`): `VimState::which_key(&self) -> Option<WhichKey>`, `WhichKey { prefix: String, entries: Vec<WhichKeyEntry> }`, `WhichKeyEntry { key: String, label: WhichKeyLabel, group: bool }` with `WhichKeyLabel::{Builtin(&'static str), Command(String) /* fe: id, host resolves the title */, Keys(String) /* the right side as written */, Group(usize) /* n continuations */}`. Entries sorted by key (lower before upper as which-key.nvim does).
- Sources: (a) while the reducer waits on `g`, `z`, `Z`, `[` or `]` (normal and visual), a static table of that prefix's built-in keys with short titles (`gg` "first line", `zt` "scroll top"...), built from what the reducer and the features take today; (b) while the remap engine holds keys, or the typed keys so far in `which_key_for(&[VimKey])` for a host that holds them itself, every longer map of the mode's `RemapSet`, grouped by the next key, a user map winning over a built-in title for the same key; a key that is both a map and a prefix has the map's label and `group: true`.
- `RemapSet` (remap.rs) gains `continuations(mode, keys) -> Vec<(VimKey, &Remap, bool /*longer maps exist*/)>`; `VimState` (state.rs) exposes the held keys.
- `None` when nothing waits, when only a count or register waits, in operator-pending (out of scope: `d`, `y`, `c` show no popup), or when the prefix has no continuation.
- Not here: popup, delay, settings (next card); keymap chords such as `] d` and `<C-w>` (a later card, they live in the keymap matcher).

## Tests

New `crates/fe-vim/tests/which_key.rs`: built-in `g`, `z`, `[`, `]`; remaps with `<leader>` (nested `<leader>f` group); exact-and-prefix; no popup for counts, operators and leaf maps; and a drift test feeding every table key through `HeadlessVim` to prove the reducer takes it (a stale table entry fails). Behaviour reference: which-key.nvim's trigger/continuation model; no nvim oracle.

## Implementation notes

- The built-in table (`which_key::BUILTIN_KEYS`, public for the drift test) lists every key the reducer and features take after each prefix in normal or a visual mode; at run time an entry lists only where the reducer takes it now, by running `step` on the pending value (`g&` and `gi` not in visual mode, `g<C-a>` only there, `gr` only with ReplaceWithRegister on). The drift test checks both ways: every table key is taken, and every key taken (printable ASCII, ctrl letters, named keys) has a title.
- `RemapSet::continuations` returns `Continuation { key, map: Option<&Remap>, command: Option<&CommandId>, longer: usize }` rather than the spec's `(VimKey, &Remap, bool)`: a pure group key has no map of its own, and `Group(n)` needs the count. `n` counts the longer maps under the key. The set keeps each `Remap` as written in place of its left side's text.
- A held single key that leaves the reducer waiting on a built-in prefix (`g` held because `gx` is mapped) lists that prefix's built-in keys with the maps; a map's key replaces the built-in entry for the same key.
- `VimState::remap_held` exposes the held keys; `Pending::which_key_prefix` (reducer.rs, out of the listed scope) says which of `g z Z [ ]` waits, `[` and `]` being one `PendingKind`. `vimrc::notation` became `pub(crate)` for the keys' names.
- Carried T205 F1 and F2 fixed in remap.rs (`map_mode` reads the key after `<C-r>` unmapped; `hides` is false for insert and cmdline maps first), with `tests/remap_insert_register.rs`.
