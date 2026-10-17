---
task_uid: "01m53q0v3kt4xq4whfw3azmcjk"
task: "T118"
rev: 2
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "terminal_panel.rs's registry, Commands global and op() are gone, run_terminal_op's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists the 9 terminal.* commands with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the 9 terminal panel commands to #[kiln::command] methods (Mac T703)

Filed on argon from Mac T703 (card ~/coordination/argon-cards/zo/T703.json rev 2; note zo, the kiln phase-3 chain, card 12 of 17, serial). Mac task uid 01m4vbjwr8115cczeggg27pmkt. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T117 (Mac T702), the card before. Title the host commit "... (Mac T703)".

## Problem

The `terminal_panel` family (`crates/fe-workbench/src/terminal_panel.rs` ~587-690: `TerminalOp`, its registry, `op()`) and its `run_terminal_op` arm (`workbench.rs` ~692) still dispatch through `fe_command`: `terminal.toggle`, `terminal.new`, `terminal.close`, `terminal.next`, `terminal.previous`, `terminal.next_prompt`, `terminal.previous_prompt`, `terminal.find`, `terminal.vi_mode`.

## Change

Move the 9 commands to methods in `workbench_commands.rs` with their old IDs, titles and contexts, calling `toggle_terminal` and what `run_terminal_op` does. `TerminalOp` stays if the terminal panel's tab bar buttons use it. Nothing in `fe-terminal-view` or `TerminalElement` changes.

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
## Lead's note (argon, 2026-10-17; from the Mac lead's note zzm)

The rest of the zo chain now runs across hosts (T696 on neon, T704 on helium), so cherry-picks between hosts must stay
clean. Where a new #[kiln::command] method can live in a per-family file (as T702's find_commands.rs does, for example a
terminal_commands.rs) rather than being appended to workbench_commands.rs, prefer that. If an existing test asserts a
moved command is in crate::command_list(), the same narrow exception as T112/T116 applies: change only that lookup to
the typed commands or command_catalog(), keeping every assertion. Say so in your criteria file.

## Implementation notes

- The 9 methods are in `workbench_commands.rs`'s one `#[kiln::command(context = "workbench")] impl Workbench`
  block, not a per-family `terminal_commands.rs`: the macro emits a `ViewCommands` impl and a view gets one, so a
  second block on `Workbench` can't compile (`find_commands.rs` could only because its block is on `Pane`). They're
  appended at the block's end; each is one or two lines calling a `Workbench` method in `workbench.rs`.
- `run_terminal_op` became one `pub(crate)` method per command (`new_terminal`, `close_terminal`, `cycle_terminal`,
  `find_in_terminal`, `scroll_terminal_to_prompt`, `toggle_terminal_vi_mode`), with the same steps in the same
  order; `toggle_terminal` became `pub(crate)`. `TerminalOp` is gone: only command dispatch used it (the tab strip's
  "+" button runs `terminal.new` by ID).
- `terminal.previous_prompt`, `terminal.next_prompt` and `terminal.vi_mode` are in the `terminal` context, which
  their registry's dispatch checked but kiln doesn't (T115 F1). `in_editor`'s guard became `in_context(context, ..)`,
  which `in_editor` and the three terminal methods call. `typed_command_ids_tests.rs`'s
  `terminal_commands_run_only_in_the_terminals_context` checks `terminal.vi_mode` from outside the terminal doesn't
  show the dock.
- Existing tests edited under the T112/T116 exception, lookup only, every assertion kept: four tests looked the
  moved commands up in `crate::command_list()` (`terminal_panel_tests::toggle_is_in_the_palette`,
  `terminal_tabs_tests::commands_are_in_the_palette`,
  `terminal_shell_integration_tests::prompt_commands_are_in_the_palette_in_the_terminal`,
  `terminal_vi_mode_tests::vi_mode_is_in_the_palette_in_the_terminal`). Each now looks them up with
  `kiln::platform::command_actions::typed_command` after `crate::init`, so it became a `#[gpui::test]`.
