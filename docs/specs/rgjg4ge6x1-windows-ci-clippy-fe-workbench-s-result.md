---
task_uid: "01m7qmkcw3xvyr10rgjg4ge6x1"
task: "T1954"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "On a non-unix target none of the named lints fire: move_one's Err is boxed or carries a targeted allow with a reason; the libc::kill tests in chat_tool_loop_tests.rs are #[cfg(unix)]; the Debuggee import, the workbench/cx bindings and the mut are cfg-gated or allowed with a reason; unix behaviour is unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "cargo clippy -p fe-workbench --all-targets -- -D warnings and cargo nextest run -p fe-workbench for the touched tests pass on the worker's host"
    required: true
    manual: false
---

# Windows CI Clippy: fe-workbench's result_large_err on move_one (project_panel_clipboard.rs:723) and unix-only test code (libc::kill, unused Debuggee import, unused workbench/cx and mut) fail -D warnings off unix

GitHub CI's windows "Clippy" job (`cargo clippy --workspace --all-targets -- -D warnings`) on main cdb78b942 (run 44334965415) now gets past fe-git (T1951) and fails in fe-workbench. Each error is Windows-only:

- lib, crates/fe-workbench/src/project_panel_clipboard.rs:723: `clippy::result_large_err` on `move_one`'s `Result<Moved, NotMoved>`; the Err is at least 136 bytes on Windows (PathBuf is 32 bytes there, 24 on unix, and `NotMoved` holds `ops::Failure` plus an `Option<undo::Moved>` with two PathBufs). Fix by boxing: `Result<Moved, Box<NotMoved>>` (3 `Err(NotMoved {..})` sites at ~734, 759, 788 and the two callers at ~607, 674), or box `kept`. A targeted `#[allow(clippy::result_large_err)]` with a reason comment is acceptable if boxing touches much more than those sites; say which in the commit.
- lib tests, src/chat_tool_loop_tests.rs:454,465,605,696: `libc::kill(pid, 0)` does not exist on Windows. Gate the tests that use it with `#[cfg(unix)]` (they run `sh` commands and check a process group is killed, which is unix-only); drop any helpers or imports that become unused off unix.
- src/tests/debug_session.rs:42: unused import `terminal_panel::Debuggee`; its only uses (lines ~1278, 1313) are in `#[cfg(unix)]` tests. Put `#[cfg(unix)]` on the import item.
- src/agent_tools_tests.rs:1750: unused `workbench` and `cx` in `the_executors_never_open_a_fifo`, used only in its `#[cfg(unix)]` blocks. Gate the whole test `#[cfg(unix)]` (the Windows build has nothing to check), or underscore-bind them off unix; pick the simpler.
- src/tests/agent_actions.rs:222: unused `mut` on `items` (its `items.extend` at 229 is cfg(unix)-only). Use a `#[cfg(unix)]`/`#[cfg(not(unix))]` pair of bindings or `#[cfg_attr(not(unix), allow(unused_mut))]` with a one-line reason.

The lints fire only on a Windows target, so the fix is proved by reading the cfgs, plus unix clippy and tests. No workflow edits. Unix behaviour is unchanged.

Note: crates after fe-workbench in the build order (e.g. fastedit) may surface further Windows lints once this lands; those get their own card, not this one. After landing, the lead checks the next windows Clippy run.
