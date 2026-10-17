---
task_uid: "01m53q0tjbrzm6sdgt66wag379"
task: "T115"
rev: 2
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "lsp.rs's, open.rs's and trust.rs's registries, Commands globals and op() functions are gone, run_lsp_op, prompt_open and set_root_trust's arm in Workbench::on_run_command and its lines in command_catalog() and command_list() are deleted, and each command is a #[command(id = \"..\")] method in workbench_commands.rs's one #[kiln::command(context = \"workbench\")] impl Workbench block"
    required: true
    manual: false
  - id: "C2"
    text: "typed_command_ids_tests.rs lists the 6 lsp.*, 7 open (window.new, workbench.new_file, open_file, open_folder, open_folder_in_new_window, open_settings, save_as) and 2 trust commands with its old title and context, each a registered typed command that no fe_command registry in command_catalog() still claims, and passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench -p fastedit passes with no existing test file edited except the conversion of workspace_edit_tests.rs stub_commands_in_the_editor_context the lead's note allows (registration, title and editor context kept, through the typed commands; only the LspOp dispatch assertion dropped), and no command ID, title or key context differs from the base (keymap and menu TOML untouched)"
    required: true
    manual: false
---

# Move the lsp, open and trust commands to #[kiln::command] methods (Mac T699)

Filed on argon from Mac T699 (card ~/coordination/argon-cards/zo/T699.json rev 2; note zo, the kiln phase-3 chain, card 9 of 17, serial). Mac task uid 01m4vbjsfwvav4cnwkqh58d70b. Base: argon trunk (mac/main e776ee0 synced; a9a5125e's stack 12 doesn't touch T700's files). Design record: docs/decisions/kiln-app-layer.md. Argon dep: T114 (Mac T698), the card before. Title the host commit "... (Mac T699)".

## Problem

Three small families still dispatch through `fe_command`: `lsp` (`crates/fe-workbench/src/lsp.rs` ~2013-2110: `LspOp`, registry, `op()`; `run_lsp_op` arm): `lsp.code_actions`, `lsp.format_document`, `lsp.format_selection`, `lsp.rename`, `lsp.restart`, `lsp.show_log`; `open` (`crates/fe-workbench/src/open.rs` ~34-150: `OpenOp`, registry, `op()`; the `prompt_open` arm): `window.new`, `workbench.new_file`, `workbench.open_file`, `workbench.open_folder`, `workbench.open_folder_in_new_window`, `workbench.open_settings` and Save As (`fe_gpui::pane_item::SAVE_AS`); and `trust` (`crates/fe-workbench/src/trust.rs` ~102-134: registry, `op()`; the `set_root_trust` arm): `workbench.trust_folder`, `workbench.untrust_folder`. This card absorbs the former T700 (open and trust), which was repurposed as the chain's kiln prerequisite.

## Change

Move the 15 commands to methods in `workbench_commands.rs` with their old IDs and titles, calling `run_lsp_op`'s, `prompt_open`'s and `set_root_trust`'s bodies. `fe-lsp` is not touched; `lsp_format.rs` (Mac T542's) is not touched.

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

C1 and C3 conflicted on workspace_edit_tests.rs `stub_commands_in_the_editor_context`, which reads lsp::registry() and
dispatches LspOp through it. Don't delete it. C3 is narrowed to allow converting that one test, nothing else: for each
of lsp.rename, lsp.code_actions, lsp.format_document and lsp.format_selection, keep asserting that it's registered, its
title, and that its context is the editor's (fe_editor::commands::CONTEXT), now through the typed commands (for example
kiln::platform::command_actions::typed_command, as typed_command_ids_tests.rs does). Drop only the LspOp dispatch
assertion and the imports it used, because C1 removes LspOp. workspace_edit_tests.rs is added to the scope.
T114's F1 (palette opt-out) is T134's, which comes right after this card, so leave it alone here.

## Implementation notes

- The 15 commands are methods in `workbench_commands.rs`'s `#[kiln::command(context = "workbench")]` block, each
  with its old ID and title; the four editor-context LSP commands give `context = "editor"`. The LSP methods call
  `run_server_op`, `rename_symbol`, `show_code_actions` and `format_active` directly, so `LspOp` and `run_lsp_op`
  are gone (review F2). The open methods call `prompt_open` (now `pub(crate)`), and `OpenOp` stays because
  `prompt_open` and its panel's answer match on it. The trust methods call `set_root_trust`. `lsp::init` keeps the
  quit hook, `trust::init` keeps `kiln_shell::trust::init`, and `open::init` is gone.
- **Editor context enforced (review F1).** kiln attaches a typed command's handler to the view's root and runs it
  wherever it's dispatched, without evaluating its context. The old registry rejected an invocation whose context
  stack didn't hold. `Workbench::in_editor` restores that check for the four editor commands: an invocation
  (menu, key or `RunCommand`) outside the `editor` context does nothing, and a raw action (no invocation, so its
  key binding already checked the context) runs. `typed_command_ids_tests.rs`
  `editor_lsp_commands_run_only_in_the_editors_context` covers it: with no server, each command's prompt or toast
  appears only in the editor's context, never in `workbench`, `project_panel` or `terminal`. It fails without the
  guard.
- **C1/C3 conflict, resolved by the lead's note.** `workspace_edit_tests.rs::stub_commands_in_the_editor_context`
  read `lsp::registry()` and dispatched `LspOp` through it. It's converted, as the note allows: a `#[gpui::test]`
  that inits the workbench and asserts, through `kiln::platform::command_actions::typed_command`, that each of the
  four editor commands is registered with its title and the editor's context. Only the dispatch assertion and its
  imports went; dispatch in the editor context stays covered by the LSP rename, code-actions and format tests.
- **T114 F1 (carried).** Left to T134, per the lead's note: kiln's palette lists every typed command whose context
  holds, and `#[command]` has no palette opt-out. None of this card's 15 commands were hidden from the palette by
  their registries, so this card adds no new case.
