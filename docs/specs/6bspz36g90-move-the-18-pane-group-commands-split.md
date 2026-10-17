---
task_uid: "01m53q0s1qxp50v06bspz36g90"
task: "T111"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "PaneCommands, its Commands global, init, registry() and op() in pane_group.rs are gone, run_pane_op's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists all 18 pane.* commands with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the 18 pane_group commands (split, close, focus, move tab, resize, equalize) to #[kiln::command] methods (Mac T694)

Filed on argon from Mac T694 (card ~/coordination/argon-cards/zo/T694.json rev 2; note zo, the kiln phase-3 chain, card 5 of 17, serial). Mac task uid 01m4vbjn7vt28rmzqymw6th25a. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T110 (Mac T693), the card before. Title the host commit "... (Mac T694)".

## Problem

The `pane_group` family (`crates/fe-workbench/src/pane_group.rs` ~92-203: `PaneOp`, `PaneCommands`, its `Global`, `init`/`registry()`/`op()`) and its `run_pane_op` arm (`workbench.rs` ~1476) still dispatch through `fe_command`: 18 commands, `pane.split_right`, `pane.split_down`, `pane.close`, `pane.focus_{left,right,up,down,next,previous}`, `pane.move_tab_{left,right,up,down}`, `pane.grow_width`/`grow_height`/`shrink_width`/`shrink_height` and `pane.equalize`, all in the `workbench` context.

## Change

Move the 18 `pane.*` commands to methods in `workbench_commands.rs`, each with its old ID and title, calling what `run_pane_op`'s arms call (`split`, `close_active_pane`, focus, `move_tab`, resize, equalize). `PaneOp` goes unless a non-command caller uses it. fastedit's `PaneGroup` wrapper keeps its rendering and the copy T690 gave it.

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

## Implementation notes

- `PaneOp`, `PaneCommands`, its `Commands` global, `init`, `registry()` and `op()` are gone from
  `pane_group.rs`; only `RESIZE_STEP` stays there (`pane_resize_tests.rs` reads it as `super::RESIZE_STEP`).
  Nothing outside command dispatch used `PaneOp`.
- `run_pane_op`'s arms are now `Workbench` helpers in `workbench.rs` (`focus_pane`, `focus_next_pane`,
  `resize_active_pane`, `equalize_panes`); `split`, `move_tab` and `close_active_pane` became `pub(crate)` so the
  `workbench_commands.rs` methods can call them.
- The palette dispatches a typed command straight to its action, which skips `on_run_command`, so the test-only
  `commands_run` log (which `palette_tests.rs` reads for `pane.split_right`) missed it. Now each typed
  workbench command's handler notes `kiln::current_invocation` there (`Workbench::ran`), and
  `on_run_command` notes only the commands it doesn't hand on as typed actions, so nothing is logged twice. Later
  family cards should call `self.ran(cx)` in their methods too.
