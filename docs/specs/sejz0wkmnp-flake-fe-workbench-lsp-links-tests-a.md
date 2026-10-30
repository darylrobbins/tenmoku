---
task_uid: "01m654hxwjq10713sejz0wkmnp"
task: "T1243"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Cause: under load (T1229 gate, 2026-10-29, while another lane built) the test's until-loop gave up after 60 s with 'timed out waiting for the links'; alone it passes 5/5 in under 1 s. Find what the retry waits on (a retry timer advanced by the fake clock, an executor that never parks, a reply consumed before the retry is armed) and state it in the spec's notes; T1242 (a second foldingRange request under load) may share a cause in the fake-LSP harness: check and say."
    required: true
    manual: false
  - id: "C2"
    text: "Fix the test (deterministic clock/executor advancing) without weakening what it checks: a failed links request is asked again and the retry's links are shown. If the cause is in production code (the retry is not armed or is lost under load), block and tell the lead: it becomes a bugfix with a regression test."
    required: true
    manual: false
  - id: "C3"
    text: "Proof: the test passes 30 times in a row under load (20+ CPU burners), recorded in the spec's notes; existing tests pass unedited; clippy passes."
    required: true
    manual: false
---

# Flake: fe-workbench lsp::links::tests::a_retry_asks_again times out after 60 s waiting for the links under load (tests/lsp.rs:522)

## Implementation notes

- C1 cause: the retry waits on a 500 ms `REST` timer on the executor's clock (`lsp_links.rs`, `background_executor().timer`). The timer is armed only after the error reply is handled. The test advanced that clock once (510 ms), right after it saw request 1 recorded. Under load the reply was still unhandled then, so the timer was armed after the one advance and nothing ever advanced the executor clock again (`until_ticking` advances only the manager clock). The loop spun until the 60 s bound. Production code is fine: the retry is armed, not lost.
- T1242 (second foldingRange request) is a different symptom (too many requests, not a hang) and the folds test never waits on the executor clock, so no shared cause was found there. Its extra request is more likely from the 20 ms-per-poll manager clock ticking a debounce while the first answer is slow.
- C2 fix: the test advances the executor clock on every poll of the until-loop, so the timer fires whenever it is armed. It still checks that the failed request is asked again (2 requests) and that the retry's three links are shown.
- C3 proof: 30/30 passes with 24 CPU burners running (load average about 39); clippy clean.
