---
task_uid: "01m67g18nw85g3t4vrb29jpkde"
task: "T377"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The seven caret_keys tests in fe-workbench pass on Linux and keep asserting the same behaviour"
    required: true
    manual: false
---

# Fix the seven Mac T1240 caret-key tests that fail on Linux

Mac main da40996c (merged into helium as the sync commit 0d26579...) fails seven Mac-written tests on Linux; they pass on the Mac's macOS gate, so the Mac cannot see it. All seven came with Mac T1240 (86b9600a, "Find bar, terminal find, search panel, Keyboard Shortcuts filter and inline edit inputs take the TextInput caret keys"):

- tests::caret_keys_hosts::{find_bar,search_panel,terminal_find,inline_edit,shortcuts_editor_filter}_takes_caret_keys
- tests::caret_keys_lines::{vim_line,git_commit_box}_takes_caret_keys

Typical failures: caret_keys_hosts.rs:54 "the input has focus" (search panel), :73 and :120 `pane.find_bar().unwrap()` is None after `press("secondary-f")`. Mac main itself fails the same gate on helium's Linux.

Find why on Linux (a key that is bound differently there, such as secondary-f / secondary-shift-f, a vim or terminal context, or a platform-specific modifier in `press`) and fix the TEST helpers or setup so the tests pass on both platforms and still test the same behaviour. Do not weaken the assertions. Do not change the shipping keymap unless the Linux bug is real in the app; if it is, fix it and say so in the evidence. Keep macOS behaviour unchanged.
