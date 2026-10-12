---
task_uid: "01m4q5k6gvm0b8ek520k23epd1"
task: "T67"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T65

Findings of the review of T65 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-workbench/src/lsp.rs:830: User-file retention still fails outside the launch root. fastedit's settle() constructs a fresh SettingsStore for those windows. Corrupting a user file containing lsp.enabled=false produces a diagnostics event from the persistent app store, but settle() then delivers defaults to secondary windows. Applying the retained project table over those defaults can launch servers. The new test supplies settings from a persistent store directly, bypassing this delivery path.
  fix: Preserve the validated user layer for every window independently of fresh host reads, and test invalid user-file reloads through secondary-window settings delivery.
  instances: crates/fe-workbench/src/lsp.rs:830

- F4 medium crates/fe-config/src/settings.rs:485: The trusted layer is merged over settings already containing narrowed project values. In a trusted secondary root, removing lsp.enabled=false or a server's enabled=false leaves the old disabling value in lsp.settings; its watcher cannot restore the user's value because the app watches only the launch project's filtered layer. This also defeats project retention when an invalid command is accompanied by a changed disabling flag: the filtered store accepts that flag while ProjectLsp rejects the file.
  fix: Merge the validated project layer over retained user settings and defaults, keeping filtered effective settings separately for untrusted operation. Cover removal of both disabling flags and invalid project edits that change them.
  instances: crates/fe-config/src/settings.rs:485, crates/fe-config/src/settings.rs:418, crates/fe-workbench/src/lsp.rs:830

- F5 medium crates/fe-workbench/src/lsp.rs:750: watch_project() locks ProjectLsp on the UI thread, while background reads hold that mutex throughout filesystem access and validation. Untrusting and retrusting during an outstanding read therefore blocks the UI until that read finishes, potentially indefinitely on a stalled filesystem.
  fix: Obtain the immutable project path without acquiring the read-state mutex, or obtain it on the background executor.
  instances: crates/fe-workbench/src/lsp.rs:750

- F6 medium crates/fe-workbench/src/lsp.rs:768: The foreground task owns ConfigWatcher after awaiting its construction. Canceling that task during untrusting, root replacement, or view release drops the watcher on the UI thread. ConfigWatcher::drop joins its thread, which can be performing blocking filesystem reconciliation, so these transitions can freeze the application.
  fix: Keep watcher ownership and destruction on the background executor, with a cancellation signal that the foreground task can release without waiting.
  instances: crates/fe-workbench/src/lsp.rs:768, crates/fe-workbench/src/lsp.rs:612, crates/fe-workbench/src/lsp.rs:724
