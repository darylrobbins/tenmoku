---
task_uid: "01m4m2vfvkcdv4ecvzx3ne7tqr"
task: "T28"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp.tests::completion::resolve_triggers_and_timeouts passes in the gate, and what it asserts is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "In expect_timeout and its callers, the fake's request count is taken before resolve()/completion() is called, for both the resolve and the completion timeouts"
    required: true
    manual: false
  - id: "C3"
    text: "3 consecutive runs of the test (nextest -E 'test(=tests::completion::resolve_triggers_and_timeouts)') pass, with each run's 1-min load average logged in the finish notes"
    required: true
    manual: false
  - id: "C4"
    text: "Only crates/fe-lsp/src/tests/completion.rs changes; nothing in crates/fe-lsp/src/tests/fake.rs (its 10-s WAIT stays)"
    required: true
    manual: false
---

# Flaky under load: fe-lsp tests::completion::resolve_triggers_and_timeouts: count the fake's requests before sending, like T9

## Problem

`fe-lsp.tests::completion::resolve_triggers_and_timeouts` fails under load on neon. It failed in T16's
refresh gate `01m4m27611beee6yh3wg2bz8fs` (757bfe8 on 20dc95c, 1332/1333 passed). It also failed in six
earlier runs on bases 06af853, 39b6749, 74fe68d and 5f16e7f, and in another candidate's gate on 20dc95c.
It passed in main's post_merge run on 20dc95c. Each failure panicked at `crates/fe-lsp/src/tests/fake.rs:433`
with `timed out waiting for completionItem/resolve` (the fake's 10-s `WAIT`). Run alone it passes in about
0.15 s. In 3 back-to-back runs at load 25-27 on 12 cores, it failed once and passed twice.

The cause is the test's ordering, the same race T9 fixed in `tests::hover::hover_replies_convert`.
`expect_timeout` (`crates/fe-lsp/src/tests/completion.rs` ~344) counts the fake's `method` requests
first, then waits for one more. But its callers create `late` before calling it. `Manager::resolve`
(`crates/fe-lsp/src/completion.rs` ~143) builds its request through `request_if` when it is called, not when
the future is polled. So the comment "The request goes out once `late` is first polled" is wrong for the
resolve case, and it may be wrong for `completion` too. Under load the fake records the request before
`count` is taken, and the wait needs a request that is never sent. Every failure's record dump already ends
with that resolve request.

## Fix

Count the requests before the request can be sent. For example, `expect_timeout` takes a closure that
starts `late` and takes `count` before calling it, or the callers pass in a count they took before calling
`resolve`/`completion`. Fix both callers (resolve ~285, completion ~304). Change only the test file. Leave
`fake.rs` and its 10-s `WAIT` alone, and don't change what the test asserts.
