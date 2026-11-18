---
task_uid: "01m7p63y1xmqe25bezve8a441s"
task: "T683"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::launcher::python_env::pyright_is_found_in_the_project_venv passes: a root with .venv/bin/python and an executable .venv/bin/pyright-langserver script, the builtin pyright spec with env PATH=/nonexistent, and LocalLauncher.launch(\u0026spec, \u0026root, \u0026LaunchCancel::new()) is Ok and runs the venv's script (plain assert!, no message)"
    required: true
    manual: false
  - id: "C2"
    text: "Env order is a tested table in python_env::tests (crates/fe-lsp/src/python_env_tests.rs): lsp.python_environment setting (absolute, ~, or relative to the server root) \u003e $VIRTUAL_ENV \u003e \u003croot\u003e/.venv \u003e \u003croot\u003e/venv; a candidate counts only if bin/python (Scripts\\python.exe on Windows) exists; none found falls back to the PATH lookup exactly as today"
    required: true
    manual: false
  - id: "C3"
    text: "Any spec whose languages include python (built-in or [lsp.servers.\u003cx\u003e], e.g. ty or pyrefly) looks up a bare command in the env's bin before PATH; the server process gets PATH with the env's bin first and VIRTUAL_ENV set; non-Python servers are unchanged (tests::launcher_path_safety still passes)"
    required: true
    manual: false
  - id: "C4"
    text: "A generic ServerSpec hook names the setting the interpreter goes in; pyright's built-in sets python.pythonPath, and a fake-server test sees workspace/configuration for python answer pythonPath = the env's python; no hook means nothing is added"
    required: true
    manual: false
  - id: "C5"
    text: "A not-found Python server names the command and where it looked (e.g. 'pyright-langserver not found in /p/.venv/bin or on PATH'); rust-analyzer's 'rust-analyzer not found' text is unchanged (tests::lsp::restart_and_show_log passes)"
    required: true
    manual: false
  - id: "C6"
    text: "lsp.python_environment is a user-level setting in schema, template and Settings label; a project file's value is dropped with the existing untrusted-lsp warning (crates/fe-config/src/settings_python_env_tests.rs, declared from settings.rs)"
    required: true
    manual: false
  - id: "C7"
    text: "lsp::python_env_tests::status_item_names_the_env passes: a Python file's status item shows the env (e.g. 'pyright (.venv): ready'), with the env's full path in its tooltip; Rust files' item unchanged"
    required: true
    manual: false
  - id: "C8"
    text: "Checked in the real Linux app (run-fastedit-linux): a project with pyright installed only via pip into .venv starts pyright, shows the env in the status item and publishes diagnostics for an unresolved import (screenshot in the decision file)"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p fe-lsp -p fe-config -p fe-workbench python_env lsp passes; cargo clippy -p fe-lsp -p fe-config -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Python language servers ignore the project's virtualenv: find the env ($VIRTUAL_ENV, .venv, venv, or a setting), run the server from its bin and hand it the env's interpreter (pyright in .venv not found, Linux user) (Mac T1940)

User report (Linux, Tenmoku 0.2.3, Python project, PRIORITY): "it's trying to find pyright for linting I think. I installed it to my project virtual environment. But it didn't find it."
Cause: the built-in pyright spec runs bare `pyright-langserver` (crates/fe-lsp/src/registry.rs:46-53), looked up only on the spec's or the login shell's PATH (crates/fe-lsp/src/launcher.rs:374-398, 565-597). fe-lsp has no virtualenv handling. The error names the spec rather than the binary (crates/fe-lsp/src/server.rs:796-801). pip's `pyright` and npm both install a `pyright-langserver` entry point; pip puts it in <venv>/bin.
USER direction: make this server-neutral. Python is moving off pyright to a Rust-native server (pyrefly default, ty alternative; a separate card adds that choice and depends on this one); this card only makes any Python server use the project's env.
Design: a new crates/fe-lsp/src/python_env.rs finds the env as the criteria order it, at the server's root (its marker root). Precedent is fe-dap's python_interpreter (crates/fe-dap/src/registry/builtin.rs:162-182). Running a project's .venv is safe because LSP servers only start in trusted roots (LspManager::set_trust, manager.rs:32-35). Relative PATH entries stay unsearched; only the found env's absolute bin is added.
Resolve the env per launch in the server thread (server.rs, which has spec and root): prepend the bin to the lookup and to the child's PATH, set VIRTUAL_ENV, and merge the interpreter into a copy of spec.settings at the spec's hook key (router.rs section() answers workspace/configuration from it). The workbench status item calls the same python_env function for the editor's server root.
Do not widen --scope; if a file outside it is needed, stop and say so.

## Helium filing note

- USER PRIORITY (Linux tester, Python project). Lent from the Mac as y141. Rebase on T1939 first.
- Test layout differs from the Mac card on purpose:
  - Regression test (C1, base API only: LocalLauncher, the builtin pyright spec, LaunchCancel): a
    submodule of the existing fe-lsp `tests::launcher` (`mod python_env;` in `src/tests/launcher.rs`, file
    `src/tests/launcher/python_env.rs`): `tests::launcher::python_env::pyright_is_found_in_the_project_venv`.
  - The env-order table (C2) and other tests of the new python_env API: `crates/fe-lsp/src/python_env_tests.rs`,
    declared `#[cfg(test)] #[path = "python_env_tests.rs"] mod tests;` from python_env.rs.
  - The setting test (C6): `crates/fe-config/src/settings_python_env_tests.rs`, declared from settings.rs,
    instead of a new `crates/fe-config/tests/lsp_python_environment.rs`.
  - The status-item test (C7): `crates/fe-workbench/src/lsp_python_env_tests.rs`, declared
    `#[cfg(test)] #[path = "lsp_python_env_tests.rs"] mod python_env_tests;` from lsp.rs:
    `lsp::python_env_tests::status_item_names_the_env`. It can use the `crate::tests::lsp` helpers.
  - `tests/mod.rs` (fe-lsp and fe-workbench) is out of scope.
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
