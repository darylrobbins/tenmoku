---
task_uid: "01m4p6btha4243cdgsc81vy1v5"
task: "T50"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::nav_partial::an_empty_partial_answer_says_the_project_is_still_being_indexed: an empty partial answer opens the empty picker with status 'No definition found for `helper` yet: the project is still being indexed (by …)'"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::nav_partial::an_empty_complete_answer_says_none_found keeps 'No definition found for `helper`'"
    required: true
    manual: false
  - id: "C3"
    text: "Every other fe-workbench nav test passes; answers with items show the same status as before"
    required: true
    manual: false
---

# Say the project is still being indexed when search go-to-def finds nothing before its index is complete (Mac T411)

## Problem

PERF.md Navigation (T108, 2026-10-07) has the row "go-to-def, ripgrep fallback: rare name | 95 ms, **0
items, partial**". With the index still building (the first ~2 s of a 10,000-file project, ~9 s on one
thread), the fallback searches the project for a name with one definition. It runs out of its 80 ms budget
(`BUDGET`, `crates/fe-nav/src/search.rs:47`) before reaching the file, so it answers `items: []` with
`partial: true`. PERF.md:624-635 says this is "close to it and wrong", with a "Follow-up to file … or tell the
user it timed out rather than showing an empty answer". Nobody filed it.

The workbench drops `partial`. `Workbench`'s definitions handler (`crates/fe-workbench/src/nav.rs:732-736`)
shows "No definition found for `{word}` (by search)" whether or not the answer was partial. So while the
index builds, the user is told a definition that does exist isn't there. Once the index is complete, the same
request answers in 0.38 ms.

## Change

In `crates/fe-workbench/src/nav.rs`, when the answer has a word, no items, and `partial` set, the status
reads "No definition found for `{word}` yet: the project is still being indexed{by}". The picker still opens
empty, as it does now. A complete empty answer keeps today's text. Answers with items are unchanged.

Regression test: `crates/fe-workbench/src/tests/nav_partial.rs`, declared in `tests/mod.rs`. It adds a
provider through `Workbench::navigation()` that answers `Definitions { word: Some("helper"), items: vec![],
unambiguous: false, partial: true }` ahead of search, presses f12 on `helper`, and checks the new status. A
second test checks that the same answer with `partial: false` keeps "No definition found for `helper`". If
the existing `tests::nav` "No definition" assertion (`tests/nav.rs:233`) turns out to be a partial answer,
update its expected text. Make no other change in that file.
