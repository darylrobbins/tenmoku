---
task_uid: "01m77ad95jwptr1fgzznpk9cn1"
task: "T355"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Tests in tests::git_log_b3 (cargo nextest run -p fe-workbench) pass: a constrained-width test renders the production totals row and each separator stays on the same line as its count, and a value that itself starts with the FSI character is not mistaken for a formatter-added isolate (a trailer name containing its own isolate characters is tested)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Git log: constrained-width totals rendering test and FSI-start values (T354 follow-up F2/F4)

# Git log: constrained-width rendering test for the totals, and FSI-start values (follow-up of argon T354, review F2 and F4)

Argon T354 (follow-up of the Mac T1551 catalogue work) renders styled spans from the complete formatted message and keeps each count with its separator. Its C1 is narrowed to what its review round 2 verified; this card is the known limit it names: review finding F2 (medium) and F4 (low).

## Change (`crates/fe-workbench/src/git_log.rs`, `crates/fe-workbench/src/tests/git_log_b3.rs`)

- F2: add a constrained-width test that renders the production totals row at a narrow width and checks that each separator stays on the same line as its count (the grouping that T354 restored), not a helper-level assertion.
- F4: a value that itself starts with the FSI isolate character (git_log.rs near line 584) is mistaken for a formatter-added isolate. Distinguish an exact raw-value match from a formatter-wrapped match, and test a trailer name that contains its own isolate characters.

Platform: **Linux-OK**.
