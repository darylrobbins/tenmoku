---
task_uid: "01m7cw14gh0d61snbpkhtxssvc"
task: "T647"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::registry (cargo nextest run -p fe-dap registry) pass: resolution_order_is_host_user_extension_builtin as a table test, type_aliases_resolve (lldb-dap, lldb, debugpy, python, go), a_missing_program_is_listed_not_found_with_a_hint, the_rust_default_is_lldb_dap_when_found_else_codelldb"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::registry::bare_commands_resolve_on_the_login_path passes: an adapter's bare command is resolved as language servers' are (fe-lsp's LoginPath, crates/fe-lsp/src/launcher.rs), checked with a fake login PATH, not the app's PATH"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in tests::registry pass for the data rules: python_interpreter_order (.venv, $VIRTUAL_ENV, python3), rust_lldb_init_commands_come_from_rustc_print_sysroot (with a fake runner), and the macOS xcrun lookup as data (cfg(target_os = \"macos\") code tested as a table on every OS)"
    required: true
    manual: false
  - id: "C4"
    text: "Test a_server_port_adapter_connects_to_the_port_its_host_gives in crates/fe-dap/tests/registry_host.rs (cargo nextest run -p fe-dap --test registry_host) passes: a spec of the new Port launch kind (the adapter's port comes from a host, not ${port}) connects fe-dap to fe-dap-fake listening there"
    required: true
    manual: false
  - id: "C5"
    text: "Tests in crates/fe-config/tests/debug_settings.rs (cargo nextest run -p fe-config --test debug_settings) pass: user_adapter_settings_parse, a_project_file_cannot_set_an_adapters_command_args_or_env_or_log_payloads (narrow_debug drops them with a warning), timeouts_have_defaults"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-config passes (settings template, schema and l10n tests included)"
    required: true
    manual: false
---

# Debug adapter registry and [debug] settings: built-in lldb-dap, codelldb, debugpy and delve specs on the login PATH, a server-port launch kind, and project narrowing (debugger D3) (Mac T1843)

# Debug adapter registry and [debug] settings: built-in lldb-dap, codelldb, debugpy and delve specs on the login PATH, a server-port launch kind, and project narrowing (debugger D3)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **m**.

## Problem

A launch configuration names an adapter by name or by VS Code `type`. Something must
turn that into a command to run, without letting a project file choose what runs.

## What to build (§3; §11 D3; §13 amendments)

- **`AdapterSpec`** (name, command, args, env, transport, languages, `types`,
  `launch_defaults`, `init_commands`) and §3's built-in specs as data: `lldb-dap`
  (and `xcrun -f lldb-dap` on macOS), `codelldb`, `debugpy`, `delve` (tested end to
  end in D19). js-debug's spec waits for D23b.
- **A third launch kind, `Port` (§13, Java):** the host supplies the port of an
  adapter it already started (jdtls's java-debug, D21b), alongside `Stdio` and `Tcp`
  (`${port}`). D1's TCP connect does the connecting.
- **Resolution order** host → user settings → extension → built-in, as fe-lsp's
  `registry.rs` layers.
- **PATH (§13 amendment):** bare commands resolve on the **login shell's PATH** as
  language servers do (`fe_lsp::LoginPath`, `crates/fe-lsp/src/launcher.rs`), not "app
  PATH plus mise shims" as §3 says. fe-dap may depend on fe-lsp for it or take a
  resolver from the host; record the choice.
- The Python interpreter choice, Rust `init_commands` from `rustc --print sysroot`,
  "not found" status with a hint.
- **`DebugSettings`** in a new `crates/fe-config/src/debug_settings.rs` (as
  `agent_settings.rs` was added): `[debug.adapters.<name>]` (`command`, `args`,
  `env`, `path`), timeouts, `debug.log_payloads`; `narrow_debug` strips a project
  layer's adapter commands and `log_payloads` (as `narrow_lsp`,
  `crates/fe-config/src/settings.rs`). `debug.inline_values` is D18's. Settings
  template, schema and their strings follow the `[agents]` precedent.

## Not in this card

Launch configurations (D4), extension-contributed adapters (D15), installing
adapters (D14), the java spec itself (D21b).

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Linux-OK** (headless tests only; no screenshot goldens move).

## Implementation notes

- fe-dap depends on fe-lsp for `LoginPath` (and on fe-config for `DebugSettings`);
  the design calls are in `docs/decisions/bpkhtxssvc-debug-adapter-registry.md`.
- `crates/fe-dap/src/registry.rs` holds the `Registry` and `registry/builtin.rs` the
  built-in specs and data rules (Python, Rust formatters, `lldb_dap_lookups`).
- `AdapterTransport::Port(u16)` is the host-given port. `LocalLauncher` only connects.
- The macOS xcrun criterion's test is `tests::registry::the_macos_xcrun_lookup_is_data`.
