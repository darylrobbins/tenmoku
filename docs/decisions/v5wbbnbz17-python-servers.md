# T684: Python's server: pyrefly by default, ty or pyright, with ruff beside it

`lsp.python_server` (`fe_config::settings::PythonServer`: `pyrefly`, `ty`,
`pyright`) picks Python's built-in main server. Only that one runs, as only
the TypeScript server `lsp.typescript_server` picks does; there's no
fallback to the others when it's missing.

- **Unset** (the default): pyrefly, unless `[lsp.servers.ty]` or
  `[lsp.servers.pyright]` is there, turned on or off: then that one (in the
  setting's order, pyrefly, ty, pyright). `LspSettings::chosen_python_server`
  has the rule. So a user who configured pyright keeps it, and one who
  turned pyright off (to have no Python server, or a server of their own,
  such as `[lsp.servers.pylsp]`) doesn't get pyrefly in its place. An
  explicit `python_server` beats any table.
- **Unknown values** (`"jedi"`, `3`) are dropped with a warning at the key
  (`config-python-server-unknown`), in the user file, a project file, and a
  trusted root's layers, so the rest of the file still applies and the
  server is the default. In an untrusted project the key is narrowed like
  every other `lsp` key but `enabled = false` (`UNTRUSTED_LSP`); servers
  don't start there anyway (lsp-client.md §9 Q7).

## Built-ins (crates/fe-lsp/src/registry.rs)

| name | command | interpreter key (`ServerSpec::interpreter_setting`) | install hint |
|---|---|---|---|
| pyrefly | `pyrefly lsp` | `python.pythonPath` | `pip install pyrefly` |
| ty | `ty server` | `ty.configuration.environment.python` | `uv tool install ty` |
| pyright | `pyright-langserver --stdio` | `python.pythonPath` (unchanged) | `npm install -g pyright` |
| ruff | `ruff server` (add-on) | none | `pip install ruff` |

All four share the root markers `pyproject.toml`, `pyrefly.toml`, `ty.toml`,
`setup.py`, `requirements.txt`, `pyrightconfig.json`, in that order
(pyright's `setup.cfg` is gone). Each runs in T683's environment
(python_env.rs), looked up in its bin before the `PATH`; the interpreter
goes in at the spec's hook, never special-cased in the launcher. pyrefly
asks `workspace/configuration` for `python` and reads `pythonPath`, as VS
Code's Python extension sends it; ty asks for `ty` and takes
`configuration` as an inline `ty.toml`.

## Install hint

`ServerSpec::install` (beside `package`) is appended to a not-found status
by server.rs `launch_error`: `pyrefly not found on PATH, and <root> has no
Python environment (.venv or venv); install it with pip install pyrefly`, or
after T683's `… not found in <env>/bin or on PATH`, or a host launcher's
`pyrefly not found`. The status item shows it as `pyrefly: stopped: …`.

## ruff: only when found, silent otherwise

- `ServerSpec::only_if_found`: just before each launch, after the
  environment is chosen, fe-lsp asks the launcher's new
  `ServerLauncher::finds(spec, root)`, whose default is what `LocalLauncher`
  would run (a file at an absolute or relative command, or a bare one in the
  environment's bin or on the spec's or login shell's `PATH`; `WithPath`
  uses its own login path). Not found, the thread never sends `Starting`,
  never calls the launcher, logs at info, and ends `Stopped(None)`: no
  status text (the workbench item follows the lead only) and no error. A
  host launcher running servers elsewhere can override `finds`.
- **No toast.** T1741's one-time "missing add-on" toast hasn't landed on
  helium. When it does, ruff must stay its exception: it never fails a
  launch (it's never launched), so a toast keyed on a launch's NotFound
  won't fire, but one keyed on `package` or on `Stopped` must skip
  `only_if_found` specs. ruff's `package` is left `None` for that reason.
- **Beside a main server only.** An `only_if_found` add-on is left out of
  `Registry::specs` while none of its languages has a main server (the
  chosen Python server turned off, with no server of the user's), so it
  never leads a document and a missing one never becomes a file's status.
- **No completions.** `ServerSpec::completion = false` takes
  `completionProvider` out of the capabilities its session records, so it's
  never asked (ruff offers none today; the main server has them).
- `[lsp.servers.ruff] enabled = false` turns it off; its other keys apply
  as for any built-in.

## T683 F4 (carried)

A launch's environment is chosen after the thread's first `Starting`. Now,
once it's chosen and before the launcher is called (which may block), the
thread sends `Starting` again if the environment differs from the one the
session had (none, on a first launch; the last launch's, on an automatic
restart), so the workbench item, which reads `LspManager::python_env` as it
hears, names it while the launch blocks. Servers with no environment send
no extra event. Tested by
`tests::python_servers::a_new_environment_is_announced_before_a_blocked_launch`.

## Tests

- fe-lsp `tests::python_servers`: C1–C6 and F4.
- fe-config `tests/lsp_python_server_setting.rs`: default, each value, the
  table rule, unknown values, project narrowing.
- Fixtures that pin pyright for Python: fe-workbench's `tests::lsp::Lsp`
  writes `python_server = "pyright"` and `servers.ruff.enabled = false` into
  the user file it reads at trust (`pinned`), so its `Switch` launcher still
  routes the Python fake by the name `pyright`. fe-lsp's generic registry
  tests move to pyrefly, or keep pyright through a `[lsp.servers.pyright]`
  table.

## Scope

Respecced in (rev 2): `crates/fe-lsp/src/python_env_tests.rs` (its
registry test used `ty` as a user's own server, now a built-in, and pinned
pyright as the only interpreter hook), `crates/fe-config/l10n/en-US/config.ftl`
(the unknown-value warning), `crates/fe-workbench/src/plugin_prompt.rs`
(pyrefly, ty and ruff are built-in servers a plugin's `lsp:<server>` may
name), and this file.

## C8: the real Linux app

Xvfb + xdotool (run-fastedit-linux), a fresh config: a project with
`pyproject.toml` and a `.venv` made by `python3 -m venv` and
`.venv/bin/pip install pyrefly ruff` (pyrefly 1.4.1, ruff 0.17.5), neither
on any `PATH` and `$VIRTUAL_ENV` unset, trusted. `main.py`:

```python
import os


def double(n: int) -> int:
    return n * 2


result: str = double(3)
```

`v5wbbnbz17-pyrefly-ruff.png`: the status item reads "pyrefly (.venv):
ready" (T683's environment), the editor shows pyrefly's
`` `int` is not assignable to `str` `` on line 8 and ruff's
`` `os` imported but unused `` on line 1, and the Problems panel lists them
by source (Ruff 1:8, Pyrefly 1:8 hint, Pyrefly 8:15 error). No other
Python server started (the pyrefly Log and `ps`).

Note: pyrefly with no `pyrefly.toml` or `[tool.pyrefly]` uses its `basic`
preset, which doesn't report this bad assignment (`pyrefly check` agrees:
"0 errors ... using preset `basic`"). The project here has an empty
`[tool.pyrefly]` table, which gives the default checks. Users wanting full
checks without a config can set
`[lsp.servers.pyrefly.settings.python.pyrefly] typeCheckingMode = "default"`;
the built-in sends no default of its own.
