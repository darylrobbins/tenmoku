---
task_uid: "01m55ac9k4aa0rmjjdmtypt3mx"
task: "T142"
rev: 2
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "dock.rs's, outline_panel.rs's, references_panel.rs's and problems_panel.rs's workbench-context registries and op() functions and project_panel's RevealActive op are gone, run_dock_op, run_references_command and run_problems_op's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists dock.show_project, dock.toggle, search.show, search.show_replace, search.replace_all, git.show_changes, git.refresh, outline.show, the 4 References commands and the 6 problems and diagnostics commands with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "the search panel's own-context commands live in a registry in search_panel.rs, not dock.rs, and still run while the search panel is focused; cargo test -p fe-workbench -p fastedit passes with no existing test file edited, and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the sidebar's show commands and the References and Problems panels' workbench commands to #[kiln::command] methods (Mac T696)

## Problem

The sidebar's show commands and the panels' workbench commands still dispatch through `fe_command`: `dock.rs`'s registry (`DockOp`, `dock::op`, `run_dock_op` in `workbench.rs` ~2428) with `dock.show_project`, `dock.toggle`, `search.show`, `search.show_replace`, `search.replace_all`, `git.show_changes`, `git.refresh`; `outline.show` (`outline_panel::shows`, `outline_panel.rs` ~380); `project_panel`'s `PanelOp::RevealActive` (`project_panel.rs` ~863-1000, `reveal_active`); the References commands (`nav.find_references`, `references_panel.open`, `references_panel.select_next`, `references_panel.select_prev`: `references_panel.rs` ~584-640, `run_references_command`); and the problems ops (`problems.show`, `problems.open`, `problems.select_next`, `problems.select_previous`, `diagnostics.next`, `diagnostics.previous`: `ProblemsOp`, `problems_panel.rs` ~63-120, `run_problems_op`). With T692 each panel's spec names its show command.

## Change

Move these commands to methods in `workbench_commands.rs` with their old IDs, titles and contexts. `DockOp` and `ProblemsOp` stay (rail clicks and `SidebarPanel::show_op` use them); their registries and `op()` functions go. The search panel's own-context commands (`search.toggle_*`, `search.select_next`/`select_previous`, `search.open_selected`, `search.focus_results`, `text_input.submit`), which the search panel runs while focused, stay in an `fe_command` registry, moved from `dock.rs` to `search_panel.rs`; so do the project panel's own file operations (`project_panel.rs`'s other ops), which the panel runs itself. `problems.warnings` is the status item's ID, not a command, and stays.

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

## Placement (neon lead, per Mac note zz30)

argon's T702/T703 and helium's T704 move other subsystems' commands into the same files at the same time. kiln's `#[kiln::command(context = ..)] impl` emits one `impl kiln::ViewCommands for Workbench`, so a second block on `Workbench` in a per-family file would conflict: keep the single block in workbench_commands.rs, but add this card's methods together at a distinct place in it (for example right after the dock/layout commands), not at the end of the impl. Keep lib.rs and typed_command_ids_tests.rs edits to this card's own entries.

## Implementation notes

- The search panel and the References panel still do their commands themselves while focused, as they did when
  they caught the `RunCommand`: the panels handle the typed actions (`ReplaceInFiles`, `ReplaceAllInFiles`,
  `SelectPreviousReference`, `SelectNextReference`, `OpenReference`) with `.on_action` before the workbench's
  handler, so `search.show_replace` in the focused panel still seeds nothing from the editor, and opening a
  reference (which updates the workbench) runs in the panel's own update. Reached at the workbench instead, the
  References commands run on the panel once the workbench's update is over.
- Every moved command checks its context against the invocation's, as its registry's dispatch did (review
  F1): `workbench_commands::invoked_in`, through `Workbench::in_context` (which `in_editor` now calls) in the
  workbench's handlers, and directly in the search and References panels' listeners. A run with no invocation,
  a raw action, applies. `typed_command_ids_tests.rs` checks matching and mismatching contexts through the
  workbench's handlers and the focused panels'.
- `project_panel.reveal_active` is a typed command, and its `RevealActive` op is gone, but the project panel's
  registry still lists the ID, naming no op: `tests/tree_keys.rs`'s
  `the_default_keymap_binds_the_panel_commands`, which can't be edited, asserts the registry has it. So it isn't
  in `MOVED` (no registry may claim those); `reveal_active_is_a_typed_command` checks its title and context. The
  palette lists it once, as it dedupes by ID, and a dispatch or keymap check finds the typed command first.
- The search panel's own commands are `search_panel::registry`, of `SearchOp`s; `vim_line.rs` reads
  `text_input.submit` from it.
