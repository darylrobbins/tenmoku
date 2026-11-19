---
task_uid: "01m7reb8bjyejhf8qm6wcec102"
task: "T1973"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "GH CI windows Test passes tests::debug_commands::a_delayed_removal_holds_the_continue, tests::debug_commands::a_rejected_removal_holds_the_next_run_to_cursor, tests::debug_commands::remove_all_breakpoints_clears_every_file, tests::debug_panel::breakpoints_are_grouped_by_file_and_enabled_from_their_row, tests::debug_panel::rows_and_controls_have_accessible_names"
    required: true
    manual: false
  - id: "C2"
    text: "GH CI windows Test passes tests::debug_session::a_failing_pre_launch_task_starts_no_session, tests::debug_session::a_new_window_takes_the_debug_settings_already_loaded, tests::debug_session::changing_log_payloads_applies_to_the_next_session, tests::debug_session::a_read_run_keeps_its_output_whatever_the_scrollback, tests::launch_defs::variables_expand_from_the_active_editor_and_the_environment"
    required: true
    manual: false
  - id: "C3"
    text: "GH CI windows Test passes tests::task_defs_python::main_py_runs_with_the_project_python, tests::task_defs_python::project_scripts_and_uv, tests::task_defs_python::pytest_when_tests_exist, tests::project_multi_select::copy_path_joins_lines, tests::project_new_menu::target_is_the_selected_folder"
    required: true
    manual: false
  - id: "C4"
    text: "Tests only: expected paths are built per component or with tests::slashed, and roots are canonicalized with dunce; macOS and Linux expectations unchanged"
    required: true
    manual: false
---

# fe-workbench debug, launch, Python task and project panel tests on Windows: expected paths built with mixed separators or std canonicalize (\\?\); and the debug read-run test runs /bin/sh

# Path-separator expectations in fe-workbench tests

GH CI 44405849017 on 8ba6f008c, windows Test. Production is consistent in every case below; the tests' expected values are wrong on Windows:
- debug_commands.rs:1016 and :754: `root.join("src/other.rs")` gives `...\src/other.rs`, which never equals the adapter's `source.path`, so `sent_lines` never matches: "timed out waiting for the first run's send / the refused removal / the empty sets". Use `root.join("src").join("other.rs")`, as `main_path` does.
- debug_panel.rs:749/:774: rows show `src\main.rs, 2 breakpoints` and the test expects `src/main.rs`. Use `tests::slashed` or a built path.
- debug_session.rs:562, :1094, :787: the launch config's `"${workspaceFolder}/target/debug/main"` expands as text to `C:\...\.tmpX/target/debug/main`, while the test expects `root.join("target/debug/main")` = `C:\...\.tmpX\target/debug/main`. Expect `format!("{}/target/debug/main", root.display())`.
- debug_session.rs:1167 a_read_run_keeps_its_output_whatever_the_scrollback runs `["/bin/sh", "-c", ...]`, so the exit is `None`. Use the POSIX sh helper from the run_process card.
- launch_defs.rs:797: `${file}` is `/project\src/pkg/tool.py` on Windows (Path::join of the fake root); expect that, built the same way.
- task_defs_python.rs:110/:119/:295: production gives `/project\.venv\Scripts\python.exe`, and the expectation joins `.venv/Scripts/python.exe` as one string (mixed). Build the expectation from components.
- project_multi_select.rs:410: `root.join("d/e.txt")` vs the copied `d\e.txt`. Join components.
- project_new_menu.rs:367: `fs::canonicalize(dir.path())` gives `\\?\`, so `reveal` finds no row ("src expanded: [(0, "src"), (0, "Cargo.toml")]"). Use `dunce::canonicalize`.

If one of these turns out to be a production mismatch that users see, note it in the decision file for a separate card; don't change production here.
