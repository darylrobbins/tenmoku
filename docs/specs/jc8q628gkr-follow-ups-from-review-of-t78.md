---
task_uid: "01m490xvztzqt2tqjc8q628gkr"
task: "T121"
rev: 4
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F3: with a Rust outline shown, switching the editor to Python and back to Rust while the Python outline job is pending leaves the Rust outline displayed once the job lands; a job's result is installed only if it matches the editor's current language, and the panel reconciles the current syntax when a job completes even if no refresh was deferred (deterministic test)"
    required: true
    manual: false
  - id: "C2"
    text: "New tests live in crates/fe-workbench/src/tests/outline_panel_followups.rs and fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T78

Findings of the review of T78 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-workbench/src/outline_panel.rs:201: A pending job can permanently install the wrong language's outline. Starting with a Rust outline, start a Python outline job without editing, then switch back to Rust before it lands. Once Rust reparses, refresh returns because the retained Rust outline already matches, without setting dirty. The Python result is then accepted based only on editor identity; behind and dirty are false, so it remains displayed until another editor notification.
  fix: Validate the result against the editor's current language or language generation before installing it, and reconcile the current syntax when a job completes even if dirty is false. Add a deterministic regression test that switches languages and back while the intermediate language's job is pending.
  instances: crates/fe-workbench/src/outline_panel.rs:146, crates/fe-workbench/src/outline_panel.rs:201


Triage (2026-10-06, main a7975bd): F3 confirmed still present (refresh returns early on `shows` without setting dirty; `landed` checks only the editor id). Kept. Scope narrowed to outline_panel.rs plus a new test file; no bindings are added.
