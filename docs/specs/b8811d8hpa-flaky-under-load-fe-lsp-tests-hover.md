---
task_uid: "01m4kkr3brx0936jb8811d8hpa"
task: "T9"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp.tests::hover::hover_replies_convert passes in the gate, with its existing assertions unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "In hover_replies_convert, both counts of the fake's hovers (before wait_for(\"the hover\") and before wait_for(\"the late hover\")) are taken before the matching s.manager.hover call, so a request the fake records first can't already be counted"
    required: true
    manual: false
  - id: "C3"
    text: "3 consecutive runs of the test (nextest -E 'test(=tests::hover::hover_replies_convert)') pass, with each run's 1-min load logged in the decision file (the 10-runs-under-load proof was waived by the user on 2026-10-10: the sandbox refuses loaded test loops here, and the post-merge checks carry the load evidence)"
    required: true
    manual: false
  - id: "C4"
    text: "Only crates/fe-lsp/src/tests/hover.rs changes, and nothing in crates/fe-lsp/src/tests/fake.rs (its 10-s WAIT stays) or fe-lsp's production code"
    required: true
    manual: false
---

# Flaky under load: fe-lsp tests::hover::hover_replies_convert: count the fake's hovers before sending each one, so a request that lands first isn't already counted

## Problem

`fe-lsp.tests::hover::hover_replies_convert` failed in four gate runs on neon, with the 1-min load at 30-45 on
12 cores: T1's post-merge run `01m4kesk6n0t1sqhbgvq0sc1z1` (10.70 s), its retry `01m4kf55zz05xbhgmwt6m2rvef`
(10.15 s), the reverted-tip check `01m4kfkfmq84h1s4fhjs8kysw7` (10.04 s), and T3's finish gate
`01m4kjntrf1pyx0ska8v9aaftb` (10.12 s). Each time it panicked at `crates/fe-lsp/src/tests/fake.rs:433` with
`fake: timed out waiting for the hover`, which is the fake's 10-s `WAIT`. Run alone, it passes in about 0.1 s.
The stage logs are `test.fastedit.log` in those runs' directories under `~/.local/state/regatta/runs/`.

The cause is the test's ordering, not the length of the wait and not fe-lsp. In
`crates/fe-lsp/src/tests/hover.rs` (~182), the test sends the delayed hover and only then counts the hovers
the fake has seen:

```rust
let asked = s.manager.hover(buffer.id(), buffer.version(), beta);
let count = hovers(&s.fake.records()).len();
s.fake.wait_for("the hover", |records| hovers(records).len() > count);
```

`Manager::hover` (`crates/fe-lsp/src/hover.rs` ~61) sends the request through `document_request` straight
away; the future it returns only waits for the reply. Alone, the test thread counts before the request
reaches the fake. Under load, the test thread is preempted after `hover` returns, the fake records the
request first, and `count` already includes it. The wait then needs a hover that is never sent, and it runs
out its 10 s. The logs prove it: every failure's record dump already ends with that hover request, at
character 17 in the UTF-8 pass (three runs) or 15 in the UTF-16 pass (`01m4kjntrf1pyx0ska8v9aaftb`), which is
`beta`, not the `beta + 1` the earlier asks use. No 10-s wait would fix this.

The late hover (~203-210) has the same shape: it sends with `s.manager.hover(..., beta + 1)` and only then
takes `count`, so it can lose the same race at `wait_for("the late hover", ...)`.

This is the same kind of race as T7 (a step taken before the state it depends on is read), on the test's
side only. In production nothing counts requests.

## Change

Test-only, in `crates/fe-lsp/src/tests/hover.rs`, `hover_replies_convert`. In both places, take `count`
before the request is sent:

- Move `let count = hovers(&s.fake.records()).len();` above
  `let asked = s.manager.hover(buffer.id(), buffer.version(), beta);`.
- Move the second `let count = ...` above `let mut late = s.manager.hover(...).boxed();`.

Add a one-line comment at the first: `hover` sends at once, so count before asking or the request may
already be counted. Change no assertion, and change nothing in `fake.rs` (its 10-s `WAIT` stays) or in
fe-lsp's production code.
