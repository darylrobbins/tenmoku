---
task_uid: "01m5pt7avcj0qmxbht9qqs4vv6"
task: "T312"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Blur commits an input that was edited (Changed event since the last commit or sync) even when its final text equals the value it showed: clearing an unset tab_size, typing its default 4 and blurring writes the override; an inherited list retyped to the same items and blurred writes it; blur with no edit writes nothing. Tests blur_after_retyping_the_default_writes, blur_after_retyping_an_inherited_list_writes, blur_without_edit_writes_nothing"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Commit a blur on an edited settings input even when the text equals its shown value (T296 F19)

## Problem
T296 (Mac T1097) review round 4 finding F19: the Settings tab's number, text and list inputs commit on blur only when the final text differs from what the input first showed. Clearing an unset `tab_size`, retyping its default `4` and clicking away therefore writes nothing, while Enter writes the override; likewise an inherited list that is retyped to the same items and blurred. Enter and blur should agree, and "setting a value equal to its default writes it" should hold for blur too.

## Change
- `crates/fe-workbench/src/settings_editor_controls.rs`: keep an `edited` flag per input Field, set on `TextInputEvent::Changed`, cleared on commit and on sync from the model. Blur commits when the flag is set, not when the text differs from the shown value. Same for list item inputs.

## Tests
New `crates/fe-workbench/src/settings_editor_controls_blur_tests.rs` (declared with `#[path]`): retype an unset number's default then blur writes the override; retype an inherited list then blur writes it; blur without any edit writes nothing.
