---
task_uid: "01m7hd4yabjp6t9ampwpxwgqjk"
task: "T399"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "wide_parent_is_linear checks linearity in a way load can't break: count work (rows or nodes visited, through an existing public API) or take the minimum of several timed samples per size with a ratio bound that holds at load ~30, and still fails if the bracket pass is made quadratic in a wide parent's child count"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax --test brackets passes 30 repeated runs alongside a parallel cargo build"
    required: true
    manual: false
---

# fe-syntax brackets::wide_parent_is_linear flakes under load: it asserts a wall-clock time ratio (Mac T1887)
