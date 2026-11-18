---
task_uid: "01m7p63xdrmqg5xt9h44rnbgtz"
task: "T682"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::lsp::log_tab::a_stopped_servers_log_tab_is_titled_and_says_why passes: Lsp::new, open tool.py, force_lsp_status(Stopped(Some(\"pyright-langserver not found\"))), run lsp.show_log; the active tab's tab_title is '\u003cname\u003e Log' (pyright Log), never 'untitled', the editor is read-only and its text holds the status text ('not found') and the server's root"
    required: true
    manual: false
  - id: "C2"
    text: "With server output (the stderr and logMessage case of tests::lsp::restart_and_show_log) the tab shows the log text as now, under the same title, and tests::lsp::restart_and_show_log still passes"
    required: true
    manual: false
  - id: "C3"
    text: "Running lsp.show_log again for the same server and root focuses the existing log tab with fresh text rather than adding a second tab"
    required: true
    manual: false
  - id: "C4"
    text: "Checked in the real Linux app (run-fastedit-linux): a Python project with no pyright-langserver on PATH, click the red pyright status item; a 'pyright Log' tab opens and says why it is stopped (screenshot in the decision file)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench lsp:: passes; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Language Server Log opens an empty, editable 'untitled' tab when the server never started (pyright not found, Linux user) (Mac T1939)

User report (Linux, Tenmoku 0.2.3, Python project, PRIORITY): "Clicking the log button opens 'untitled' which has no content."
The status item "pyright: stopped: pyright not found" runs lsp.show_log (crates/fe-workbench/src/lsp.rs:913-921). run_server_op (lsp.rs:2168-2194) opens EditorModel::new(Buffer::from_file_contents(&manager.log(..)), None): no path, so the tab is "untitled", and it is editable. The ring (crates/fe-lsp/src/manager.rs:1125) holds only stderr and window/logMessage, and a failed launch writes nothing to it (crates/fe-lsp/src/server.rs:522-528), so it is empty.
Fix in the workbench only, the way debug_show_log does it (crates/fe-workbench/src/debug_session.rs:1085-1108): pane.new_editor, set_read_only(true), set_tab_title(tr!("workbench-lsp-log-title", name), "<name>.log"). When the ring is empty, show a short header first: the server, its root, its current status text (status_text in lsp.rs) and a note that it has written no output. Reuse an open log tab for the same (name, root).
T1741 later rewrites this path to list logs for every server. Keep the change confined to the log branch of run_server_op plus one helper. Do not widen --scope; if a file outside it is needed, stop and say so.

## Helium filing note

- USER PRIORITY (Linux tester, Python project). Lent from the Mac as y141.
- Test layout differs from the Mac card on purpose: the regression test goes in a submodule of the
  existing `tests::lsp` module (`mod log_tab;` in `src/tests/lsp.rs`, file `src/tests/lsp/log_tab.rs`,
  beside `watched_files`), so its path is `tests::lsp::log_tab::a_stopped_servers_log_tab_is_titled_and_says_why`.
  `tests/mod.rs` is out of scope. Everything in `src/tests/lsp.rs` and `src/tests/lsp/**` must compile
  against the base's API (force_lsp_status, lsp.show_log and tab_title exist there).
- Why: rgt finish's regression check applies only test files (`tests/**`, `*_tests.rs`) plus the
  test-support paths to the base and builds there. A new module declared from `tests/mod.rs` gives
  "0 tests" at base, and any overlaid test file that uses an API this card adds breaks the base build;
  both fail a bugfix finish as needs_human:infra. So: the regression test sits in a submodule of a
  module the base already has and uses only the base's API; tests needing new APIs go in a `*_tests.rs`
  declared from a production file (not declared at base, so not built there). Never add a new
  `crates/*/tests/*.rs` integration test or touch `tests/mod.rs` on this card.
- Regression test and criteria file use the bare paths above (no crate prefix).
- The real-app check (Xvfb + xdotool) uses the run-fastedit-linux skill; screenshot in the decision file.
  Kill only processes you started, by PID; never pkill/killall by name (other lanes share this host).
- Ask the lead before rgt finish if anything in the criteria can't be met as written.
