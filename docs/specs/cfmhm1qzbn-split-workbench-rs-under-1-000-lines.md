---
task_uid: "01m53q0wdsrbyh3ecfmhm1qzbn"
task: "T123"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "wc -l crates/fe-workbench/src/workbench.rs is under 1,000; workbench_root.rs, workbench_files.rs, workbench_palettes.rs and workbench_chrome.rs hold the functions the spec lists, each as an impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "the moved functions keep their names and visibility and no new pub or pub(crate) item is added, and cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Split workbench.rs under 1,000 lines into root, file, palette and chrome modules of the workbench (Mac T706)

Filed on argon from Mac T706 (card ~/coordination/argon-cards/zo/T706.json rev 3; note zo, the kiln phase-3 chain, card 17 of 17, serial). Mac task uid 01m4vbjz0t7qk4vgsb0a7k6bb1. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T122 (Mac T705), the card before. Title the host commit "... (Mac T706)".

## Problem

`crates/fe-workbench/src/workbench.rs` was 2,232 lines when the design record set "under ~900"; it is 2,996 on
e776ee0e (Save As, file moves, hot exit). After T690-T705 remove the pane glue, the layout code and the command
chain it is still well over 1,500. The record's idea of reaching Lsp, Git, DiskSync, Trust and Nav through
`cx.service::<T>()` doesn't fit them: `kiln::provide` stores one value per app (`kiln/crates/kiln/src/model.rs`
~173), and fastedit's are per window, following each window's root (`lsp`, `git` and `trust` are replaced by
`set_root`). That part is left to the lead (see the hand-back); this card does the split.

## Change

Move code out of `workbench.rs` into child modules of `workbench`, each an `impl Workbench` block with no new
`pub` items and no behaviour change:

- `workbench_root.rs`: `set_root`, `follow_root`, `follow_changes`, `new_git`, the root and trust plumbing.
- `workbench_files.rs`: `new_file`, `prompt_save_as`, `save_editor_as`, `uses_file`, `editor_moved`,
  `open_from_tree`, `tree_opened`, `busy_under`, `plan_rename`, `moves`, `panel_renamed`, `follow_rename`,
  `follow_moves`, and the free helpers `overlap`, `Busy`, `under`, `moved_to`, `resolved`, `DetachedSaves`.
- `workbench_palettes.rs`: `show_palette`, `quick_open`, the language, go-to-line, recent and toolbar pickers,
  `dismiss_palette`, `push_symbols`, `toggle_breadcrumbs`, `set_nav_buttons`.
- `workbench_chrome.rs`: the title bar, menu bar, window frame and status bar setters and getters.

`Workbench`'s fields, `new`, `render`, `Focusable` and the editor tracking stay in `workbench.rs`.

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

- The spec's lists alone left `workbench.rs` at about 2,050 lines (the pane glue, layout code and terminal
  dock the problem expected T690-T705 to remove are still here), so `workbench_chrome.rs` also takes the
  window around the editors: the terminal dock (`follow_terminal`, `toggle_terminal` and the rest), the
  docks (`run_dock_op`, `move_sidebar`, `resize_focused_dock`, ...), the panes (`pane_added`, `split`,
  `move_tab`, `close_active_pane`, ...), the saved layout (`set_layout` to `restore_layout`, `impl
  LayoutKeys` and its free helpers), `toasts`, `follow_active_editor`/`follow_diff_side` and
  `terminal_dir`. `workbench_files.rs` also takes `prompt_open`, `save_editor_as_with`,
  `set_preview_tabs`, `open_settings`, `launch_reading` and `file_read`; `workbench_root.rs` takes `root`,
  `git`, `git_log_listing` and `reload_settings`; `workbench_palettes.rs` takes the `Palette` enum and
  every picker's accessor. `workbench.rs` keeps the fields, `new`, `render`, `Focusable`, the editor
  tracking (`track_editors`, `tint_tabs`, `editor_changed`, `open_editors`, `active_editor`), the
  child-module declarations and the inline `focus_tests`.
- Bodies are moved verbatim. A private item moved into a child that `workbench.rs` (or a sibling) still
  calls is `pub(super)`, which reaches exactly what private reached before (`workbench` and its
  descendants); `pub`/`pub(crate)` items keep theirs. Paths other modules use stay valid through
  `pub(crate) use self::workbench_files::{Busy, resolved}` and `pub(crate) use
  self::workbench_palettes::Palette` in `workbench.rs`: re-exports, not new items.
- `project_panel_ops.rs` (out of scope) has a doc link to `crate::workbench::overlap`, now at
  `crate::workbench::workbench_files::overlap`; rustdoc isn't in the gate, so it's left for a follow-up.
