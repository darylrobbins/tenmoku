---
task_uid: "01m4sjp1d4a8w9vh7d9xj8jpny"
task: "T83"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "kiln-shell test interrupted_g_prefix_is_dropped (tests/picker_vim_g_prefix.rs): from the last match in normal mode, g left g, g ctrl-x g, and g then a propagated non-picker command then g each leave the last match selected, while g g still selects the first"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-shell tests picker_vim and fe-workbench picker_vim pass unchanged; cargo test -p kiln-shell -p fe-workbench and cargo clippy -p kiln-shell --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Drop the picker pending g prefix on any other keystroke or host command (T72 review F1) (Mac T596 follow-up)

Follow-up from T72 (Mac T596, picker vim keys, landed as f5488dc), filed on helium. Review 01m4she15wn4j9h86jwzmk5pnv
(`rgt review show T72`), finding F1.

## Problem

In `kiln/crates/kiln-shell/src/picker.rs`, the pending `g` prefix (`pending_g`) is cleared only for a printable,
unmodified keystroke in normal mode (:307), or when a picker command runs (:261). The other paths return before
clearing it:

- the modified-key and no-`key_char` returns in `on_key_down` (:301, :304)
- the `cx.propagate()` for commands that aren't the picker's, in `on_run_command` (:257)

So from the last match, `g`, `left`, `g` jumps to the first match. The spec says anything other than a second `g`
drops the prefix.

## Change

In picker.rs only: in normal mode with the switch on, every keystroke other than a second `g` drops `pending_g`.
That includes modified keys, keys without a char, and escape (`on_key_down` clears it before its early returns).
A command that isn't the picker's also drops it before propagating to the host (`on_run_command`, before
`cx.propagate()`). Nothing else changes.

## Tests

Add a new integration test `kiln/crates/kiln-shell/tests/picker_vim_g_prefix.rs`. Copy the setup it needs from
`tests/picker_vim.rs`: a `Words` picker with the switch on, in normal mode, the last match selected.

- `interrupted_g_prefix_is_dropped`: `g left g`, `g ctrl-x g`, and `g` then a command dispatched to the picker
  that isn't the picker's (it propagates to the host), then `g`. Each leaves the last match selected. A plain `g g`
  still selects the first.
