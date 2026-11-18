---
task_uid: "01m7p7gy2prmmzf47enr7hxm2k"
task: "T688"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::task_defs_python::main_py_runs_with_the_project_python passes (faked ProjectFiles): a root with main.py and .venv/bin/python lists '\u003croot\u003e/.venv/bin/python main.py' (Argv, group run, Source::Python); with no .venv it lists 'python3 main.py' ('python' on Windows), as fe_dap::registry::python_interpreter picks; a root with no Python marker (main.py, pyproject.toml, setup.py, requirements.txt, uv.lock) lists no Python task"
    required: true
    manual: false
  - id: "C2"
    text: "tests::task_defs_python::pytest_when_tests_exist passes: '\u003cpython\u003e -m pytest' (group test, its group's default by the by-name fallback when no other source gives one) is listed when the root has a non-empty tests/ folder, a test_*.py or *_test.py, conftest.py, pytest.ini, [tool.pytest.ini_options] in pyproject.toml or [tool:pytest] in setup.cfg, and not otherwise"
    required: true
    manual: false
  - id: "C3"
    text: "tests::task_defs_python::project_scripts_and_uv passes: each [project.scripts] name in pyproject.toml, in file order, is listed as 'uv run \u003cname\u003e' when uv.lock is at the root, else as '\u003croot\u003e/.venv/bin/\u003cname\u003e' only if that file exists; with uv.lock the main file and pytest are 'uv run main.py' and 'uv run pytest' (Source::Uv); a pyproject.toml that doesn't parse yields only the file-based tasks"
    required: true
    manual: false
  - id: "C4"
    text: "Run Task… rereads on changes to pyproject.toml, uv.lock, main.py, pytest.ini, setup.cfg, conftest.py and the root's tests/ and .venv/ (touches_tasks); rows show 'python' or 'uv' as their source; the existing task_defs, task_commands and run_widget tests pass"
    required: true
    manual: false
  - id: "C5"
    text: "Checked in the real Linux app (run-fastedit-linux): a Python project with main.py, a .venv and tests/test_x.py, and no tasks.toml; Run Task… lists the .venv python main.py and pytest tasks, and running main.py shows its output; a uv project (uv.lock) lists uv run tasks if uv is on the host, else the decision file says it wasn't (screenshots in the decision file)"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-workbench task_defs task_commands run_widget passes; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Python projects detect no tasks: Run Task… says 'No tasks: add .tenmoku/tasks.toml' for a project with main.py and .venv; detect the main file, [project.scripts], pytest and uv run (Mac T1945)

User request (Linux tester, approved by the user, PRIORITY): "Doesn't detect tasks automatically for Python projects". The project has main.py, a .venv and no pyproject; Run Task… shows "No tasks: add .tenmoku/tasks.toml" (commands.ftl:476).
State: detected_tasks (crates/fe-workbench/src/task_defs.rs:504-525) covers Cargo, npm, Gradle, Maven, .NET, Make and Just, and only reads the root's own files through ProjectFiles (task_defs.rs:110-126). Python appears only in the debug launch configs (launch_defs.rs:86, "Debug current file").
Design: add Source::Python and Source::Uv (task_defs.rs:198-208), named in source_name (task_commands.rs:91-101), and add both to the by-name default fallback (task_defs.rs:484-496). Add python_tasks to detected_tasks after .NET. The interpreter comes from fe_dap::registry::python_interpreter(root, None, |path| files.exists(path)) (crates/fe-dap/src/registry/builtin.rs:169-183): <root>/.venv/bin/python, else python3. Commands are argument lists, never shell lines. Add the Python files to touches_tasks (run_widget.rs:249-263). Update the module docs' Detection list. Running a task already needs folder trust, which covers running the project's .venv.
Out of scope: running the current file (load has no editor state; Debug current file exists). T1940's env lookup (venv/, lsp.python_environment) is a follow-up once it lands. pytest at cursor (test_runnables.rs:804-810, T1856) is also out of scope.
Overlaps: T1707 (ready) also changes task_defs.rs; rebase on whichever lands first. Do not widen --scope; if a file outside it is needed, stop and say so.

## Helium filing note

- USER-approved feature (priority), lent from the Mac as y145. No dependency. On the helium board it overlaps
  T662/T664/T685/T687 on `tests/mod.rs` only (a one-line mod list); rebase as they land.
- The real-app check (Xvfb + xdotool) uses the run-fastedit-linux skill; screenshots in the decision file.
  Make the scratch Python project inside your lane worktree, never /tmp. Kill only processes you started,
  by PID; never pkill/killall by name (other lanes share this host).
- Ask the lead before rgt finish if anything in the criteria can't be met as written.
