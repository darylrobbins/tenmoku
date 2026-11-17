---
task_uid: "01m7j21v10rhr3pdyahzk7wka5"
task: "T1898"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::editor_menu_context::code_actions_item_appears_after_rename_when_the_server_offers_actions, no_code_actions_item_for_only_source_actions_or_none and a_late_code_action_answer_adds_nothing (menu closed, other ticket or old version) pass, with lsp_lightbulb.rs unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "tests::editor_menu_context::run_test_shows_on_a_test_and_runs_it, no_run_test_outside_a_test_or_while_parsing, toggle_breakpoint_follows_breakpoint_handlers, git_blame_item_shows_in_a_repository_with_its_state and ask_agent_attaches_the_selection pass"
    required: true
    manual: false
  - id: "C3"
    text: "Passing: tests::editor_menu, its tests unchanged but for this card's new items in their expected menus (Copy Reference in the path group, the context group last); unchanged and passing: test_commands_tests, tests::project_menu and the lsp_lightbulb tests; cargo nextest run -p fe-workbench --test l10n passes"
    required: true
    manual: false
  - id: "C4"
    text: "path_commands_tests::copy_reference_copies_path_and_line and copy_reference_gives_a_row_range_for_a_multi_row_selection pass; workbench.copy_reference has its rows in typed_command_ids_tests.rs and command_catalog_tests.rs and cargo nextest run -p fe-workbench typed_command_ids command_catalog passes"
    required: true
    manual: false
  - id: "C5"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden unchanged, cargo run -p fe-workbench --example screenshots -- audit target/ui-audit exits 0, and a new editor_menu audit scene (audit/editor_menu.rs registered in audit_scenes.rs; opened by a real right-click via deferred dispatch_event; one shot with the server starting, LSP items disabled; one with the context items) was looked at"
    required: true
    manual: false
---

# Editor context menu (2/2): Code Actions when the server offers some at the point, Run Test, Toggle Breakpoint, Git Blame and Ask Agent items

# Editor context menu (2 of 2): code actions, run test, breakpoint, git blame, ask agent

## Why

The user, on finding that a right-click in the editor does nothing: "We have no
context menu for the editor?" and, on the proposed list: "Yes please, let's
prioritize this one. Seems like a big gap."

Card 1 (this card's dependency) adds the menu (fe-gpui's
`crates/fe-gpui/src/view/context_menu.rs`, the provider global, the
`EditorMenuItem`/`EditorMenuRequest` types, `extend_editor_menu`) and the
workbench's provider in `crates/fe-workbench/src/editor_menu.rs` with the
clipboard, path and LSP navigation items. This card adds the remaining agreed
items, in the workbench only: no fe-gpui, kiln or `view.rs` change.

## What the user sees

The same rules as card 1: an item shows only where it applies, LSP items are
disabled while no server is ready, each shows its shortcut from the keymap in
the editor's own context (`pane_toolbar::menu_shortcut` with
`request.contexts`), titles are menu strings in
`crates/fe-workbench/l10n/en-US/editor.ftl`.

- **Code Actions…** (`lsp.code_actions`), in the LSP group right after Rename
  Symbol (`extend_editor_menu(ticket, "lsp.rename", ..)`), only if the server
  offers actions at the point: when the menu opens on a buffer whose server has
  a code action provider, the workbench asks for the actions at the request's
  range (the `fe_lsp::code_action::CodeActionQuery` the lightbulb in
  `crates/fe-workbench/src/lsp_lightbulb.rs` sends, `triggerKind` invoked, no
  kind), and adds the item when an enabled action other than a `source` one
  comes back for the same buffer version and range. The menu opens at once
  without it; an answer after the menu closed, for another ticket or an old
  version, adds nothing; a failed or empty answer adds nothing. Do not change
  `lsp_lightbulb.rs` (T1739 owns it): call the query from `editor_menu.rs`.
- **Run Test** (`tests.run_at_cursor`), in a last "context" group: when
  `Workbench::test_file(Target::Cursor, ..)` finds a runnable at the caret
  (which card 1 already moved to the click); "Run Tests" when it is a module.
  Nothing while the language loads or the parse is stale (no toast from the
  menu).
- **Toggle Breakpoint** (`debug.toggle_breakpoint`): when the editor has
  breakpoint handlers (`EditorView::has_breakpoint_handlers`, set by
  `debug_gutter.rs` for files the debugger can stop in); it acts on the
  caret's row, which is the clicked row.
- **Annotate with Git Blame** / **Hide Git Blame**
  (`git.toggle_blame_annotations`), by whether annotations show in this
  editor: only for a file inside the window's git repository.
- **Ask Agent About Selection** (`chat.ask_about_selection`; "Ask Agent About
  This Line" with an empty selection, as the command attaches the current row
  then): for any pane editor with a buffer, as the command works in an
  untrusted folder too.

## Out of scope

- **Debug Test:** no command yet. T1856 adds `debug.test_at_cursor`; adding
  the item after it lands is a one-line follow-up.
- **Explain Selection:** no command; `chat.ask_about_selection` covers asking.
  A prompt-prefilled "Explain" would need a new chat command (agent_actions.rs
  is T1834's).
- Changes to the menu itself (card 1).


## Lead respec (2026-11-16)
Moved here from T1897 so T1897 could start beside T1852: the Copy Reference item and its new command workbench.copy_reference (path:line, a row range for a multi-row selection) in path_commands.rs with its rows in typed_command_ids_tests.rs and command_catalog_tests.rs, and the editor_menu UI audit scene. This card is now Mac-only (audit).
