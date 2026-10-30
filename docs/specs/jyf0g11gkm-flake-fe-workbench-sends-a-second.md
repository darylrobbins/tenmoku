---
task_uid: "01m64wnv72tetx99jyf0g11gkm"
task: "T1242"
rev: 5
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Cause: under load (T582 gate and T1229 gate, 2026-10-29) two different tests failed on assert_eq!(requests(\u0026s), 1) right after waiting for the server's folds, with left 2 right 1; both pass alone. Seen in two places, so treat it as a real double-ask in lsp_folds.rs (e.g. a re-ask on a debounce/refresh timer, focus or draw while the first request is still outstanding) until shown otherwise. Name the cause in the spec's notes."
    required: true
    manual: false
  - id: "C2"
    text: "Fix in lsp_folds.rs so one document version with no edits gets one foldingRange request while a request is outstanding, however slowly the reply arrives relative to the fake clock; a later edit or server refresh still re-asks."
    required: true
    manual: false
  - id: "C3"
    text: "Regression test in a new file crates/fe-workbench/src/tests/lsp_folds_single_request.rs (declared from tests/mod.rs, main's API only): it delays the fold reply across several clock advances and draws, then asserts exactly one request with assert_eq!; it fails at base. Name it with --regression-test."
    required: true
    manual: false
  - id: "C4"
    text: "If the second request turns out to be legitimate behaviour, block and tell the lead instead of changing lsp_folds.rs (it becomes a test-only chore)."
    required: true
    manual: false
  - id: "C5"
    text: "Proof: both named tests pass 30 times in a row under load (20+ CPU burners), recorded in the spec's notes; existing tests pass unedited; clippy passes."
    required: true
    manual: false
---

# Flake: fe-workbench sends a second foldingRange request under load: lsp::folds::tests::fold_command_folds_the_servers_range (lsp_folds_tests.rs:106) and lsp::trust_tests::grammarless::a_file_without_a_grammar_goes_to_its_configured_server (lsp_grammarless_tests.rs:117) see 2 requests instead of 1

## Implementation notes

- Cause (C1): `sync_folds` asked as soon as a buffer was drawn, while the server had not yet registered the document (`active_identity` = None). That request still went out once admitted; when the identity then appeared, the slot reset `key`/`task` and asked again, so a second foldingRange request was sent. Unloaded, the first ask is usually cancelled before admission; under load it is admitted first. Not legitimate behaviour (C4 n/a).
- Fix (C2): `sync_folds` skips asking while the identity is None; later edits and a new opening still re-ask.
- Regression (C3): `tests::lsp_folds_single_request::a_document_opened_while_the_server_starts_is_asked_for_once` (server initialize delayed on the fake clock; the request sent before the opening is let through, then draws): deterministic, fails 3/3 at base with 2 requests, passes with the fix.
- Proof (C5): `a_document_opened`, (earlier multi-file variant, 30/30 under 24 burners) `fold_command_folds_the_servers_range`, `a_file_without_a_grammar_goes_to_its_configured_server` run together 30 times under 24 burners: 30/30 passed.
