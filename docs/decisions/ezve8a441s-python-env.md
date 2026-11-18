# T683: Python language servers run in the project's virtualenv

`crates/fe-lsp/src/python_env.rs` finds a Python server's environment at its
root: `lsp.python_environment` (absolute, `~`, or relative to the root), then
`$VIRTUAL_ENV` (the spec's `env`, else Tenmoku's; absolute only), then
`<root>/.venv`, then `<root>/venv`, the first with `bin/python`
(`Scripts\python.exe` on Windows). A server is Python's when its spec's
`languages` hold `python`, built-in or `[lsp.servers.<x>]` (ty, pyrefly).

- `LocalLauncher` looks a bare command up on `<env>/bin` + the PATH it used
  before, and runs the server with that PATH and `VIRTUAL_ENV=<env>`. With no
  env, nothing changes. A host's own launcher gets the spec unchanged;
  `fe_lsp::python_env` is public for it.
- `ServerSpec::interpreter_setting` names where the interpreter goes in the
  `workspace/configuration` answers (pyright: `python.pythonPath`), unless the
  user's settings set it; no hook adds nothing. Merged per launch in server.rs
  and on each `Configure`.
- Not found: `LocalLauncher`'s error for a Python server's bare command says
  where it looked, and the status shows it: `pyright-langserver not found in
  <env>/bin or on PATH`, or with no env `pyright-langserver not found on PATH,
  and <root> has no Python environment (.venv or venv)`. A host launcher's
  NotFound, whose search fe-lsp doesn't know, still reads `pyright not found`
  (existing fe-lsp tests use such a launcher), as does every other server.
- The env is found once per launch, just before it, and is the one thing all
  use: the spec handed to the launcher carries it
  (`ServerSpec::python_env_choice = Chosen(env)`, which `LocalLauncher` runs in
  without looking again, and a host launcher should too), the session keeps it
  (`LspManager::python_env(server)`), and the interpreter setting is merged
  from it. An env made or removed mid-launch counts from the next launch.
  Specs from settings or a host default to `Find`: `LocalLauncher` used alone
  (C1) finds one itself.
- The workbench item for a Python file whose server has an env shows it
  (`pyright (.venv): ready`, `ready` shown only then) with
  `Python environment: <path>` in its tooltip. A file in another language on
  the same server (one serving Python and Rust) gets the item as before.
- The setting is in the schema and the Settings label; the template documents
  it inside the `[lsp.servers]` block (a new block would change the qps-ploc
  fixture, outside the card's scope). An untrusted project's value is dropped
  with the untrusted-LSP warning; a trusted folder's applies, like its other
  `lsp` keys.

C8, real Linux app: a project with only `pip install 'pyright[nodejs]'` into
`.venv` (no pyright on any PATH), trusted, `main.py` importing a missing
module: `ezve8a441s-pyright-in-venv.png` shows "pyright (.venv): ready" and
`Import "missing_module_t683" could not be resolved`.
