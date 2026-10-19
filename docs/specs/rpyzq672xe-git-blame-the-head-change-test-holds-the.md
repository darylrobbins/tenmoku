---
task_uid: "01m58v8padmnc5gcrpyzq672xe"
task: "T209"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench unit test workbench::git_blame::git_blame_tests::head_race::old_head_blame_landing_after_a_head_change_is_revalidated passes: a tracked file's blame is captured at the old HEAD with its completion held, HEAD changes and validation is requested, the completion is released, and the annotation shown is the new HEAD's with no further refresh; it fails at the base or documents the defect it exposed and fixes it in git_blame.rs"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (git_blame_tests.rs only gains the #[path] mod line); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Git blame: the head-change test holds the old-HEAD blame, changes HEAD, releases it and sees the revalidated annotation (T135 review F3)

## Problem

T135 (cancel blame and diff work superseded by Save As, HEAD changes, caret moves or pane focus round trips) landed on a user findings_override with this review finding (codex, medium, F3, `crates/fe-workbench/src/git_blame_tests.rs:~1220`) open and its test deferred by a relaxed criterion: the deferred-completion test only blames an untracked file twice, without changing HEAD or checking an annotation. Both HEAD-change tests amend HEAD before starting the pending blame and then cancel it. Nothing exercises an old-HEAD blame landing after HEAD changed, followed by the automatic revalidation.

## Change

Add a test with controlled ordering: capture a tracked file's blame at the old HEAD, hold its completion, change HEAD and request validation, release the completion, and verify the annotation shown is the new HEAD's without another refresh. If the test finds a real defect in `git_blame.rs` (the old-HEAD result replaces the revalidated one, or validation is dropped while blame is pending), fix it there.

New tests go in a new file `crates/fe-workbench/src/git_blame_head_race_tests.rs`, declared from `git_blame_tests.rs` with one `#[path = "git_blame_head_race_tests.rs"] mod head_race;` line.

## Implementation notes

The test found no defect: a HEAD check requested while the old-HEAD blame is pending sets `recheck`, and the landing blame re-reads HEAD and blames again. The test needed a way to hold a blame's completion, so `git_blame.rs` gains a test-only `BlameState::hold` oneshot the next blame awaits after reading.
