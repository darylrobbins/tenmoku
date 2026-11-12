---
task_uid: "01m7773k7a217t9qcj3cdrcam7"
task: "T590"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`compat::world_for(minor)` returns one generated world per supported minor; `HOST_API` is 1.1; `wit/released/1.0/` is a frozen copy (kiln-plugin and fe-plugin) with a check."
    required: true
    manual: false
  - id: "C2"
    text: "New test `released_wit_1_0_matches_frozen_copy` in kiln/crates/kiln-plugin/tests/released_wit.rs fails when the 1.0 WIT in wit/ differs from wit/released/1.0/ (and the same for fe-plugin's)."
    required: true
    manual: false
  - id: "C3"
    text: "New test `compat_1_0_plugin_runs_on_1_1_host` in kiln/crates/kiln-plugin/tests/compat.rs: an existing 1.0 component fixture loads and answers a call on the 1.1 host."
    required: true
    manual: false
  - id: "C4"
    text: "New test `compat_pinned_1_0_host_refuses_1_1_plugin` : a plugin built for 1.1 is refused with the incompatible-API diagnostic by a host pinned to 1.0."
    required: true
    manual: false
  - id: "C5"
    text: "No new interface is added; the template under crates/tenmoku-plugin/template builds against 1.1 and says 1.1 is preview."
    required: true
    manual: false
---

# API 1.1 minor: one generated world per minor, compat::world_for, frozen 1.0 WIT (C1, 4.5) (Mac T1696)

# API 1.1 minor: one generated world per minor, compat::world_for, frozen 1.0 WIT (C1, 4.5)

Draft id C1 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 4.5 and the C1 row. Narrowed from the design's wit/** and template/** globs to the files that exist today; if the worker needs another template file, it must say so in the finish notes rather than widen the scope. The wit/** scope covers only released/ here; later cards own their new .wit files.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- wasmtime 48's `NameMap` matches `@1.0.0` and `@1.1.0` both ways, for linker
  imports and for component and instance export lookups. So one linker at 1.1
  serves 1.0 imports. The per-minor world decides which generated `…Pre::new`
  checks the exports and which export names are looked up.
- `bindings::v1_0` (and `fe_plugin::v1_0`) is generated from `wit/released/1.0`,
  with `with` mapping its interfaces onto the current modules. That way the
  types and `Host` impls are shared. `compat::World` gives `api()`,
  `interface(name)` and `check(&InstancePre)`, and `Compat::world_for`
  refuses a version the host doesn't serve. The free function
  `compat::world_for` uses `Compat::CURRENT`.
- `Plugin::load_for(…, &Compat, needed)` refuses an incompatible plugin as
  `LoadError::Incompatible` before compiling it, shown with the existing
  `kiln-plugin-invalid` key. `load_hosted` and the other loaders keep their
  signatures and run against the current world.
- fe-workbench: `Found.engine` → `compat::world_for` in `decide` (failure is a
  `StopReason::Load`) → `Identity.world` → `load_with` → `HostedPlugin::load`.
  The `events.subscribe` shadow is defined under the current world's name.
- `compat::released` is the released-WIT check: `read`, `digest` (sha256,
  CRLF normalized) and `changes`. `changes` is structural: it ignores
  comments, layout and versions, counts each name a `use` brings in on its
  own, and reports any change to a released record, variant, enum, flags or
  function. kiln-plugin checks its own copy in `tests/released_wit.rs`. kiln/
  may not name fe-plugin, so fe-plugin's check is a unit test in
  `src/lib.rs`.
- **Prebuilt guests rebuilt against 1.1:** `crates/fe-plugin/tests/guests/editor/editor.wasm`
  and `crates/tenmoku-plugin/template/word_count.wasm`. wasmtime's
  `Linker::define_unknown_imports_as_traps`, which `fe-plugin/tests/{editor,
  formatting,diagnostics}.rs` and `tenmoku-plugin/tests/template.rs` use,
  opens each import instance by its exact name. For a 1.0 guest it makes a new
  `…@1.0.0` instance of trap stubs, which shadows the host's `@1.1.0`
  definitions. That fails 20 tests. The real hosts (the runner,
  fe-workbench) don't stub, so 1.0 plugins load there; `compat_1_0_plugin_runs_on_1_1_host`
  and fe-workbench's `@1.0.0` WAT fixtures cover that. An embedder that stubs
  unknown imports has to build its guests against the current minor.
