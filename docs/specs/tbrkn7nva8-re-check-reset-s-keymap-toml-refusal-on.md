---
task_uid: "01m5pzc5ky8d8xrptbrkn7nva8"
task: "T314"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Reset refuses on the source it is about to edit, not the cached one: a gated unbind saved after the menu opened is refused and keymap.toml is untouched. Tests reset_rechecks_refusal_on_fresh_source, reset_refusal_leaves_file_untouched"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Re-check Reset's keymap.toml refusal on the text actually edited (T299 F8)

## Problem
T299 review round 6 finding F8: Reset's "Edit keymap.toml" refusal (a gated unbind it cannot attribute to a default) is checked on the tab's cached `user_source`, but `write_keymap_on` rereads the file, so a gated unbind saved between opening the menu and the click slips past the refusal.

## Change
- Apply the refusal check to the text actually edited: add an fe-keymap entry point that edits a given source (or re-checks in `write_keymap_hooked`'s retry), and have the Reset worker read the file, re-run the refusal on that text and pass the same text through.

## Tests
`crates/fe-workbench/src/shortcuts_editor_reset_race_tests.rs` (declared with `#[path]`) and `crates/fe-keymap/tests/`: a gated unbind written after the menu opened is refused at click time and the file is untouched.
