---
task_uid: "01m55bq2e8gj0444711v0bra3g"
task: "T147"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "crates/fe-buffer/benches/buffer.rs has anchors/resolve 10k through {10,100,1000} edits, one by one and batched, the 100-edit names unchanged, and cargo bench -p fe-buffer -- --test runs them"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md's Buffer section has the 10- and 1,000-edit rows (fastest samples of 3 rounds, machine, commit, date, load), the 1000/100 and 100/10 ratios per mode, whether each is at most 12x, and the 'needs a second edit count, which isn't filed' sentence replaced by the result; a ratio over 12x names a follow-up in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "Only buffer.rs and PERF.md change"
    required: true
    manual: false
---

# Bench anchor resolution through 10, 100 and 1,000 edits and check M1's 'scales with the edits since the anchor' as a ratio (Mac T817)

## Problem

M1's buffer budgets (header of `crates/fe-buffer/benches/buffer.rs`) include "anchor resolution scales with
the edits since the anchor". The bench has one edit count: `anchors/resolve 10k through 100 edits, one by
one` and `..., batched`. PERF.md's Buffer section says so: "The bench uses a single edit count, so it shows
the cost is small but not that it scales with the edits since the anchor; that needs a second edit count,
which isn't filed." Diagnostics and AI proposal anchors live through thousands of keystrokes, so a
superlinear resolve would show up as typing slows down over a session.

## Change

In `crates/fe-buffer/benches/buffer.rs`, make the anchors section a loop over edit counts 10, 100 and 1,000
(the same 10k anchors, made before the first edit, and the same random single-letter edits, so the 100 case
keeps today's shape). Case names: `anchors/resolve 10k through {n} edits, one by one` and `..., batched`,
so the 100-edit names stay as they are.

Run `cargo bench -p fe-buffer -- anchors/` (release, private `CARGO_TARGET_DIR`), 3 rounds, and take each
case's fastest sample from `new/sample.json`.

Budget, the header's claim made a number: for each mode, the 1,000-edit case costs at most 12x the 100-edit
case (linear with 20% slack), and the 100-edit case at most 12x the 10-edit case. If either ratio is over,
say which mode and by how much, and name a follow-up in the decision file; don't change fe-buffer here.

Update the header's anchor line to name the three edit counts. In PERF.md's "Buffer
(`cargo bench -p fe-buffer`)" section, add the 10- and 1,000-edit rows to its table next to the 100-edit
ones (machine, commit, date, load as that section gives them), state the two ratios per mode and whether
the claim holds, and replace the "needs a second edit count, which isn't filed" sentence with the result.
