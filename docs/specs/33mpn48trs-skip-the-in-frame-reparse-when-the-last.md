---
task_uid: "01m417z49tjexyt233mpn48trs"
task: "T21"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax: after an incremental reparse slower than the budget, the in-frame parse returns false without reading the text (test syntax::reparse_budget_tests::skips_a_sync_reparse_after_a_slow_one)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax: a later reparse within budget turns in-frame parsing back on, and a first full parse never turns it off (test syntax::reparse_budget_tests::a_quick_reparse_turns_sync_parsing_back_on)"
    required: true
    manual: false
  - id: "C3"
    text: "Existing reparse and highlighting tests pass unchanged, including fe-syntax tests::incremental_reparse_matches_a_full_parse and fe-gpui tests::highlighting_follows_edits (stage test)"
    required: true
    manual: false
---

# Skip the in-frame reparse when the last reparse overran the budget

Source: Paddock T-27 (`118a01f7-d281-4517-b66c-5327f87eb6f6`), P2, draft (needs the user's approval).
Split from T-27: this is the "skip doomed sync reparses" half; the Markdown investigation is T-27b.

## Problem

PERF.md (`cargo bench -p fe-syntax`): reparse time grows with file size, not edit size, about
1.8 ms/MB of Rust in plain tree-sitter too. Above roughly 0.5 MB a keystroke reparse never fits the
view's 1 ms `SYNC_PARSE_BUDGET` (`crates/fe-gpui/src/view.rs`), so every keystroke spends that 1 ms in
`Syntax::parse_within` (`crates/fe-syntax/src/syntax.rs`), gives up, and reparses in the background
anyway. On an 8 MB file that's 1 ms of every typing frame wasted.

T-05 moves the scheduling from `EditorView::sync_syntax` into `SyntaxDriver`
(`crates/fe-syntax/src/driver.rs`), which reparses in-frame through a new
`Syntax::parse_until(out_of_time)`; `parse_within` becomes a wrapper over it that the benches keep
using. This change goes in `parse_until`, so the driver and `parse_within` both honour it.

## Change

- `Parsed` records how long its parse took (`elapsed`, measured with `Instant` around the parse in
  `ParseJob`) and whether it was incremental (`old_tree` was `Some`).
- `Syntax` keeps `last_reparse: Option<Duration>`, set by `Syntax::finish` and by a successful
  in-frame `parse_until` from incremental parses only (a first full parse says nothing about keystrokes), and
  cleared when `edit` drops the tree.
- `Syntax::parse_until` gains a `budget: Duration` parameter beside `out_of_time` and returns `false`
  at once, without parsing, when `last_reparse` exceeds it. `out_of_time` (the driver's injectable
  clock) still decides the in-frame cut-off. `parse_within` passes its budget; `SyntaxDriver::sync`
  (`crates/fe-syntax/src/driver.rs`) passes its own, a one-line change there. The caller then schedules the background job as it does today; that job's `finish`
  refreshes `last_reparse`, so a file that shrinks, or a machine that's no longer loaded, gets sync
  parsing back on the next keystroke.
- Tests in a new module declared from `syntax.rs` as `#[cfg(test)] mod reparse_budget_tests;`
  (`crates/fe-syntax/src/syntax/reparse_budget_tests.rs`). Set `Parsed::elapsed` directly (crate
  visibility) rather than relying on a slow machine. To prove no parse ran, set the existing
  `FAULT` thread-local to `Some("read")` before `parse_within` and assert it's still set afterwards and
  `failed()` is false; reset it at the end.
