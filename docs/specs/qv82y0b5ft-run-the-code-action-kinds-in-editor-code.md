---
task_uid: "01m55v1f69cp1eq3qv82y0b5ft"
task: "T184"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test lsp_format::save_tests::code_actions_on_save::organize_imports_runs_before_format passes: a save sends codeAction with only [source.organizeImports] then textDocument/formatting, and the file on disk has both edits; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit tests lsp_format::save_tests::code_actions_on_save::no_setting_no_request and lsp_format::save_tests::code_actions_on_save::a_slow_server_doesnt_hold_the_save pass: nothing is asked with the list empty and format on save off; a reply past the limit leaves the file written unedited within SAVE_HOOK_LIMIT and is never applied; the 750 ms limit is checked before each action and each command (a second action is not started once it has passed); the tests wait on the fake's wait_sent, not on sleeps"
    required: true
    manual: false
  - id: "C3"
    text: "The existing lsp_format_save_tests.rs tests pass unchanged (only the mod line added)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Run the code action kinds in editor.code_actions_on_save when a file is saved, before format on save and within the save's time limit, as VS Code's codeActionsOnSave does (Mac T940)

## Problem

LSP completeness track, item 4 (user-approved 2026-10-17: "`editor.code_actions_on_save` in the format-on-save
hook"). VS Code's `editor.codeActionsOnSave` runs the configured kinds (`source.organizeImports`,
`source.fixAll`, a server's own such as `source.fixAll.eslint`) when a file is saved, before format on save,
within a time limit (`editor.codeActionsOnSaveTimeout`, 750 ms), and saves anyway when it runs out.

fastedit's only save-time LSP work is `FormatOnSave` (`crates/fe-workbench/src/lsp_format.rs:169-200`), a
`SaveHook` whose task the save waits for, at most `SAVE_HOOK_LIMIT` (1 s, `crates/fe-gpui/src/pane_item.rs:301`)
for the whole hook; `sync_save_hooks` (`crates/fe-workbench/src/lsp.rs:1083`) installs it only while
`editor.format_on_save` is on. Once the cards this depends on land, `editor.code_actions_on_save` (a list of
kinds, default empty) exists and fe-lsp asks with `only` (`code_actions_for`, `CodeActionQuery`).

## Change

1. `crates/fe-workbench/src/lsp_format.rs` `FormatOnSave` (the hook keeps its name; its doc says it does both):
   - For each kind in `editor.code_actions_on_save`, in order: ask `code_actions_for` over the whole buffer
     with `only: [kind]` and no diagnostics, at the buffer's current version; take the enabled actions of
     that kind; resolve, apply its edits (`Workbench::apply_file_edits`, as the code-actions picker applies
     them) and run its command, one action after another. A kind with no action, a failure or a stale answer
     is logged at debug and the next kind runs; nothing is shown.
   - Then format, if `editor.format_on_save` is on, as today.
   - The whole runs inside the save's one second: code actions stop being started 750 ms after the hook
     began (VS Code's default), leaving formatting the rest; the save goes on at `SAVE_HOOK_LIMIT` as today.
   - Edits land in the buffer before the save writes it, in undo steps of their own.
2. `crates/fe-workbench/src/lsp.rs` `sync_save_hooks`: installs the hook while either setting is on (the list
   non-empty, or format on save), and passes it both, read live.
3. Tests: new `crates/fe-workbench/src/lsp_code_actions_on_save_tests.rs`, declared at the end of
   `crates/fe-workbench/src/lsp_format_save_tests.rs` as
   `#[path = "lsp_code_actions_on_save_tests.rs"] mod code_actions_on_save;`, with that file's fake:
   - `organize_imports_runs_before_format`: with `code_actions_on_save = ["source.organizeImports"]` and
     `format_on_save = true`, saving sends a `textDocument/codeAction` with `only: ["source.organizeImports"]`
     and then `textDocument/formatting`; the file on disk has both edits.
   - `no_setting_no_request`: with the list empty and format on save off, a save sends no
     `textDocument/codeAction` and no hook runs (as at the base).
   - `a_slow_server_doesnt_hold_the_save`: a code-action reply delayed past the save limit: the file is
     written unedited within `SAVE_HOOK_LIMIT` plus a frame, and the late reply is not applied afterwards.
   - The existing `lsp_format_save_tests.rs` tests pass unchanged.

Linux-OK.
## Revision 3: simplify (lead decision, zz68)

Apply each configured kind's actions once, in order. This replaces the re-ask loop, the 8-action cap and any
per-save command-ownership machinery in the change above (review F3, F5 and F6 go with them):

- No re-asking after an action is applied, no cap on the number of actions, no bookkeeping of which commands
  the save started.
- Check the 750 ms deadline before each action and before each command, not only before each kind (review F1).
- Tests wait on the fake's `wait_sent` for a request rather than sleeping or polling, so the ordering race in
  the first test goes (review F4).
- It doesn't depend on T181: both touch `crates/fe-workbench/src/lsp.rs` in different regions; whichever
  lands second rebases.

## Implementation notes

- Per revision 3: each kind is asked for once and its enabled actions are applied once, in order; no re-ask, no
  action cap, no command bookkeeping. A later action whose edits carry the buffer version the first one changed
  is stale and is logged, which ends the kind (`apply_file_edits` refuses it).
- The 750 ms limit is a timer task setting a flag, checked before each action's resolve, apply and command; the
  whole phase is also dropped at 750 ms (`with_timeout`) so a pending request can't hold up formatting.
- `reload_settings` now lives in `workbench_root.rs`, so the live setting is fed from there and from
  `Workbench::new` in `workbench.rs`.
