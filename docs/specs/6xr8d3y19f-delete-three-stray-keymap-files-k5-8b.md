---
task_uid: "01m4nqf1w0dbg3vw6xr8d3y19f"
task: "T33"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The three files named in the spec are deleted from the tree; git ls-files crates/fe-keymap/keymaps lists only real keymap files"
    required: true
    manual: false
  - id: "C2"
    text: "Scope: no other file changes, apart from this task's own spec under docs/specs/ and its decision record; fe-keymap's tests and tests/one_keymap.rs pass unchanged"
    required: true
    manual: false
---

# Delete three stray keymap files K5.8b committed (names ending in ctrl-shift-p, cmd-shift-p, secondary-shift-p) (Mac T353)

# Delete three stray keymap files K5.8b committed

K5.8b (5d146f4) committed three tracked files whose names are a shell slip, not keymaps:

- `crates/fe-keymap/keymaps/default-linux.toml ctrl-shift-p`
- `crates/fe-keymap/keymaps/default-macos.toml cmd-shift-p`
- `crates/fe-keymap/keymaps/fastedit.toml secondary-shift-p`

Each holds a lone `command_palette.toggle` binding with `keys = ""`. Nothing loads them (the keymap
loader and tests/one_keymap.rs read the three real files by exact name).

## Change

`git rm` the three files. Nothing else.
