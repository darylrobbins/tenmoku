---
task_uid: "01m5vfmn48vmvabs92c65w13z5"
task: "T357"
rev: 2
type: "bugfix"
complexity: "s"
criteria: []
---

# Remap selections of every editor sharing the buffer across workbenches on encoding reopen (T348 F14)

Follow-up to T348 (Mac T1161) review F14. Reopen/Save with Encoding remaps selections only for editors in this workbench. Editors sharing the buffer in other windows (and diff working sides) lose their selections. Collect and remap every editor sharing the buffer across workbenches before applying the re-decode, with a test using two workbenches on one buffer.
Scope: crates/fe-workbench/src/encoding_picker.rs and its tests.
