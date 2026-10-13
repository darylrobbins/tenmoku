---
task_uid: "01m4sjp4k38m1hva98whb7a3x5"
task: "T89"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui test vim_key_fallback_tests::shift_tab_jumps_newer_in_normal_mode: with the macOS and the Linux default keymap, shift-tab in normal mode after a jump and ctrl-o returns to the newer position and edits nothing"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test vim_key_fallback_tests::modified_arrows_move_visual_selections: with each platform keymap, in v, V and ctrl-v, alt-down and the duplicate-row shortcut (shift-alt-down, ctrl-shift-alt-down on Linux) extend the selection a row down and alt-up moves it back, keeping the mode and the buffer unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "only the new test file and its mod line in lib.rs change; cargo test -p fe-gpui and cargo clippy -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Test vim fallback keys through real key input: shift-tab jumps newer and modified arrows move visual selections on both keymaps (T68 review F1) (Mac T462 follow-up)

Follow-up from T68 (Mac T462, keys and menus, landed as caf5f3f), filed on helium. Review 01m4sem4wc0x3wpen349g29q48
(`rgt review show T68`), finding F1, which was still not fixed after round 2.

## Problem

T68 fixed the binding contexts so that vim's normal, operator-pending and visual modes leave `shift-tab`, `alt-up`,
`alt-down` and the duplicate-row shortcut unbound. vim's own key handler reads these: shift-tab as tab, and alt-up
and alt-down as up and down. The tests T68 shipped only check keymap lookup
(`crates/fe-keymap/tests/editing_bindings.rs` :156 `vim_keeps_its_keys`, :199
`vim_modes_keep_the_keys_its_handler_reads`). They accept "no binding" without checking that the key reaches vim
and does what vim does. The reviewer asked for "tests through the GPUI key-input path proving Shift-Tab invokes
vim.jump_newer in normal mode and modified arrows move the selection in every visual mode, using both platform
keymaps and their duplicate-row shortcuts."

## Change

Test only: add the coverage through real key input, with no production change. If a test shows the behaviour
is wrong, stop and report it, because the fix needs a bugfix card with production files in scope.

## Tests

Add a new file `crates/fe-gpui/src/vim_key_fallback_tests.rs`, declared from `crates/fe-gpui/src/lib.rs` with
`#[cfg(test)] #[path = "vim_key_fallback_tests.rs"] mod vim_key_fallback_tests;`. Build the editor as
`crate::vim::tests` does (a focused window, vim on). Run each test with `DEFAULT_MACOS` and again after
`init_with_keymap(cx, <DEFAULT_LINUX keymap>)`, and press keys with `cx.simulate_keystrokes`:

- `shift_tab_jumps_newer_in_normal_mode`: after a jump (for example `G`) and `ctrl-o`, `shift-tab` returns to the
  newer jump position, as `tab` does, and the buffer is unchanged.
- `modified_arrows_move_visual_selections`: in `v`, `V` and `ctrl-v`, `alt-down` and the platform's duplicate-row
  shortcut (macOS `shift-alt-down`, Linux `ctrl-shift-alt-down`) extend the selection down a row, and `alt-up`
  moves it back up. The mode is kept, and the buffer is unchanged (no rows moved or duplicated).
