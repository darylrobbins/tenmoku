---
task_uid: "01m5fv83rxrset8qy116anewg4"
task: "T260"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "bindings::the_kiln_world_instantiates_the_empty_fixture: tests/fixtures/empty.wat instantiates through the generated bindings and its init and shutdown exports are callable"
    required: true
    manual: false
  - id: "C2"
    text: "bindings::every_unimplemented_import_returns_the_unimplemented_error: the strings.wat fixture calling each stubbed import (commands, events, process, net, secrets) gets an unimplemented error value, not a trap"
    required: true
    manual: false
  - id: "C3"
    text: "bindings::strings_cross_the_boundary: strings.wat passes a string to the log import and the host receives it unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "fe-plugin bindings::the_editor_world_includes_the_kiln_world: the fastedit:editor world generates bindings whose linker accepts a kiln:extension component"
    required: true
    manual: false
  - id: "C5"
    text: "every WIT file parses and resolves (covered by the bindgen build), and the module doc records how fastedit:editor resolves kiln:extension"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p kiln-plugin -p fe-plugin and cargo clippy -p kiln-plugin -p fe-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Write the kiln:extension and fastedit:editor 1.0 WIT worlds, one file per interface, with generated bindings and unimplemented host stubs

## Problem

The record's API surface (§7) is a set of WIT interfaces, `kiln:extension@1.x` for what any kiln app has and
`fastedit:editor@1.x` for the editor's. Without the WIT files and generated bindings, no later card can
implement or test a host import, and each would invent its own types. This card writes the whole
v1.0 contract once, as one file per interface so later cards edit only their own, with every host import a
stub that returns an "unimplemented" error until its card lands.

## Change

- `kiln/crates/kiln-plugin/wit/` one file per interface, package `kiln:extension@1.0.0`: `types.wit`
  (result error variant, diagnostic, a command-id string alias, a buffer version as `u64`), `log.wit`,
  `commands.wit` (register with title, args schema, context predicate, `mutates-buffer`; invoke; the
  exported `run-command`), `events.wit` (buffer opened, changed with ranges and version, saved, closed;
  focus and selection changed; config reloaded; plus a `lagged` marker), `fs.wit` (empty: files come
  through WASI preopens), `process.wit` (`spawn(program, args, stdin) -> output`), `net.wit` (`fetch(request)
  -> response`), `secrets.wit` (get, set, delete by name), `world.wit` (world `extension`: imports every
  interface, exports top-level `init` and `shutdown` and the command and event handlers).
- `crates/fe-plugin/wit/`, package `fastedit:editor@1.0.0`: `buffer.wit` (snapshot at a version, ranges,
  edit transaction against a version, proposed change), `decorations.wit` (decoration sets and gutter
  entries at a version), `status.wit`, `world.wit` (world `editor-extension` that includes
  `kiln:extension/extension`). Resolve the cross-package dependency with bindgen's multiple `path`s, or a
  `wit/deps` copy guarded by a test that fails when the copy differs from the kiln package; record the
  choice in the module doc.
- `kiln/crates/kiln-plugin/src/bindings.rs`: `wasmtime::component::bindgen!` for the kiln world; a
  `HostState` struct that holds each slice's own `State` type (`commands::State`, `events::State`,
  `fs::State`, `process::State`, `net::State`, `secrets::State`; each a unit struct defined in its stub
  module now) so later cards add fields in their own files. Each slice module holds its
  `impl <interface>::Host for HostState` returning `Err(unimplemented)`. `crates/fe-plugin/src/lib.rs` and
  its `editor`, `decorations` and `status` modules do the same for the editor world.
- Fixtures: `kiln/crates/kiln-plugin/tests/fixtures/` `.wat` components: `empty.wat` (exports `init` and
  `shutdown`, nothing else) and `strings.wat` (a core module with a `memory` and a bump `realloc`, showing the
  pattern for passing strings that later fixtures reuse). No Rust guest here.

## Notes

The version is 1.0 and unreleased, so later cards may amend their own interface file freely; the compat card
(P16) is what freezes a minor. Keep the worlds small: panels are deferred, so no view-tree interface exists.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.

## Implementation notes

- fastedit:editor resolves kiln:extension through bindgen's multiple `path`s
  (`../../kiln/crates/kiln-plugin/wit`, then `wit`), so there is one copy of the kiln WIT and no
  `wit/deps` to guard. `with: { "kiln:extension": kiln_plugin::bindings::kiln::extension }` makes the
  editor world reuse kiln-plugin's generated types and `Host` traits: `fe_plugin::HostState` holds a
  `kiln_plugin::bindings::HostState`, and `fe_plugin::add_to_linker` adds kiln's imports against it
  before the editor's own. Recorded in the `fe_plugin` crate doc.
- `empty.wat` exports `init`, `shutdown` and no-op `command-handler` and `event-handler` instances:
  every export of a world is required for the typed `Extension::instantiate`, so a component with only
  `init` and `shutdown` does not instantiate through the bindings. Lifting the handlers' string and
  list parameters needs a memory and a realloc, which it keeps unexported.
- Event delivery is batched: the exported handler is `handle-events: func(events: list<event>)`, and
  `lagged(u64)` stands in for dropped events. `events.subscribe(event-kinds)` is the stubbed import.
- `log` has no slice module in scope; `HostState.log` is a `LogSink` closure, defaulting to the `log`
  crate under the `kiln_plugin::guest` target, so tests can observe what the guest logged.
- Every `error` case carries a string, keeping the variant 4-aligned; `strings.wat` relies on that
  (each `result`'s payload sits at offset 4) to hand the error it received back through a `try-*`
  export typed `func() -> error`.
- crates/fe-plugin/Cargo.toml was added to the scope: the bindings need `wasmtime` as a normal
  dependency, and the test needs its `wat` feature.
