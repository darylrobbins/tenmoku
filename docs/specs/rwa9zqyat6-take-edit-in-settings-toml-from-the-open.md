---
task_uid: "01m5pd04h6k7qq84rwa9zqyat6"
task: "T311"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "When the settings file is open in a pane, Edit in settings.toml computes the table row from that editor's model text (pane lookup by path), not from disk; a test with unsaved edits"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Take 'Edit in settings.toml' from the open editor's text when the file is open (T296 F18)

T296 (Mac T1097) review F18: Edit in settings.toml computes the table row from disk, but open_at reuses an open editor with unsaved edits. T296 documents it as a limit; this card removes it.
