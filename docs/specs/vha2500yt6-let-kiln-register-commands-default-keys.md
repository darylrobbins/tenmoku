---
task_uid: "01m4vbj49pjr3aatvha2500yt6"
task: "T673"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/contributions.rs: a command added after init with a default key and a menu runs from that key, is listed in the palette, and shows in its menu with its shortcut"
    required: true
    manual: false
  - id: "C2"
    text: "contributions.rs: a row in the app's keymap TOML or the user's keymap naming a command registered only after init binds once it is registered, instead of staying dropped as unknown"
    required: true
    manual: false
  - id: "C3"
    text: "contributions.rs: two contributed commands with the same default key in one context give the diagnostic KEY is bound to both A and B, and init panics on it under fail_fast"
    required: true
    manual: false
  - id: "C4"
    text: "contributions.rs: the app's TOML and the user's keymap still override contributed keys, a user keymap reload keeps the contributed layer, and several additions in one update re-resolve once"
    required: true
    manual: false
  - id: "C5"
    text: "Existing kiln, kiln-core and kiln-platform tests pass unedited, and fastedit's keymap and menu tests pass"
    required: true
    manual: false
---

# Let kiln register commands, default keys and menu items after init, re-resolving the keymap and menus, through a Contributions registry

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2, registry and macros; design record
`docs/decisions/kiln-app-layer.md`). Every macro in phase 2 emits plain data into one `Contributions` registry,
which is also the seam a later plugin host submits to. Today commands are frozen at init:
`kiln/crates/kiln/src/setup.rs:28-32` says an app's commands must be registered through the `Config`, "because
`init` checks the keymap and menus against every command registered when it runs: one registered later would be
dropped from them as unknown". `KeymapStore` (`kiln-core/src/load.rs`) and the `ConfigService` holding it
(`kiln-platform/src/config.rs`) never re-check the app's or the user's keymap against a grown catalog.

## Change

Add `kiln::Contributions` (a GPUI global) in a new `kiln/crates/kiln/src/contributions.rs`, re-exported from
`lib.rs`, with the command half of the registry:

- `Contributions::add_command(cx, CommandSpec { id, title, context, keys, menu, register })`: `register` records
  the typed command (`kiln_platform::register_command::<A>`) and any app-level handler; `keys` are default
  bindings (one string, or per platform as the keymap TOML allows); `menu` names a menu (created after the app's
  menus if missing) and an `order`.
- **Allowed after init.** Each addition re-resolves, in this order: kiln's bundled defaults, then a
  *contributions* layer built from the commands' default keys, then the app's keymap TOML, then the user's file;
  the app's and user's layers are re-checked against the grown catalog, so a row naming a command that is
  registered later now binds instead of staying dropped. The keymap is swapped with `keys::set_keymap` and the
  menus rebuilt and reinstalled (`menus::install`), contributed items placed into their menus. A user keymap
  reload keeps the contributions layer. Additions in one update re-resolve once (batch them at the end of the
  effect cycle), not once per command.
- **Duplicate default keys.** Two contributed commands with the same default key in the same context give the
  diagnostic `<keys> is bound to both <id a> and <id b>`, logged, and a panic under `Config::fail_fast`.
- Rewrite the `setup.rs` module doc paragraph to describe the new rule; `Config::commands` stays.

Give `KeymapStore` what this needs (replacing its base layers and catalog, then re-reading) and the
`ConfigService` a way to reach its store after start. New tests go in `kiln/crates/kiln/tests/contributions.rs`.

## Implementation notes

- `CommandSpec::new::<A>(title)` fills `id` from `A::id()` and `register` with
  `kiln_platform::register_command::<A>`; `register` is a `fn(&mut App, CommandInfo)`, so a macro can emit a
  wrapper that also installs an app-level handler. `Keys` is `None`, `All("secondary-n")` or
  `ByPlatform([("macos", ..), ("default", ..)])`, the keymap TOML's shapes. Adding a command whose ID was
  contributed before replaces it, as `register_command` does.
- Before `kiln::init`, additions are only recorded; `init` registers them after `Config::commands` and resolves
  their keys and items with the app's. After `init`, `register` runs at once (so the palette lists the command
  at once) and the keymap and menus re-resolve in one `cx.defer` per update. `Contributions::resolutions(cx)`
  counts the re-resolutions, which is how the batching is tested.
- Re-resolution re-parses the bundled and app keymaps and the app's menu table against a fresh catalog (the
  typed-command catalog is a snapshot, so the store's catalog is replaced too), then
  `KeymapStore::set_defaults` re-reads the user's file. `ConfigService::keymap_store()` returns a
  `SharedKeymap` (the store behind an `Rc<RefCell>`, shared with the running service); its `update` swaps the
  keymap and logs and republishes `ConfigDiagnostics` when they change. The service's batch `apply` now
  publishes when the combined diagnostics differ from the global, which also covers an update made from a
  source's reload hook.
- Contributed bindings carry the command's context; their diagnostics name `<contributed keys>`. A contributed
  menu item resolves its shortcut in `Config::menu_shortcut_context`, plus the command's context when that is a
  single identifier. A contributed item replaces an app item with the same ID (the command ID) in place.
- Under `fail_fast`, a re-resolution checks every shipped diagnostic again, as `init` does; note that a row in
  the app's keymap naming a command contributed only after `init` is still an unknown command at `init`, so it
  panics there under `fail_fast` (contribute such commands before `init`).
- Review round 1: each contributed command's keys are written as one `[[bindings]]` entry and read by
  `KeymapLayer::parse_for`, so they get the keymap file's checks (every platform's keys, the click rule, the
  command and its args against the catalog); diagnostics are renamed to `<id>.keys` at line = its index. A
  contributed menu item whose command the catalog rejects without args is left out with a
  `<contributed menus>` warning. `init` registers early contributions one at a time, so one contributed from a
  `register` is registered too. The config service keeps its sources (not a weak ref) once its task ends, so
  `SharedKeymap::update` still publishes `ConfigDiagnostics`. `menus::install` now refreshes windows, and
  `MenuBarView` closes an open menu the new bar no longer has (and guards its arrows on an empty bar).
