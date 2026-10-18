---
task_uid: "01m55mavzad4fswrct4j8fdawe"
task: "T187"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "remap_repeat_records_typed_keys (top level, crates/fe-vim/tests/remap_repeat.rs): recording qa, a mapped H and q stores H in register a (not ^), a held prefix's keys are recorded once, and @a after the map is changed plays through the new map"
    required: true
    manual: false
  - id: "C2"
    text: "remap_repeat_dot: nnoremap X dw then X then . repeats dw; after nnoremap Y y$ and Y, . repeats the change before it"
    required: true
    manual: false
  - id: "C3"
    text: "remap_repeat_in_macros: a macro whose keys expand runs the expansion and then its own later keys in order, and an expansion's failing key stops the macro"
    required: true
    manual: false
  - id: "C4"
    text: "remap_repeat_normal_bang: :normal H runs the map and :normal! H runs built-in H; ExCommand::Normal carries remap, and ex/normal.rs's module doc no longer says ! is the same"
    required: true
    manual: false
---

# fe-vim: remaps under . , macros and :normal as nvim's: typed keys are recorded, expansions repeat as changes, and :normal! skips the maps (Mac T870)

## Problem

`docs/decisions/vim-remaps.md` (decided 2026-10-17) §5 settles how remaps meet `.`, macros
and `:normal`. This is its card 4: the typed-vs-expanded rules. The feed card runs remaps in
`VimState::feed`; this card pins down and finishes what that means for repeats.

- **Dot.** fe-vim stores committed changes, not keys, so `.` repeats the last change the
  *expanded* keys made. With `nnoremap X dw`, `X.` repeats `dw`. A yank (`nnoremap Y y$`)
  makes no change, so `.` keeps repeating the one before it.
- **Macros.** nvim records typed keys, before mapping, and plays them through the maps. So a
  recording holds the `from` keys as typed, and a replay's keys enter the remap stage like
  typed ones.
- **`:normal`.** `ex/normal.rs` feeds keys through `feed`. `:normal` should run them through
  the maps and `:normal!` should not. But `normal::parse` reads the `!` and drops it:
  `ExCommand::Normal { keys }` in `ex.rs` has no field for it. The module doc says "`!` is
  the same (fe-vim has no mappings)".

## Change

- `ExCommand::Normal` gains `remap: bool`: false for `:normal!`, true for `:normal`. Its
  `parse` keeps the bang, and `run` takes the flag.
  - With `remap: false`, every key the `Runner` feeds, and every key an expansion or a `@`
    among them queues, bypasses the remap stage. A held prefix doesn't survive the
    `:normal`: `run` resolves it, as a timeout would, before it returns.
  - With `remap: true`, keys go through the maps. An expansion runs to the end of its line,
    like the rest of the line's keys.
  - Update the module doc's `!` sentence.
- Recording: in `state.rs`, the remap stage's typed keys reach `macros.record` once each, as
  typed, including a key held for a longer match, and an expansion's keys never do. `remap.rs`
  holds the tag rules if they need to change. `macros.rs` stays untouched (T836 holds it).
- After a remap's expansion, `.` repeats whatever the expansion's last repeatable change
  left in `last_change`. No code is expected for this. Prove it with tests.

`ex.rs` is shared with the ex-table cards (T822 → T865, lent). This card depends on T865,
the last of them, and changes only `ExCommand::Normal`'s variant and where it is matched:
`ex.rs`'s run arm, `crates/fe-gpui/src/view/host.rs` (`ExCommand::Normal { keys, .. }`) and
the variant's two constructions in `crates/fe-vim/tests/ex_global.rs`.
Tests go in the new top-level `crates/fe-vim/tests/remap_repeat.rs`.
