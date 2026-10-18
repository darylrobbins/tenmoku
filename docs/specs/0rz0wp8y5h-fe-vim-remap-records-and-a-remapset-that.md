---
task_uid: "01m55masjqcvhwd90rz0wp8y5h"
task: "T185"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "remap_set_looks_up_by_mode (top level, crates/fe-vim/tests/remap_set.rs): lookup returns None, Prefix, Exact and ExactAndPrefix as §4 rule 3 defines them, per mode (an n map isn't found in o or x), and a later entry with the same mode and from replaces an earlier one"
    required: true
    manual: false
  - id: "C2"
    text: "remap_set_resolves_notation: \u003cleader\u003e (default \\, and \u003cSpace\u003e as a leader) is replaced in from and to, \u003cNop\u003e maps to no keys, \u003cSpace\u003e \u003cBar\u003e \u003cBslash\u003e parse as ' ', '|' and '\\' (parse_keys too), and \u003cC-W\u003e equals \u003cC-w\u003e"
    required: true
    manual: false
  - id: "C3"
    text: "remap_set_reports_bad_entries: an empty from, \u003cNop\u003e or \u003cCmd\u003e in from, an unknown \u003cname\u003e on either side, \u003cPlug\u003e \u003cSID\u003e \u003cSNR\u003e \u003cLocalLeader\u003e, and \u003cCmd\u003e in to each give one RemapIssue with that entry's index, and only that entry is left out"
    required: true
    manual: false
  - id: "C4"
    text: "Only remap.rs, key.rs, lib.rs (mod remap; and its pub use) and the new test file change; feature.rs, args.rs, reducer.rs and state.rs are untouched"
    required: true
    manual: false
---

# fe-vim: remap records and a RemapSet that looks up n, x and o left sides, with <leader>, <Nop>, <Space>, <Bar> and <Bslash> (Mac T868)

## Problem

fe-vim has no key-to-key remaps (`nnoremap Y y$`, `nnoremap <leader>w :w<CR>`). The design
is `docs/decisions/vim-remaps.md`, decided by the user on 2026-10-17. Its first cut covers
modes n, x and o, with recursive maps limited to a depth of 100 and `<leader>` defaulting
to `\`. This card is that record's card 1, static half: the records, the set built from
them, the key notation they need, and the lookup the engine (next card) matches against.
Nothing runs a remap yet.

Today `parse_keys` (`crates/fe-vim/src/key.rs`) reads plain characters, `<Esc> <CR> <Enter>
<Return> <BS> <Tab> <lt> <Left> <Right> <Up> <Down>` and `<C-x>`. It has no `<Space>`,
`<Bar>` or `<Bslash>`. `VimKey` holds no other keys.

## Change

New `crates/fe-vim/src/remap.rs`, declared in `lib.rs` as `mod remap;` and exported with
`pub use remap::{…}`. It touches no other fe-vim file except `key.rs`.

- `pub enum RemapMode { Normal, Visual, OperatorPending }`: nvim's `n`, `x` and `o`.
- `pub struct Remap { pub modes: Vec<RemapMode>, pub from: String, pub to: String, pub
  recursive: bool }`: one map as written, in vim notation.
- `pub struct RemapIssue { pub index: usize, pub message: String }`: an entry that can't
  load, by its index in the input.
- `RemapSet::new(remaps: &[Remap], leader: &str) -> (RemapSet, Vec<RemapIssue>)`. It is
  `Clone + Debug + Default + PartialEq`, and an empty set maps nothing.
  - `<leader>` (any case) in `from` or `to` is replaced by `leader`'s keys. `leader` is
    itself in vim notation; `"\\"` is nvim's default and `"<Space>"` works too.
  - `<Nop>` as the whole of `to` maps to no keys.
  - For each mode, a later entry with the same `from` replaces an earlier one, as
    redefining does in nvim.
  - Each of these is an issue that leaves only its own entry out: an empty `from`; a
    `from` that is `<Nop>` or holds `<Cmd>`; an unknown `<name>` on either side (message
    `unknown key <F13>`); `<Plug>`, `<SID>`, `<SNR>` and `<LocalLeader>` ("not supported").
    `<Cmd>` in `to` is "not supported yet" until the `<Cmd>` card.
- `RemapSet::lookup(&self, mode: RemapMode, keys: &[VimKey]) -> Lookup`, where `Lookup` is
  one of: `None`; `Prefix` (a longer map starts with `keys`); `Exact { to: &[VimKey],
  recursive: bool }`; or `ExactAndPrefix { to, recursive }`. These are the record's §4
  rule 3 states.
- `RemapSet::is_empty`, and `RemapSet::hides_builtin(mode) -> Vec<String>`. The latter
  lists the `from`s whose first key starts a built-in multi-key command (`g`, `z`, `Z`,
  `[`, `]`, `"`, an operator, `f t F T r m ' \``, `q`, `@`) and that are complete maps
  shorter than it (§4 rule 4), so a host can note them.

`key.rs`: `named` also reads `<Space>` (`Char(' ')`), `<Bar>` (`Char('|')`) and `<Bslash>`
(`Char('\\')`), case-insensitively like the others. `<Nop>` and `<leader>` are not keys:
`remap.rs` handles them before calling `parse_keys`.

Tests go in the new top-level `crates/fe-vim/tests/remap_set.rs`, through the public API
only.
