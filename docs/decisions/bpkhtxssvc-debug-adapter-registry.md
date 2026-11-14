# Debug adapter registry and `[debug]` settings: design calls (debugger D3)

Card T647 (Mac T1843), under `docs/decisions/r1b9dw3g0g-debugger.md` §3 and §13.

## fe-dap depends on fe-lsp for the login PATH

§13 has bare adapter commands resolve on the login shell's `PATH`, as language
servers' do. The spec offered two ways: depend on fe-lsp, or take a resolver from
the host. fe-dap depends on fe-lsp (and fe-config, for `DebugSettings`), and the
`Registry` holds an `Arc<fe_lsp::LoginPath>`:

- the lookup is then the same code as fe-lsp's, so the two can't drift: `~` in
  commands and `PATH` entries, relative and empty entries skipped (the root may be
  untrusted), the login shell run once with its 2 s limit;
- the host passes the `LoginPath` it already shares with fe-lsp (`LoginPath::system()`),
  so the shell still runs once per process;
- both crates are GPUI-free (`scripts/check-no-gpui.sh` prints `ok: fe-dap`), and fe-lsp
  doesn't depend on fe-dap, so there is no cycle. The cost is compile time for fe-dap.

The registry resolves a found command to an absolute path, so `LocalLauncher`'s own
lookup only sees absolute commands from it. The host still gives the launcher the
login `PATH` (`with_path`) for what the adapter itself runs.

## Layers

Highest first: host → user settings → extension → built-in.

- **Host specs** are used as they are, as fe-lsp's are; settings don't apply over them.
- **The user's `[debug.adapters.<name>]`** applies over an extension's or a built-in spec
  of that name, key by key: `command`, `args`, `path`, `transport`, `languages` and
  `types` replace, `env` is set on top. A name with no spec under it is an adapter of
  the user's own and needs a `command`. Setting a built-in's `command` turns off its
  own finding (the Python choice, `xcrun`) and gives the generic hint.
- **Extension specs** replace a built-in of the same name, unlike fe-lsp, where one
  is ignored: D14 installs codelldb as an extension contributing `codelldb`, whose
  absolute command must win over the built-in's bare one.
- **Each adapter sits in the layer that won it** (review round 1, F1). A built-in or an
  extension's spec that the user's settings change is the user's, so it comes before
  every extension's and built-in spec. An extension's codelldb is in the extensions'
  layer. Within the user's layer adapters go by name, since the settings table keeps
  no other order; within the others, in the order they were given. One order serves
  `names`, `list`, `type` lookup after names, and a language's default. So a user's
  codelldb takes the `lldb` alias from an extension's adapter. Changing codelldb in
  settings also makes it Rust's default ahead of a found lldb-dap: §13 Q3's "lldb-dap
  when found, else codelldb" is the rule among the built-ins as shipped, and settings
  for an adapter state a preference for it.

`AdapterSpec` stays the launcher's type. How a built-in finds its program, its hint
and whether it takes Rust's formatters are the registry's (`Entry`), not fields of
the spec.

## `path`

The record lists `path` beside `command`, `args` and `env` without saying what it is.
Here it is a list of folders a bare `command` is looked in before the login shell's
`PATH` (or before the `PATH` the adapter's `env` sets), for an adapter kept outside
the `PATH`, such as codelldb's `adapter/` folder.

## The Port launch kind

`AdapterTransport::Port(u16)`: the host has started the adapter (jdtls's java-debug,
D21) and gives its port. `LocalLauncher` starts nothing; it connects with
`connect_with`, D1's retries and 5 s limit, since the adapter may still be starting.
The `AdapterProcess` it returns has no id. `kill` only closes the connection, because
the adapter's process is the host's to stop. `exited` resolves once the connection has
closed from either end (F3): the output is wrapped so that a read finding its end, or
failing, marks it closed. So a session whose adapter closes after `disconnect` ends at
once, without the exit grace or a kill.
The registry treats a Port spec as found: there is no program to look for.

## Data rules

- **Python** (§3): `<root>/.venv/bin/python`, then `$VIRTUAL_ENV/bin/python`, then
  `python3` on the login `PATH` (`Scripts\python.exe` and `python` on Windows). A
  `.venv` python that can't be run is passed over. `$VIRTUAL_ENV` comes from the
  `Context` (`Context::new` reads this process's, if absolute).
- **Rust formatters**: only under lldb-dap, and only when the context's language is
  `rust`. `rustc` is the one on the login `PATH`, run in the root (so a
  `rust-toolchain.toml` picks the toolchain) through a `Runner` seam. `SystemRunner`
  starts it in its own process group and waits for both its exit and the end of its
  stdout within one 5 s deadline (F2). A program that leaves a descendant holding its
  stdout has run out of time. On timeout or failure the whole group is killed and the
  pipe closed.
  The sysroot must be one absolute line, and paths are double-quoted for LLDB, so a
  sysroot with spaces stays whole. They are appended to the spec's own
  `initCommands`.
- **macOS `xcrun`**: `lldb_dap_lookups(HostOs)` is data, `[OnPath]` on every OS plus
  `Run { /usr/bin/xcrun, -f lldb-dap }` on macOS. `HostOs::current()` is the only
  `cfg` code, so the macOS rule is tested as a table on Linux. xcrun's answer must be
  an absolute path to a program that is there.
- **"Not found"** keeps the adapter listed, with `command` set to what was looked for
  and a hint from fe-dap's catalogue for each built-in, or a generic one naming
  `debug.adapters.<name>.command`.

Running `.venv`'s python or `rustc` in a cloned folder runs that folder's choices, so
the host resolves for a launch only once the folder is trusted (§8), as it launches
anything there.

## `[debug]` settings

`crates/fe-config/src/debug_settings.rs`, following `agent_settings.rs`:

- `debug.adapters.<name>`: `command`, `args`, `env`, `path`, plus `transport`, `languages`
  and `types` so that an adapter of the user's own can be complete.
- `debug.timeouts`: `request` (10000), `launch` (30000) and `connect` (5000), in
  milliseconds as `lsp.timeouts` are, matching fe-dap's `REQUEST_TIMEOUT`,
  `LAUNCH_TIMEOUT` and `CONNECT_TIMEOUT`. D7 passes them to the session.
- `debug.log_payloads` (off).

`narrow_debug` keeps a project layer's `debug.timeouts`, which choose nothing that runs,
and drops everything else with the `config-project-debug-ignored` warning at each key:
every field of every `adapters.<name>`, `log_payloads`, and unknown keys. The Settings
editor's `workspace_allowed` and its project view agree.
