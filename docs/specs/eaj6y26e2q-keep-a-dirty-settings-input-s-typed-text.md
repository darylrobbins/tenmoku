---
task_uid: "01m5pd048e880mj3eaj6y26e2q"
task: "T310"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "sync_field and sync_list skip an input whose text differs from field.shown unless forced, so typed text survives a re-render before its blur commits, after a failed write and after an external reload; scalar and list tests"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Keep a dirty settings input's typed text through its blur so a re-render cannot overwrite it (T296 F17)

T296 (Mac T1097) review F17: a dirty input is protected only while focused; GPUI re-renders before the blur callback, so newer typing can be overwritten before commit reads it. T296 documents it as a limit; this card removes it.
