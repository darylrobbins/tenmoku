---
task_uid: "01m7p64mm7vqmeyyv5wbbnbz17"
task: "T684"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::python_servers::python_files_get_pyrefly_by_default passes: with default LspSettings, Registry::default().specs() and server_for(specs, \"python\") give pyrefly, launched as `pyrefly lsp`, and pyright and ty are not in the specs"
    required: true
    manual: false
  - id: "C2"
    text: "tests::python_servers::each_choice_launches_its_command passes: python_server = ty gives `ty server`, pyright gives `pyright-langserver --stdio`; an explicit enabled [lsp.servers.pyright] with python_server unset still keeps pyright as the lead; only the chosen server runs (no silent fallback)"
    required: true
    manual: false
  - id: "C3"
    text: "tests::python_servers::each_server_gets_its_interpreter_section passes with fake servers and a root .venv: pyrefly's workspace/configuration section python has pythonPath = \u003croot\u003e/.venv/bin/python; ty's section ty has configuration.environment.python = that path; pyright's python.pythonPath = that path"
    required: true
    manual: false
  - id: "C4"
    text: "tests::python_servers::ruff_runs_alongside_only_when_found passes: with \u003croot\u003e/.venv/bin/ruff present, a .py file gets the chosen lead plus ruff (add-on, `ruff server`, asked for no completion); with no ruff anywhere, ruff never launches, shows no status and raises no toast"
    required: true
    manual: false
  - id: "C5"
    text: "tests::python_servers::missing_lead_names_binary_and_install passes: with pyrefly not found, the server stops with text naming `pyrefly` and `pip install pyrefly` (ty: `uv tool install ty`; pyright: `npm install -g pyright`), which the status item shows"
    required: true
    manual: false
  - id: "C6"
    text: "Python root markers are pyproject.toml, pyrefly.toml, ty.toml, setup.py, requirements.txt, pyrightconfig.json in that order, tested in crates/fe-lsp/src/tests/python_servers.rs; tests::python_servers and the fe-config setting tests in crates/fe-config/tests/lsp_python_server_setting.rs (default, each value, an unknown value warned and defaulted, project layer narrowed as narrow_lsp does) pass"
    required: true
    manual: false
  - id: "C7"
    text: "Existing tests that assume pyright for Python either set lsp.python_server = \"pyright\" in their fixture or move to pyrefly; cargo nextest run -p fe-lsp -p fe-config -p fe-workbench lsp passes and cargo clippy -p fe-lsp -p fe-config -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
  - id: "C8"
    text: "Checked in the real Linux app (Xvfb + xdotool, run-fastedit-linux): a Python project with `pip install pyrefly ruff` in .venv and neither on PATH; opening a .py file shows pyrefly ready (with the .venv env from T1940), a deliberate type error gets a pyrefly diagnostic, and a ruff lint shows too; screenshot in the decision file"
    required: true
    manual: false
---

# Python server choice: lsp.python_server = pyrefly (new default), ty or pyright, with ruff server running alongside when it's found, and a not-found status that names the binary and how to install it (Mac T1941)

User request (Linux tester; approved by the user, who wants a Rust-native Python server): Python should default to a Rust-native server. Lead research 2026-11-17: pyrefly 1.4.1 is stable (1.0 in May 2026), scores 141/146 on the typing conformance suite (pyright 138, ty 137), ships the widest LSP feature set, and Google adopted it; ty 0.0.105 is beta. Both are MIT, native, pip-installed.
Add `lsp.python_server` (enum PythonServer, like TypescriptServer at crates/fe-config/src/settings.rs:1487, default Pyrefly; values pyrefly, ty, pyright). Wire it the way typescript_server() works (crates/fe-lsp/src/registry.rs:127): the chosen server is the Python lead and the others never run. An explicitly enabled [lsp.servers.<name>] for a non-chosen Python server keeps today's pyright users working.

Built-ins (registry.rs builtin_specs, next to the pyright spec at ~line 45):
- pyrefly: `pyrefly lsp` over stdio; settings section "python" (pythonPath, analysis.*, pyrefly.typeCheckingMode). See https://pyrefly.org/en/docs/IDE/ and /configuration/.
- ty: `ty server`; section "ty", interpreter at configuration.environment.python. See https://docs.astral.sh/ty/editors/ and /reference/editor-settings/.
- pyright: unchanged (pyright-langserver --stdio, python.pythonPath).
- ruff: `ruff server`, a built-in add-on (spec.add_on) for python: lint, format and code actions, no completion. It runs only when found. A missing ruff is silent: no status and no T1741 missing-add-on toast. https://docs.astral.sh/ruff/editors/
All of them are pip wheels installed into the venv (<venv>/bin/pyrefly, ty, ruff), so command lookup and the interpreter go through T1940's environment discovery and its per-server interpreter hook. Set each server's interpreter key through that hook; don't special-case it in the launcher.

Missing lead: the not-found text (server.rs launch_error, which T1940 already changes to name the command and where it looked) adds an install hint from a new ServerSpec field (beside `package`, launcher.rs:65). Root markers as in the criteria, for all four.

Blast radius: the default moving off pyright breaks tests that pin pyright for Python (the files in --scope; the Switch launcher in crates/fe-workbench/src/tests/lsp.rs:167 routes the python fake by the name "pyright"). Prefer setting python_server = "pyright" in fixtures over rewriting them. Read docs/decisions/lsp-client.md §3.2–3.3 and the add-on decision (docs/decisions/q5j03ax2af-lsp-add-on-servers.md) first. Overlaps: T1742 (registry.rs, tests/registry/**) and T1741 (missing add-on toast; keep ruff's silent exception whichever lands second). Don't widen --scope; if a file outside it is needed, stop and say so.

## Helium filing note

- USER-approved feature, lent from the Mac as y142. Depends on helium T683 (Mac T1940); rebase on it
  (and T682 / Mac T1939) before starting. T683 puts python_env's tests in `crates/fe-lsp/src/python_env_tests.rs`
  and its setting test in `crates/fe-config/src/settings_python_env_tests.rs`, not where the Mac's T1940
  card named them; read T683's decision file for its interpreter hook.
- The real-app check (Xvfb + xdotool) uses the run-fastedit-linux skill, with `pip install pyrefly ruff` in a
  project .venv (neither on PATH); screenshot in the decision file. Kill only processes you started, by PID;
  never pkill/killall by name (other lanes share this host).
- Ask the lead before rgt finish if anything in the criteria can't be met as written.

## Implementation notes

Details and the C8 screenshot are in `docs/decisions/v5wbbnbz17-python-servers.md`.

- Unset, `lsp.python_server` is pyrefly unless `[lsp.servers.ty]` or `[lsp.servers.pyright]` is there (on or
  off): then that one. A table turned off still counts, so a user who turned pyright off (for no Python
  server, or one of their own) doesn't get pyrefly in its place.
- ruff (`only_if_found`): before each launch fe-lsp asks the launcher's new `ServerLauncher::finds`, which
  defaults to `LocalLauncher`'s lookup (env bin, then PATH). Missing, it never launches, sends no Starting and
  ends `Stopped(None)`. It's left out of the specs while Python has no main server, so it never leads.
  T1741's toast hasn't landed on helium; the decision says how it must skip ruff.
- "Asked for no completion" is `ServerSpec::completion = false`, which drops `completionProvider` from the
  capabilities the session records.
- The carried T683 F4 is fixed in server.rs: a second Starting once the launch's environment is chosen, if it
  differs from the session's, and before the launcher is called (tested with a held launch, first and restart).
- Respec rev 2 added `crates/fe-lsp/src/python_env_tests.rs` (its registry test used `ty` as a user server and
  pinned pyright as the only interpreter hook), `crates/fe-config/l10n/en-US/config.ftl` (the unknown-value
  warning), `crates/fe-workbench/src/plugin_prompt.rs` (the built-in server list) and the decision file.
- C8: pyrefly's `basic` preset (used with no `pyrefly.toml`/`[tool.pyrefly]`) doesn't report the sample's bad
  assignment, so the project carries an empty `[tool.pyrefly]`.
