---
task_uid: "01m4mchzzat2qnm1byq3e2qmar"
task: "T342"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The lane's commits are exactly the 12 listed neon commits, cherry-picked with -x in the listed order, each with its original message, plus the lane's own docs/specs file; git diff of each against its neon original is empty, or the decision file explains each conflict resolution"
    required: true
    manual: false
  - id: "C2"
    text: "No file outside the task's scope changes (the task's own spec under docs/specs/ and its decision record excepted); no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
  - id: "C3"
    text: "The gate's full nextest run passes on macOS"
    required: true
    manual: false
---

# Neon batch 2: cherry-pick 12 finalized neon commits (fe-display, fe-buffer, fe-fileview, fe-lsp tests, app-shell.md)

Neon batch 2: bring 12 finalized neon commits onto Mac main as cherry-picks, the same way batch 1 came
over (e5b46ef). Each was gated on Linux and reviewed on neon's board. They're all outside the crates
kiln is changing (fe-workbench, fe-gpui, fe-config, fe-keymap, crates/fastedit).

The commits are in this repo's object store, under refs/remotes/neon-lanes/main. Cherry-pick them in
this order with `git cherry-pick -x`, one commit each, keeping their messages:

| neon commit | neon task | Mac card | what |
|---|---|---|---|
| 39b6749 | T14 | T278 | fe-display block layout follow-ups |
| 1096373 | T15 | T280 | fe-buffer proposed-change follow-ups |
| 74fe68d | T17 | T284 | fe-lsp tests/registry.rs follow-ups |
| 20dc95c | T18 | T285 | fe-fileview stamp.rs follow-ups |
| aa2370a | T19 | T297 | fe-fileview line index follow-ups |
| 4c51691 | T16 | T283 | fe-buffer dry-run follow-ups |
| 3ec6f4f | T20 | T314 | spec only (findings judged no change) |
| 94e5206 | T13 | T275 | docs/decisions/app-shell.md fixes (kiln K5.7/K5.9/K5.10 build on them) |
| 2c1a71a | T21 | T316 | spec only |
| d3d7809 | T28 | - | flake fix: fe-lsp completion tests count requests before sending |
| e5145c9 | T24 | - | follow-up of T17: fe-lsp tests/registry.rs |
| 129b642 | T27 | - | follow-up of T16: fe-buffer deterministic regression test |

They apply cleanly onto 6686c7c (checked). If one doesn't apply on the lane's base, resolve it toward
the neon commit's intent and say so in the decision file; never drop a commit or add changes of your own.
Not included: neon T12 (fe-workbench lsp.rs, held while kiln works in fe-workbench), T22 and T23 (not
finalized on neon), and neon's regatta.toml lanes_max change (212f4e9, neon-only).

## Implementation notes

All 12 commits cherry-picked with -x in the listed order; every one applied cleanly (no conflict resolutions), and each diff body matches its neon original. No Cargo changes.
