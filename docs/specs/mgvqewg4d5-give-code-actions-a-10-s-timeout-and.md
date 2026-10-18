---
task_uid: "01m55sbhmnp3rkxtmgvqewg4d5"
task: "T165"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::cancel::own_timeouts::code_actions_and_signature_help_have_their_own_timeouts passes: requests::timeout gives textDocument/codeAction 10 s with LspTimeouts::default(), and with hover set to 500 ms gives textDocument/signatureHelp 3 s and textDocument/hover 500 ms; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config integration test lsp_feature_timeouts_round_trip (top level, in crates/fe-config/tests/lsp_feature_timeouts_setting.rs) passes: [lsp.timeouts] code_action = 1500 and signature_help = 700 load from a user file, and the defaults are 10000 and 3000, read through serde_json so it compiles at the base; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-config/settings.schema.json is regenerated and fe-config's schema test passes; code_action.rs and signature_help.rs docs and the timeouts table in docs/decisions/lsp-client.md name the new keys and values"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp -p fe-config -p fe-workbench passes; the only existing test edited is fe-lsp code_action_tests.rs::timeout_is_empty, moved from the old 3 s code-action timeout to the new 10 s default (its waits and advances only), plus the mod line added to tests/cancel.rs; cargo clippy -p fe-lsp -p fe-config --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Give code actions a 10 s timeout and signature help its own 3 s one, lsp.timeouts.code_action and signature_help, instead of hover's 3 s, after which cmd-. says there are no code actions (Mac T918)

## Problem

LSP gap audit (2026-10-17, main c7b6a04a): "Code actions and signature help borrow hover's 3 s timeout ...
A cold rust-analyzer can answer 'no actions'." Re-verified at main 40608138:

- `crates/fe-lsp/src/requests.rs:68-70` maps `textDocument/codeAction` and `textDocument/signatureHelp` to
  `timeouts.hover` (3000 ms, `crates/fe-config/src/settings.rs:327`). The docs say so too:
  `crates/fe-lsp/src/code_action.rs:8` and `:86`, `crates/fe-lsp/src/signature_help.rs:12`.
- On a timeout, `LspManager::code_actions` answers an empty list, and the workbench says "No code actions
  available" (`crates/fe-workbench/src/lsp_code_actions.rs:150`). After 3 s the user is told there are no
  actions when the server just hasn't answered yet, such as rust-analyzer's first request after opening a
  crate while it loads. `lsp.code_actions` (cmd-.) is an explicit request; VS Code puts no 3 s limit on
  it and cancels only when the caret moves.
- Raising `[lsp.timeouts] hover` to work around it also changes hover and signature help.

## Change

1. `crates/fe-config/src/settings.rs` `LspTimeouts`: two new keys, each with a doc line and `range(min = 1)`.
   - `code_action`, default **10000**. The request is explicit, and its answer can't land stale: the
     workbench drops a list whose editor, version or selection has changed (`lsp_code_actions.rs`
     `listed`). 10 s bounds a hung server while covering a slow first answer; `references` already
     waits 30 s for the same reason.
   - `signature_help`, default **3000**. That value is unchanged: it pops up while typing, and the next
     trigger replaces it, so a late answer is worth little. It is now independent of `hover`.

   Regenerate `crates/fe-config/settings.schema.json` (`UPDATE_GOLDEN=1`, as the schema test says).
2. `crates/fe-lsp/src/requests.rs` `timeout`: `textDocument/codeAction` → `timeouts.code_action`,
   `textDocument/signatureHelp` → `timeouts.signature_help`; `hover` keeps `textDocument/hover` alone.
3. Docs: `code_action.rs` (module docs and `code_actions`' doc) and `signature_help.rs` (module docs) name
   their own keys. In `docs/decisions/lsp-client.md`'s timeouts table (~line 941), add
   `codeAction 10 s (picker says none only once it's past)` and `signatureHelp 3 s`.
4. Tests:
   - New `crates/fe-lsp/src/tests/cancel/own_timeouts.rs`, declared in `crates/fe-lsp/src/tests/cancel.rs`
     (which holds the timeout tests). It uses only the base's API, so it compiles at the base.
     `code_actions_and_signature_help_have_their_own_timeouts`:
     - `crate::requests::timeout("textDocument/codeAction", &LspTimeouts::default())` is 10 s;
     - with `LspTimeouts { hover: 500, ..Default::default() }`, `textDocument/signatureHelp` is still 3 s
       and `textDocument/hover` is 500 ms.
   - New integration test `crates/fe-config/tests/lsp_feature_timeouts_setting.rs`,
     `lsp_feature_timeouts_round_trip`: a user file with `[lsp.timeouts] code_action = 1500` and
     `signature_help = 700` loads with those values, and the defaults are 10000 and 3000. Read the values
     through `serde_json::to_value(&settings.lsp.timeouts)`, not the new fields. Integration tests are
     overlaid at the base, and one naming a new item breaks the base build.
