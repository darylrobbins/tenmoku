---
task_uid: "01m53q0t1zb1bh401k4hjn9fw4"
task: "T113"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "StatusOp, its registry and op() in status_bar.rs are gone, run_status_op's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists view.toggle_status_bar and language.select with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the status bar's commands (view.toggle_status_bar, language.select) to #[kiln::command] methods (Mac T697)

Filed on argon from Mac T697 (card ~/coordination/argon-cards/zo/T697.json rev 2; note zo, the kiln phase-3 chain, card 7 of 17, serial). Mac task uid 01m4vbjqskjbj57rm9qyzvmpr5. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T112 (Mac T695), the card before. Title the host commit "... (Mac T697)".

## Problem

The `status_bar` family (`crates/fe-workbench/src/status_bar.rs` ~265-346: `StatusOp`, its registry, `op()`) and its `run_status_op` arm (`workbench.rs` ~2321) still dispatch through `fe_command`: `view.toggle_status_bar` and `language.select`.

## Change

Move the 2 commands to methods in `workbench_commands.rs` with their old IDs and titles, calling `show_status_bar`/`set_status_bar_visible` and `select_language` as `run_status_op` does. fastedit's `StatusBar` view, its items and its vim line stay as they are: kiln's built-in workspace bar has no command line, so moving the items to `Workspace::add_status_source` is not part of this card.

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
