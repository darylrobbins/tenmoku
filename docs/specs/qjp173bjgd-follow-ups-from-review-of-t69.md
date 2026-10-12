---
task_uid: "01m4q8nyn3n7m7c3qjp173bjgd"
task: "T70"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T69

Findings of the review of T69 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/lsp.rs:857: User-layer initialization still depends on successfully starting a rooted manager. Rootless windows, failed root resolutions, and reloads during root resolution have no self.lsp.user, so this method silently skips them. Load or reload lsp.enabled=false in a rootless window, corrupt the file, then open and trust a root: the newly created layer has never retained the disabling value and enables servers by default. This leaves the requested app-wide initialization and maintenance incomplete.
  fix: Initialize and retain the app's user layer independently of manager creation and root resolution, and update it on reloads even when the receiving window has no manager. Add a regression covering a valid disabling setting in a rootless window followed by corruption and opening a trusted root.
  instances: crates/fe-workbench/src/lsp.rs:708, crates/fe-workbench/src/lsp.rs:857, crates/fe-workbench/src/lsp.rs:950

- F2 medium crates/fe-workbench/src/lsp.rs:950: Maintaining the user layer only through delivered effective-settings reloads misses valid user edits masked by project settings. SettingsStore::reload suppresses events when effective settings and diagnostics are unchanged, and fastedit's give also suppresses equal settings. For example, start untrusted with user enabled=true and project enabled=false, change the user to enabled=false, corrupt the user file, remove the project's disabling key, then trust the root. The valid user edit generated no delivered reload, so the retained user layer still enables servers. The new tests call reload_settings directly and bypass this suppression.
  fix: Update the shared validated user layer from user-file changes or the underlying validated user-layer reload independently of effective-settings change delivery. Add an integration regression covering a disabling edit masked by a project disabling setting, subsequent corruption, removal of the project override, and trust.
  instances: crates/fe-workbench/src/lsp.rs:950
