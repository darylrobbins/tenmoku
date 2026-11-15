# Launch configurations: design calls (T655, debugger D4)

The model is `crates/fe-workbench/src/launch_defs.rs`; its module docs are the
reference. These are the calls §4 of `r1b9dw3g0g-debugger.md` left open.

- **The alias table without resolving.** `fe_dap::Registry::find` resolves an
  adapter, which may run the login shell, `rustc` or `xcrun`. Listing
  configurations must not do that, so fe-dap gained `Registry::name_for`: the
  name `find` would choose, by name and then by `type`, in the same layer
  order. Nothing is looked up. `launch_defs` keeps what was written and the name
  it maps to (`AdapterRef::Named`).
- **Listed but not startable.** Attach, an unknown adapter or `type`, a
  pre-launch task that isn't in `tasks`, and an unsupported or unknown variable
  each add a reason to `LaunchConfig::blockers`, and each is also a diagnostic
  ("…, so it can't be started"). A variable outside §4's list, such as
  `${config:…}` or `${pathSeparator}`, is treated like `${command:…}`. If it
  were passed through unexpanded, the debuggee would get a wrong path rather
  than an error.
- **cwd.** A leading `${workspaceFolder}` is read as the root, so the usual
  `launch.json` `"cwd": "${workspaceFolder}"` is checked by its words like
  `launch.toml`'s. A cwd with any other variable is `Cwd::Expand`, checked when
  it starts (D7). An absolute or escaping cwd leaves the configuration out in
  both files, as `task_defs` does.
- **Program from cargo.** A detected binary's program is
  `Program::CargoArtifact { bin }`, and its pre-launch step is
  `PreLaunch::CargoBuild(argv)`, not a task label. Its adapter is
  `AdapterRef::DefaultFor("rust")`, so §13 Q3's choice between lldb-dap and
  codelldb is made when it starts, not when it is listed.
- **launch.json details.** A name that `launch.toml` also has is dropped with a
  diagnostic, because the user wrote both. A detected name either file has is
  dropped silently, as tasks are. `console` (`"integratedTerminal"`) is passed
  through as written, since §4 types only `request`, `program`, `args`, `cwd`,
  `env` and `stopOnEntry`. The platform sub-object is merged deeply (an `env`
  in `linux` adds to the base `env`), as VS Code's `mixin` does. A `windows`
  sub-object applies only on Windows.
- **Detection order.** The package's own `src/main.rs` binary comes first, then
  `[[bin]]` entries in file order, then `src/bin/` by name. `autobins = false`
  is honoured, as is a `[[bin]]` that claims a file by its `path`.
- **Variables.** `launch_defs::variables::expand` is written and tested here,
  so D7 only has to fill in `Values`. `${env:NAME}` with `NAME` unset expands to
  nothing, as in VS Code.
- **Review round 1.**
  - Binary discovery follows cargo's edition rule: with a `[[bin]]` and no
    `autobins`, the 2015 edition (also the default when none is written)
    finds no files. An inherited edition is read from the root's own
    `[workspace.package]`, from the manifest that `package.workspace` names,
    or from the nearest `[workspace]` manifest above the root. When there is
    none to inherit, cargo can't build, so nothing is offered. Hidden
    `src/bin/` entries are skipped, as cargo skips them (round 2). All of this
    was checked against `cargo metadata`.
  - The typed fields see a TOML date or time as the wrong type.
    `adapter_args` holding a date, a time or a non-finite float leaves the
    entry out, because JSON can't carry it and changing it would pass the
    adapter something the file didn't say.
  - A leading BOM is skipped in both files.
