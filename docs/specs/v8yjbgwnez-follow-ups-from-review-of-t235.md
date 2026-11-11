---
task_uid: "01m71dydg7w4hm5mv8yjbgwnez"
task: "T316"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T235

Findings of the review of T235 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-gpui/src/element/sticky_scroll.rs:470: Sticky-scroll headers bypass the new mode and always draw absolute numbers whenever the gutter exists. With sticky scroll enabled by default, scrolling inside a scope still displays header numbers after editor.line_numbers="off" or :set nonu nornu; relative modes also retain absolute header numbers.
  fix: Apply the editor's line-number mode to sticky-scroll numbering, preferably sharing the number calculation with normal layout, and test pinned headers in all four modes.
  instances: crates/fe-gpui/src/element/sticky_scroll.rs:470

- F3 medium crates/fe-gpui/src/diff_view.rs:215: Resetting diff-side numbering only during construction does not preserve the required absolute numbers. Both sides are ordinary Vim-enabled EditorViews, so :set nonu rnu subsequently switches a diff side to relative numbering, and nothing restores its absolute mode.
  fix: Enforce absolute numbering throughout the diff-view lifecycle or prevent these options from changing its rendered numbering. Add a test that runs the new :set options inside a diff side.
  instances: crates/fe-gpui/src/diff_view.rs:215
