---
task_uid: "01m55mayk675ptvtbvd4v063dc"
task: "T189"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "remaps_match_neovim (top level, crates/fe-vim/tests/remaps_oracle.rs) passes on tests/corpus/remaps.toml, at least 24 cases recorded with nvim 0.12.x, among them (asserted by name) remap_Y, remap_H, remap_count_H, remap_dH_onoremap, remap_dH_nnoremap_only, remap_visual_x, remap_dot, remap_macro_record_play, remap_recursive, remap_noremap, remap_self_prefix, remap_self_prefix_more, remap_failed_prefix_suffix_mapped, remap_cycle_e223, remap_leader, remap_normal, remap_normal_bang, remap_unmap, remap_ex_line_in_to"
    required: true
    manual: false
  - id: "C2"
    text: "oracle.lua puts back every map a case changed (nvim's default Y map included), so a case's result doesn't depend on the cases before it; the header comment documents the mapping field"
    required: true
    manual: false
  - id: "C3"
    text: "Only remaps.expected.json is re-recorded; every other tests/corpus/*.expected.json is byte-identical to the base"
    required: true
    manual: false
---

# fe-vim: record remap cases against nvim with a mapping field on corpus cases, run through the vimrc parser on both sides (Mac T872)

## Problem

Every M7 vim card is checked against recorded nvim. `docs/decisions/vim-remaps.md` §8
(decided 2026-10-17) adds an optional `mapping` field to a corpus case. Its lines run in nvim
before the keys. On the fe-vim side they are parsed by the same vimrc parser into a
`RemapSet`, so the parser is under test on both sides. This is the record's card 5. It
comes after the engine (feed and repeat cards) and the vimrc parser.

Facts about the recorder (`crates/fe-vim/tests/support/oracle.lua`, driven by the ignored
`record_corpus_with_neovim` in `tests/oracle.rs`):

- It runs one nvim process per corpus file, every case in one session. It resets buffers
  and registers between cases, but not mappings.
- An error flushes typeahead, so a failing command (the E223 cycle) must be a case's last
  key.
- It can't hold a case that ends in insert mode. It can't wait out 'timeoutlen' either, so
  a case must not end on an ambiguous prefix. Keys fed with no delay resolve as "no
  timeout"; timeouts stay unit tests (feed card).
- nvim `--clean` has default maps, e.g. `Y` → `y$`. A case that maps `Y` shadows one.

## Change

- `tests/support/mod.rs`: `Case` gains `#[serde(default, skip_serializing_if =
  "Vec::is_empty")] mapping: Vec<String>`. `check` builds the case's `HeadlessVim`, then
  `parse_vimrc(&mapping.join("\n"))`. It fails the case on any issue, then calls
  `set_remaps(RemapSet::new(&remaps, "\\"))` before feeding the keys.
- `tests/support/oracle.lua` runs each `mapping` line with `vim.cmd` before the keys. After
  the case it puts back every map the case changed, using `vim.fn.maparg(lhs, mode, false,
  true)` saved before and `vim.fn.mapset` or `unmap` after. So the next case starts from
  nvim's defaults (the `Y` default included). Update the header comment.
- New `tests/corpus/remaps.toml`, recorded with nvim 0.12.x into
  `tests/corpus/remaps.expected.json`, and the new top-level `tests/remaps_oracle.rs`, with
  `remaps_match_neovim` in the shape of `macros_oracle.rs`.
- Fix in `src/remap.rs`, `src/state.rs` or `src/vimrc.rs` any divergence the recording
  shows. Record nvim's answer for the self-prefix rule (`nmap l ll`, `nmap ab abx`) and
  the cycle in a short "nvim facts" comment at the top of `remaps.toml`.
- Re-record only `remaps.toml`. Every other `*.expected.json` stays unchanged.

## Implementation notes

- The recording showed no divergence in `remap.rs`, `state.rs` or `vimrc.rs`: all 42 cases
  in `remaps.toml` match nvim 0.12.4 as the engine stands.
- `oracle.lua` saves `maplist()`'s global maps (and `g:mapleader`) before every case, not
  only those with a `mapping`, and puts them back after it. It deletes each map that isn't
  one it saved, in each of its modes, then calls `mapset` for each one missing, and asserts
  the result is exactly what it saved (callbacks, script IDs and line numbers included). A
  re-record left every other `*.expected.json` byte-identical.
- fe-vim has no built-in `Y`: nvim's `Y` → `y$` is a default map, and fe-vim forwards an
  unmapped `Y`. So no case types `Y` without its own map. `remap_H_restored` and
  `remap_leader_restored` show the restore through the gate instead.
