---
task_uid: "01m5rjaq46refem4dzfaxqhs3r"
task: "T335"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test project_panel_clipboard_tests::reopen_keeps_clipboard_after_stale_existence_check: hold a missing-path existence result, recreate the path, reopen the tree (toggle ignored entries) with no new cut/copy; the clipboard is kept"
    required: true
    manual: false
---

# Invalidate pending clipboard existence checks when the project tree reopens and reconcile the clipboard against the newly opened tree (T319 F6)
