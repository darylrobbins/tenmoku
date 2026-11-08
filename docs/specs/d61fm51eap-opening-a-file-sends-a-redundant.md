---
task_uid: "01m6x2pp1xybmn0hd61fm51eap"
task: "T527"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "sync_links sends no documentLink request for a buffer whose document is not yet activated (active_identity None), as lsp_colors.rs does"
    required: true
    manual: false
  - id: "C2"
    text: "drawing_before_the_document_activates_asks_for_links_once, in lsp_links_tests.rs, holds activation, draws several times, releases it, waits for the 3 links and asserts exactly one documentLink request; it fails at the base commit"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench lsp::links passes and the vet stage (clippy -p fe-workbench --all-targets -D warnings) passes"
    required: true
    manual: false
---

# Opening a file sends a redundant documentLink request when a draw runs before the server activates the document: lsp::links::tests::a_retry_asks_again (flake)

fe-workbench `lsp::links::tests::a_retry_asks_again` failed main's post-merge check of 731654ce: it timed out after 60.5 s waiting for "the first ask" (tests/lsp.rs:522, called from lsp_links_tests.rs:412). Load was 45-75. Run 01m6wzw9mc4fmv19ew0pm8chnv, test_all log around line 6199. It is the first failure of this wait. T1243 (ef82911c) fixed this test's second wait, not this one.

Cause: this is the documentColor race fixed in T384 (f2bca769), still present in document links. `Workbench::sync_links` (crates/fe-workbench/src/lsp_links.rs:177-232) asks as soon as `server_of(buffer)` is Some. But `active_identity` stays None until the server thread activates the document. Here is what happens when a draw runs before activation:
1. That draw sends ask #1 with identity None (`first`, zero delay).
2. When the identity becomes Some, lsp_links.rs:191-196 resets `slot.key = None`. `moved` is false because the old identity was None, so the next draw sends a second `first` ask.
3. `links_landed` drops ask #1's reply because `identity.is_none()` (lsp_links.rs:317).

The documentLink request is sent twice for one open. a_retry_asks_again waits for `requests(LINKS).len() == 1`. When both requests are recorded between two polls, the count goes from 0 straight to 2, so the wait can never succeed. Ask #2's ContentModified retry then sits on the executor timer, which phase 1 never advances. Waiting longer doesn't help.

Fix: in `sync_links`, skip a buffer whose active identity is None before the slot's key/ask logic, as lsp_colors.rs:104-108 does. The next draw looks again once the document activates. Don't change `links_landed`'s retry handling.

Regression test: add it to the existing crates/fe-workbench/src/lsp_links_tests.rs, not a new module, modelled on lsp_colors_tests.rs `drawing_before_the_document_activates_asks_nothing`:
- before opening src/main.rs, hold activation with the test-support `hold_next_activation`, then `wait_activation_held`;
- draw 5 times, advancing `s.clock` 20 ms each, and assert there are no documentLink requests;
- `release_activation`, wait (until_ticking) for the 3 links, and assert exactly one documentLink request.
At base it fails with 2. Use only base APIs. Leave a_retry_asks_again as it is: it should pass once the cause is fixed.

Not in scope: lsp_folds.rs:83 and lsp_lenses.rs:168 read `active_identity` the same way without the skip. If they have the same race, file follow-ups rather than widening this card.
