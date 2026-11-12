---
task_uid: "01m76s8vesqmw0vc27jx4c0h6z"
task: "T354"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::git_log_b3 (cargo nextest run -p fe-workbench) pass: the regression test drives the production formatting helpers for the trailers, totals and signed counts and verifies their English output, and styled spans are rendered from the complete formatted message with literal whitespace and every placeholder preserved, the count kept with its separator; a constrained-width rendering test of the production totals (review F2) and a value that itself starts with the FSI character (review F4) are a known limit, left to follow-up T355"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Git log: production-path rendering test and styled spans from the whole message (T352 follow-up F1/F2)

# Git log: production-path rendering test and styled spans from the whole message (follow-up of argon T352, review F1 and F2)

Argon T352 (the Mac T1551 follow-up that moves the Git log's remaining text into `git_log.ftl`) is narrowed so that its C1 covers only: the Git log's prose is whole catalogue messages in `git_log.ftl`, and counts are passed as numbers (its review finding F3). This card is the known limit it names: review findings F1 and F2.

## Change (`crates/fe-workbench/src/git_log.rs`, `crates/fe-workbench/src/tests/git_log_b3.rs`, `crates/fe-workbench/l10n/en-US/git_log.ftl`)

- F1: extend the regression test so it drives the production rendering paths (the real trailers, totals and signed-count code, not a stand-in) and verifies their English output.
- F2: render styled spans from the complete formatted message instead of the marker-based reconstruction that alters catalogue formatting: preserve literal whitespace and every placeholder, and keep the styling (colours, runs) and interaction identical; the English text stays byte-equal.

Platform: **Linux-OK**.
