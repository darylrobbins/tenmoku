---
task_uid: "01m5pawpg79dtqmyb7cgwycz17"
task: "T308"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "On a workspace-root change the tab drops every uncommitted row input, list input and its blur subscription; a test that a root change with an uncommitted input does not write to the new root"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Drop row inputs, list inputs and blur subscriptions when the settings root changes (T296 F16)

T296 (Mac T1097) review round 3 F16: a root change keeps uncommitted inputs and their blur subscriptions. T296 documents it as a limit; this card removes it.
