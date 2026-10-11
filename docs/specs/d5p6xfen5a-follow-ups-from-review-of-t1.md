---
task_uid: "01m4p59j913bqvagd5p6xfen5a"
task: "T10"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1

Findings of the review of T1 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-terminal/src/session.rs:254: The cwd validation rejects missing paths and files but still accepts directories the user cannot enter. metadata().is_dir() succeeds without search permission on the directory; Alacritty then ignores the failed chdir and runs the command in the app's inherited directory. This leaves the unusable-directory case of F3 unresolved.
  fix: Check directory search permission using an OS access check without changing the app's cwd, return PermissionDenied when inaccessible, and add a regression test using a directory without search permission.
  instances: crates/fe-terminal/src/session.rs:254

- F6 medium crates/fe-terminal/src/session.rs:207: A pre-existing output-loss case remains for unfinished synchronized updates. A child that prints ESC[?2026h followed by text and immediately exits leaves that text buffered in VTE. Alacritty drains the PTY but exits without flushing this parser buffer, so Exited arrives with the text permanently absent from the snapshot. Shutdown and error cleanup also discard the buffered state.
  fix: Add a regression covering exit during an unfinished synchronized update and arrange an upstream-supported parser-buffer flush before publishing completion and discarding event-loop state.
  instances: crates/fe-terminal/src/session.rs:207, crates/fe-terminal/src/session.rs:304
