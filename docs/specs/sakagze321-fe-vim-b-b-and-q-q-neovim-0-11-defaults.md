---
task_uid: "01m5qycj7gev7bzcsakagze321"
task: "T332"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Integration test vim_bracket_pairs_resolve_in_normal_mode (top level, crates/fe-keymap/tests/vim_bracket_pairs.rs): ] b, [ b, ] q and [ q resolve in editor \u0026\u0026 vim.mode == normal \u0026\u0026 !vim.pending to workbench.next_tab, workbench.previous_tab, diagnostics.next and diagnostics.previous"
    required: true
    manual: false
  - id: "C2"
    text: "vim_bracket_pairs_stay_out_of_other_modes: they resolve to nothing with vim off, in insert and visual mode, with an operator pending and outside an editor context"
    required: true
    manual: false
  - id: "C3"
    text: "The default keymap still has no duplicate bindings, and every command named exists in the registry (fe-gpui command_tests pass with the binding count updated by exactly four)"
    required: true
    manual: false
  - id: "C4"
    text: "The comment above the new bindings says the Problems list stands in for quickfix, and the ]d/[d bindings are unchanged"
    required: true
    manual: false
---

# fe-vim: [b ]b and [q ]q (Neovim 0.11 defaults) go to the previous and next tab and Problems entry (Mac T1163)

## Problem

The user asked on 2026-10-24 for the unimpaired-style pairs beyond `[d ]d`. Neovim 0.11 ships `[b ]b` (previous/next buffer) and `[q ]q` (previous/next quickfix entry) as defaults, and LazyVim keeps them. fastedit has the matching commands but no keys, so `]b` cancels.

## Current state

- `]d`/`[d` are keymap chords in `crates/fe-keymap/keymaps/fastedit.toml:920-930`, context `editor && vim.mode == normal && !vim.pending`, to `diagnostics.next`/`previous`, which step through the Problems list across files (`ProblemsOp::Next`, `crates/fe-workbench/src/workbench_commands.rs:560`). The tab commands are `workbench.next_tab`/`workbench.previous_tab` (same file, :43,:49). fastedit has no separate quickfix list: the Problems list is the nearest thing, and the Search and References panels' selection commands are panel-scoped (`search.select_next`, `references_panel.select_next`).
- `crates/fe-gpui/src/command_tests.rs` counts the default keymap's bindings, so adding bindings changes it (as the multi-cursor card did); `crates/fe-keymap/tests/vim_multi_cursor_keys.rs` is the model for a keymap resolution test.
- A parallel card track also edits `fastedit.toml`'s panel sections: touch only the vim pairs block under the `]d` bindings.

## Change

In `fastedit.toml`, same context as `]d`: `] b` -> `workbench.next_tab`, `[ b` -> `workbench.previous_tab`, `] q` -> `diagnostics.next`, `[ q` -> `diagnostics.previous` (the Problems list stands in for quickfix; say so in the comment above them). Update the binding count in `command_tests.rs`. `[B`, `]B`, `[Q`, `]Q` (first/last) are out: no first/last commands exist. With the which-key popup (T1141) these chords list themselves.

## Tests

New `crates/fe-keymap/tests/vim_bracket_pairs.rs`: each of the four resolves in an editor in vim normal mode with nothing pending to its command, and resolves to nothing with vim off, in insert mode, with a pending operator, or in a non-editor context; no duplicate bindings. The updated `command_tests.rs` binding count keeps the keymap and the registry in step (dispatch is covered there).

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y58), not truncated.
