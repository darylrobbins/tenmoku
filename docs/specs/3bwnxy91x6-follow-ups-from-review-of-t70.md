---
task_uid: "01m4r07t9frqbtey3bwnxy91x6"
task: "T71"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T70

Findings of the review of T70 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp_trusted_settings_tests.rs:477: The requested rootless initialization regression first creates a rooted workbench through Lsp::untrusted, which initializes the shared user layer before the rootless window exists. File changes and the final trust operation also target that rooted workbench. Consequently, this test can pass if rootless windows merely reuse an existing layer but cannot initialize or maintain one themselves—the central scenario from T69 F1 remains untested.
  fix: Create an app whose first and only workbench is rootless, read a valid disabling user setting, corrupt the file, then open and trust a rooted workbench and assert that its manager remains disabled. Initialize the test configuration and launcher without first constructing a rooted workbench.
  instances: crates/fe-workbench/src/lsp_trusted_settings_tests.rs:477
