---
task_uid: "01m67vc6q60eg573s2rhmfcfd0"
task: "T260"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test git_log_ref_pills_tests::lane_colour: a branch pill's colour is its commit's lane colour (GRAPH_COLORS by GraphRow::color), the same as its dot"
    required: true
    manual: false
  - id: "C2"
    text: "git_log_ref_pills_tests::kinds: local branch, remote-tracking branch (cloud icon) and tag (tag icon) render differently; HEAD's pill is filled, the others tinted"
    required: true
    manual: false
  - id: "C3"
    text: "git_log_ref_pills_tests::overflow: more than 3 pills on one row collapse to the first 3 and '+N'; the card from T1221 lists all of them"
    required: true
    manual: false
  - id: "C4"
    text: "existing git_log tests pass; cargo test -p fe-workbench passes; clippy clean"
    required: true
    manual: false
---

# Colour a Graph commit's ref pills by kind and by their branch's lane, with HEAD filled, remote refs on a cloud icon and tags on a tag icon (Mac T1222)

## Problem
The user: "Colors around the branch". Lanes already use VS Code's scmGraph colours; `ref_label` pills are one neutral style. VS Code colours each ref by its branch colour.
