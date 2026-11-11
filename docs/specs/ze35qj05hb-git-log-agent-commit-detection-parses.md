---
task_uid: "01m75pyd7w2p2ewsze35qj05hb"
task: "T351"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Tests in tests::git_log_b3 (cargo nextest run -p fe-workbench) pass: a commit whose last paragraph is prose quoting an agent address is not flagged as an agent's, and the same address in a real Co-Authored-By trailer is"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Git log: agent-commit detection parses the trailer block (Mac T1551 follow-up F2)

# Git log agent-commit detection parses the trailer block (follow-up of Mac T1551, review F2)

Mac T1551 (argon T348) marks an agent's commit, from provenance where known, by matching recognised agent addresses in the commit message's final paragraph. Prose in that last paragraph that merely quotes an agent address is then flagged falsely. This card makes the detection parse the actual trailer block.

## Change (`crates/fe-workbench/src/git_log.rs`, `crates/fe-workbench/src/tests/git_log_b3.rs`)

Match recognised agent addresses only in the commit message's real trailer block (the final block of `Key: value` lines, as `git interpret-trailers` defines it), never in free prose. Add a regression test where the last paragraph is prose that quotes an agent address and the commit is not flagged, and one where the same address in a real `Co-Authored-By:` trailer is.

Platform: **Linux-OK**.
