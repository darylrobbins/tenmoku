---
task_uid: "01m53q0snw9z71y2p6rv42p470"
task: "T112"
rev: 2
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "DockLayoutOp, its registry, init and op() in dock_layout.rs are gone, run_dock_layout_op's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists the 7 dock.* and terminal.move_to_* commands with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited except the one lookup change the lead's note allows in dock_resize_tests.rs and dock_position_tests.rs (crate::command_catalog() in place of crate::command_list(), both assertions kept), and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the 7 dock_layout commands (move the sidebar and the terminal, grow and shrink the dock) to #[kiln::command] methods (Mac T695)

Filed on argon from Mac T695 (card ~/coordination/argon-cards/zo/T695.json rev 2; note zo, the kiln phase-3 chain, card 6 of 17, serial). Mac task uid 01m4vbjnxcx5415y7bgvtr05p6. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T111 (Mac T694), the card before. Title the host commit "... (Mac T695)".

## Problem

The `dock_layout` family (`crates/fe-workbench/src/dock_layout.rs` ~26-151: `DockLayoutOp`, its registry, `init`, `op()`) and its `run_dock_layout_op` arm (`workbench.rs` ~1507) still dispatch through `fe_command`: `dock.move_sidebar_left`, `dock.move_sidebar_right`, `dock.grow`, `dock.shrink`, `terminal.move_to_bottom`, `terminal.move_to_left`, `terminal.move_to_right`.

## Change

Move the 7 commands to methods in `workbench_commands.rs` with their old IDs and titles, calling `move_sidebar`, `move_terminal` and the dock resize that `run_dock_layout_op` does. `dock_layout::position_name` and the layout helpers T691 uses stay.

## How a command family moves (T693 sets this up; every later family card repeats it)

- Each command becomes a method marked `#[command(id = "<old id>")]` in the one
  `#[kiln::command(context = "workbench")] impl Workbench` block in `crates/fe-workbench/src/workbench_commands.rs`.
  A view gets one `ViewCommands` impl, so every family's Workbench methods go in that block; a method's body calls
  the family's existing function (`self.run_pane_op(..)`'s body, moved or called). Its doc line is the old title;
  a command whose old context wasn't `workbench` gives `context = ".."` on the method.
- Delete the family's `CommandTarget`, `Commands` global, `init`/`registry()`/`op()`, its arm in
  `Workbench::on_run_command` (`workbench.rs`), and its lines in `command_catalog()` and `command_list()`
  (`lib.rs`). Its `Op` enum goes too unless something other than command dispatch uses it (a rail click, a
  toolbar button): then it stays, only the registry goes.
- Append the family's IDs, titles and contexts to the table in `crates/fe-workbench/src/typed_command_ids_tests.rs`.

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
## Lead's note (argon, 2026-10-17, after the worker's block)

C1 and C3 conflicted. Two existing tests assert the moved commands are in `crate::command_list()`:
dock_resize_tests.rs `grow_and_shrink_are_in_the_palette_with_no_default_key` and dock_position_tests.rs
`the_move_commands_are_in_the_palette_with_no_default_key`. Once the commands are typed, the palette lists them itself
rather than through command_list(). C3 is narrowed to allow one edit in each of those two tests, nothing else: look the
IDs up in `crate::command_catalog()` (which holds the recorded typed commands) instead of `crate::command_list()`. Keep
both assertions, that each ID is listed and that no default keymap binds it. Don't add the commands back to command_list()
(that would list them twice in the palette). dock_resize_tests.rs and dock_position_tests.rs are added to the scope.
