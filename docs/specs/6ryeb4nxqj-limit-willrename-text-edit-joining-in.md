---
task_uid: "01m5rjaqcpw3yek76ryeb4nxqj"
task: "T336"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test lsp_file_operations_tests::will_rename_edits_around_an_overwrite_create_file: a willRename edits the same child before and after an overwrite CreateFile; both edits apply in order and the file ends with the new contents"
    required: true
    manual: false
---

# Limit willRename text-edit joining in lsp_file_operations to runs between resource operations (T319 F7)
