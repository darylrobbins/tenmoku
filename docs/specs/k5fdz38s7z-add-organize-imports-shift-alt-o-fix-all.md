---
task_uid: "01m55v1dhw1pebxck5fdz38s7z"
task: "T181"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test lsp_code_actions::tests::kinds::organize_imports_applies_the_one_action passes: lsp.organize_imports sends only [source.organizeImports] and applies a lone action with no picker, lists two, and names the kind when there are none; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit tests lsp_code_actions::tests::kinds::quick_fix_leaves_source_actions_out and lsp_code_actions::tests::kinds::refactor_lists_refactors pass"
    required: true
    manual: false
  - id: "C3"
    text: "fe-keymap integration test organize_imports_and_refactor_keys_match_vs_code (crates/fe-keymap/tests/code_action_kind_keys.rs, top level) passes; the menu holding Code Actions… lists the four right after it; typed_command_ids_tests.rs gains four rows only"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench -p fe-keymap passes with no existing test edited beyond what the spec names, except the count/list expectations in crates/fe-keymap/tests/menu.rs, crates/fe-workbench/src/tests/menu_bar.rs, crates/fe-gpui/src/keymap_tests.rs, crates/fe-gpui/src/tests.rs and crates/fe-gpui/src/vim/tests.rs that the four new menu items and keys force, and the CodeActioner::ask query change in crates/fe-workbench/src/lsp.rs; cargo clippy -p fe-workbench -p fe-keymap --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Add Organize Imports (shift-alt-o), Fix All, Refactor… (ctrl-shift-r) and Source Action…, each asking for its kind, and leave source actions out of Quick Fix, as VS Code does (Mac T937)

## Problem

LSP completeness track, item 4 (user-approved 2026-10-17). fastedit has one code-action command,
`lsp.code_actions` (cmd-. / ctrl-., `crates/fe-workbench/src/workbench_commands.rs:324-328`), which lists every
action at the selection (`show_code_actions`, `crates/fe-workbench/src/lsp_code_actions.rs:69`). VS Code also
has kind-specific commands, each asking with `only`:

- **Organize Imports** (`editor.action.organizeImports`, shift-alt-o): `source.organizeImports`; applied at
  once when there is exactly one such action, else listed;
- **Fix All** (`editor.action.fixAll`, no key): `source.fixAll`, the same way;
- **Refactor…** (`editor.action.refactor`, ctrl-shift-r): `refactor` actions, listed;
- **Source Action…** (`editor.action.sourceAction`, no key): `source` actions, listed;
- **Quick Fix…** (cmd-.) lists everything except `source` actions, which only the commands above offer.

Once the card this depends on lands, fe-lsp asks with `only` (`LspManager::code_actions_for`,
`CodeActionQuery`) and filters by kind.

## Change

1. `crates/fe-workbench/src/lsp_code_actions.rs`:
   - `show_code_actions` takes a `CodeActionQuery` and a `direct: bool`. With `direct`, exactly one enabled
     action is resolved and applied as a confirm in the picker applies it, with no picker; otherwise the
     picker lists them as today. "No code actions available" names the kind when there is one ("No organize
     imports actions available"), as VS Code's messages do.
   - Quick Fix (`lsp.code_actions`) asks with `only: None` and drops `source`-kind actions from the list
     (`CodeAction::kind_is("source")`).
2. `crates/fe-workbench/src/workbench_commands.rs`: new commands, context `editor`:
   `lsp.organize_imports` "Organize Imports" (direct), `lsp.fix_all` "Fix All" (direct), `lsp.refactor`
   "Refactor…", `lsp.source_action` "Source Action…".
3. `crates/fe-keymap/keymaps/fastedit.toml`: `shift-alt-o` → `lsp.organize_imports` and `ctrl-shift-r` →
   `lsp.refactor` (macOS and elsewhere, as VS Code), context `editor`, like `lsp.code_actions`'s binding;
   neither key is bound today.
4. `crates/fe-keymap/menus/fastedit.toml`: the four after "Code Actions…".
5. `crates/fe-workbench/src/typed_command_ids_tests.rs`: four rows in its table (rows only).
6. Tests: new `crates/fe-workbench/src/lsp_code_action_kinds_tests.rs`, declared at the end of
   `crates/fe-workbench/src/lsp_code_actions_tests.rs` as `#[path = "lsp_code_action_kinds_tests.rs"] mod kinds;`,
   with that file's `setup`:
   - `organize_imports_applies_the_one_action`: the fake answers a `source.organizeImports` action with an
     edit; `lsp.organize_imports` sends `only: ["source.organizeImports"]`, applies the edit and shows no
     picker; with two such actions the picker lists both; with none, the info prompt names organize imports.
   - `quick_fix_leaves_source_actions_out`: a reply of a `quickfix` and a `source.fixAll` action lists only
     the quick fix under `lsp.code_actions`, and only the fix-all under `lsp.source_action`.
   - `refactor_lists_refactors`: `lsp.refactor` sends `only: ["refactor"]` and lists `refactor.extract`.
   - fe-keymap: new integration test `crates/fe-keymap/tests/code_action_kind_keys.rs`,
     `organize_imports_and_refactor_keys_match_vs_code` (top level): shift-alt-o and ctrl-shift-r resolve to
     the two commands in an editor context on macOS and Linux, as cmd-. / ctrl-. resolves to
     `lsp.code_actions`.

Order: after the kinds card; the keymap, menu and typed-ID files are append-only collisions with T852 and the
others that hold them, so this follows T852. Linux-OK.

## Implementation notes

- `crates/fe-workbench/src/lsp.rs` (`CodeActioner::ask` takes the `CodeActionQuery`), `crates/fe-keymap/tests/menu.rs` (the Edit menu's item list) and `crates/fe-workbench/src/tests/menu_bar.rs` (the wrap-around index 25 → 29) are touched though outside the listed scope: each is forced by the four new items / the new parameter.
- `CodeActionQuery` is not re-exported from fe-lsp's root; it's imported as `fe_lsp::code_action::CodeActionQuery`.
- Carried finding T89 F1 (`diagnostic.data` dropped) is not fixed: the editor's diagnostic entries (`fe-editor`) carry no `data`, so fixing it needs changes outside this scope.
- `ctrl-shift-r` is bound in plain `editor`, as specified; it also resolves in vim normal and visual (only that exact chord, so vim's ctrl-r is untouched). `crates/fe-gpui/src/{tests,keymap_tests}.rs` and `vim/tests.rs` list the new keys (forced by their completeness checks).
