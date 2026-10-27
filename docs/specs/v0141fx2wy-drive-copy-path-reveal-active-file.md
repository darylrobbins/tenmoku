---
task_uid: "01m5xjcswt6r3kqev0141fx2wy"
task: "T370"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Drive Copy Path / Reveal Active File chords through GPUI key input (T359 F1)

Follow-up to T359 (Mac T1188) review F1. The tests added in fe-gpui keymap_tests and fe-workbench default_keymaps inspect the parsed bindings; the reviewer wants one that drives the chords through GPUI key input: focus an editor in a Workbench window, simulate `secondary-k p` and `secondary-k r`, and check the copied path on the clipboard and the reveal seam's recorded path. Test-only; use the existing path_commands_tests helpers (reveal seam, clipboard read).
