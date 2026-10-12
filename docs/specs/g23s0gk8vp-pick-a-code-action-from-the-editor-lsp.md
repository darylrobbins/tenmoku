---
task_uid: "01m4rwrnqyj0qm3zg23s0gk8vp"
task: "T89"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test lsp_code_actions::tests::quick_fix_applies: with a diagnostic at the caret, lsp.code_actions sends it in the request, lists the preferred quick fix first and selected, and enter applies its edit as one undoable step"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test lsp_code_actions::tests::commands_and_disabled: an action with a command runs workspace/executeCommand, and the edit the server sends back through workspace/applyEdit is applied; a disabled action shows its reason and refuses the pick"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test lsp_code_actions::tests::empty_and_stale: no actions shows No code actions available; an edit to the buffer before the pick refuses it with The file changed; ask again"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Pick a code action from the editor: lsp.code_actions lists the server's actions for the caret or selection, quick fixes first, and applies the chosen one (Mac T474)

## Problem

The user (2026-10-12) asked for LSP code actions (quick fixes). T469 adds `code_actions`, `resolve_code_action`,
`code_action_edits` and `run_code_action_command` to fe-lsp; T472 applies workspace edits (including a command's
`workspace/applyEdit`) and stubs `lsp.code_actions` (`Workbench::show_code_actions` in
`crates/fe-workbench/src/lsp_code_actions.rs`).

## Change

In `crates/fe-workbench/src/lsp_code_actions.rs` only:

- `show_code_actions`: for the active editor, the primary selection's range (a caret is an empty range) and the
  diagnostics the editor shows overlapping it go to `code_actions` at the buffer's version. While waiting nothing is
  shown; an empty answer is an info prompt "No code actions available".
- The actions show in a `CodeActionsPicker` as `Palette::CodeActions`: one row per action, titled, with its kind
  (`quickfix`, `refactor.extract`…) dimmed; disabled ones greyed with their reason and refusing a pick; the
  preferred quick fix selected first. Typing filters by subsequence as the toolbar picker does.
- Confirm: `resolve_code_action`, then apply `code_action_edits` with `apply_file_edits` (a stale version or a
  refused edit: info prompt, nothing changed), then `run_code_action_command` if it has one; edits the server sends
  while running it arrive as `LspEvent::ApplyEdit`, applied by T472. Focus returns to the editor.
- The buffer changing before a pick lands refuses the pick with "The file changed; ask again".

Not in this card: a lightbulb in the gutter, and a key (cmd-. / ctrl-.), which the LSP key card binds.

## Tests

New file `crates/fe-workbench/src/lsp_code_actions_tests.rs`, declared from lsp_code_actions.rs with `#[path]`, on
fe-lsp's fake server.

## Implementation notes

The card said `lsp_code_actions.rs` only, but three small additions outside it were unavoidable:

- `fe_lsp::Diagnostic` is `#[non_exhaustive]` with no constructor, so the workbench couldn't build the diagnostics to
  send. Added `Diagnostic::new(range, message)` in fe-lsp; the host sets the other (pub) fields.
- `lsp.rs`: `Lsp::code_actioning` (the ask in flight) and `CodeActioner` / `Workbench::lsp_code_actioner`, as
  `Renamer` is, since the manager is private to that module.
- `lsp_rename.rs`: `tell` and `reason` are now `pub(super)` and reused.

The editor's diagnostics don't keep the server's `data`, so it isn't sent back in the request's context.

The picker moves the preferred quick fixes ahead of the rest (fe-lsp ranks those fixing a diagnostic first, so a
preferred fix for nothing came later). A disabled row shows its kind and reason, with its title in the muted colour.
