---
task_uid: "01m53q0w7mzb0hwaz7ze1nxwsb"
task: "T122"
rev: 3
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "Workbench::on_run_command has no per-family branch: kiln::commands::dispatch_typed, the remaining panels' propagation, then cx.propagate(); command_catalog() and command_list() list each remaining fe_command registry once and no fe-workbench family that moved"
    required: true
    manual: false
  - id: "C2"
    text: "command_catalog_tests.rs: every command ID bound in fe-keymap's default keymaps or named in its default menus, on macOS and on Linux, checks OK against command_catalog() with no app, and the palette in a test window lists every command ID it listed at the base (a fixed list in the test)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Build fastedit's command catalog from kiln's recorded commands plus the registries left, and cut Workbench::on_run_command to the typed bridge and propagation (Mac T705)

Filed on argon from Mac T705 (card ~/coordination/argon-cards/zo/T705.json rev 2; note zo, the kiln phase-3 chain, card 16 of 17, serial). Mac task uid 01m4vbjybc0tq8s3abz5h68zpa. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T121 (Mac T704), the card before. Title the host commit "... (Mac T705)".

## Problem

After T693-T704 the workbench's commands are typed, but `command_catalog()` and `command_list()`
(`crates/fe-workbench/src/lib.rs`) still list the remaining registries by hand, and `Workbench::on_run_command`
(`crates/fe-workbench/src/workbench.rs` ~2512) keeps whatever of its 15-arm chain the family cards left (the
`propagate` fallthrough, any arm a family kept for a non-command caller).

## Change

- `command_catalog()` is `kiln::recorded_catalog("fe_workbench")` (T700, T693) followed by the `fe_command`
  registries that still exist: kiln-shell's picker, menu bar and palette catalogs, the search
  panel's and project panel's own-context registries, and `fe_gpui::commands::command_catalog()`. Each remains
  only if its registry does; no fe-workbench family appears twice.
- `command_list()` holds only what the palette doesn't already get from `typed_commands`: the same remaining
  registries' commands and the editor's.
- `on_run_command` is `dispatch_typed`, then the editor's and remaining panels' propagation, then `cx.propagate()`:
  no per-family `if`. `commands_run` (test-only) still records every dispatch.
- New tests in `crates/fe-workbench/src/command_catalog_tests.rs`.

## Rules for every card of this chain

Phase 3 of the Kiln app layer (`docs/decisions/kiln-app-layer.md`, approved 2026-10-13). The chain runs serially,
in this order: T700 (kiln), T690, T691, T693, T694, T695, T697, T698, T699, T701, T702, T703, T692, T696, T704,
T705, T706.

- No command ID or key context is renamed (`app-framework.md` §4): every moved command keeps its ID with
  `id = ".."` and its context with `context = ".."`.
- No existing fastedit test is edited; behaviour is proven by the existing suites plus the new test file each
  card names.
- Nothing on the paint path changes: `EditorElement`, `TerminalElement` and `FileViewerElement` stay raw
  `impl Element`.
- The keymap and menus stay TOML tables (`crates/fe-keymap/keymaps/*.toml`, `menus/*.toml`), untouched.
- `crates/fastedit/src/main.rs` is not touched: `fe_workbench::init` registers what the chain adds (neon's T707
  owns `main.rs` and the keymap files).

## Implementation notes

- At the base, the family cards had already cut `Workbench::on_run_command` to `kiln::commands::dispatch_typed` and
  then `cx.propagate()`. The panels take their own `RunCommand`s themselves, and `commands_run` records the
  propagated runs there and the typed runs through `Workbench::ran`. `command_catalog()` was already
  `kiln::recorded_catalog("fe_workbench")` followed by the registries. So `workbench.rs` is unchanged.
  T171 F2 (inlay hints) is not addressed: its fix and test belong in `lsp_inlay_hints.rs` and
  `tests/lsp_inlay_hints.rs`, outside this card's scope.
- The registries left: kiln-shell's picker, menu bar and palette catalogs, the search panel's, the Changes
  panel's commit box key, the project panel's, the vim command line's cancel (`status_bar`), and the editor's.
- The project panel's registry still names the typed `project_panel.reveal_active`, for the panel to pass on.
  `tests/tree_keys.rs` asserts that, so the entry stays. `command_list()` now drops registry entries that are
  workbench typed commands, so the palette lists it as typed, with the same title and `workbench` context.
  `command_catalog()` checks the recorded catalog first, so the typed command answers for it.
- `command_catalog_tests.rs` keeps the palette's base IDs as fixed lists, for the editor focused and for the
  project panel focused. It checks that each is still listed (a superset check), so a later card that adds a
  command doesn't have to edit it.
