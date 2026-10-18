---
task_uid: "01m55c0ykf73yb1wakpmf8n8cr"
task: "T144"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "manager::trust_tests::extension_servers::an_extension_server_routes_its_language: route_for gives the extension server for a language no built-in lists, and the built-in for one a built-in lists"
    required: true
    manual: false
  - id: "C2"
    text: "manager::trust_tests::extension_servers::user_settings_apply_over_an_extension_server: [lsp.servers.\u003cname\u003e] command and args apply over it, enabled = false removes it, and an extension spec named like a built-in is ignored"
    required: true
    manual: false
  - id: "C3"
    text: "manager::trust_tests::extension_servers::changing_extension_specs_restarts_only_changed_servers: a change to an extension spec's settings alone restarts nothing, a changed command restarts that server only"
    required: true
    manual: false
  - id: "C4"
    text: "extension_lsp_servers_reach_the_workbench: an extension in a temp dir with [contributes.lsp.zls] gives fe_workbench::extension_servers a zls spec with its command, args and languages after kiln::init, and a wrongly typed field is a positioned error in kiln::platform::config::diagnostics"
    required: true
    manual: false
---

# Route languages to extension-contributed language servers through fe-lsp's registry, under the user's [lsp] settings (Mac T844)

Filed on argon from Mac T844 (card ~/coordination/argon-cards/zzn/T844.json rev 1; note zzn, kiln extensions stage 1: declarative, data only). Mac task uid 01m55bz5bnmbm6vawm7phy4nmt. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced). Respect scripts/check_kiln_boundary.py and give every new kiln file its SPDX header line. Argon deps: T142 (Mac T842), T122 (Mac T705). Title the host commit "... (Mac T844)".

## Problem

Stage-1 extensions contribute language-server configs (`[contributes.lsp.<name>]`,
extensions record §2, §4). fe-lsp's registry (`crates/fe-lsp/src/registry.rs`) knows built-in
specs, the user's `[lsp]` settings and host specs (`LspManager::register_spec`, which win over
settings); an extension's server has no tier. Precedence the record fixes, lowest first:
built-ins, extension servers, the user's `[lsp]` settings; host specs stay on top.

## Change

- **fastedit's kind.** `crates/fastedit/src/config.rs` `with_extensions` (the themes card's)
  adds `Config::extension_kind("lsp", schema)`, the schema a map of
  `fe_config::settings::LspServerSettings` (schemars), so field errors are positioned
  diagnostics, and an `on_extensions` callback turning each extension's tables into specs
  (`fe_lsp::extension_spec`) handed to `fe_workbench::set_extension_servers`.
- **fe-lsp.** `crates/fe-lsp/src/registry.rs`: `Registry::set_extension_specs(Vec<ServerSpec>)`,
  a tier between built-ins and settings: `apply(spec, settings)` lays the user's
  `[lsp.servers.<name>]` over an extension spec as over a built-in, and `enabled = false`
  turns it off. Order in `specs()`: built-ins, settings-only, extension (in the order given),
  host. An extension spec named like a built-in or a host spec is ignored with a `log::warn!`.
  `pub fn extension_spec(name: &str, settings: &LspServerSettings) -> Option<ServerSpec>`
  (`None` without a `command`). `crates/fe-lsp/src/manager.rs`:
  `LspManager::set_extension_specs(Vec<ServerSpec>)`, applied as `update_settings` is
  (`reconcile`; only servers whose spec changed restart, `needs_restart`).
- **fe-workbench.** `crates/fe-workbench/src/lsp.rs`: `pub fn set_extension_servers(cx,
  Vec<ServerSpec>)`, a global every workbench's manager starts with (`new_lsp_manager`) and
  that live managers get on change; `pub fn extension_servers(cx) -> Vec<ServerSpec>` reads it.
  Both are re-exported beside `set_lsp_settings` in `crates/fe-workbench/src/lib.rs` (that one
  `pub use` line is this card's only change there).
- Trust is unchanged: an extension server, like any, starts only under a trusted root.

Tests: unit tests in the new `crates/fe-lsp/src/extension_servers_tests.rs`, declared in the
existing `crates/fe-lsp/src/trust_tests.rs` with
`#[path = "extension_servers_tests.rs"] mod extension_servers;` (so their path is
`manager::trust_tests::extension_servers::…`); and `crates/fastedit/tests/extension_servers.rs`,
including `../src/config.rs` and `../src/theme.rs` by `#[path]` as
`crates/fastedit/tests/shipped_config.rs` does.

## Out of scope

Downloading or installing server binaries; workspace extensions (a later card).

## Implementation notes

- **kiln-core, beyond the listed scope.** kiln's schema check
  (`Layer::check_schema`, `kiln/crates/kiln-core/src/layers.rs`) didn't go
  into a map's values (`additionalProperties`). So with the schema a map of
  `LspServerSettings`, a wrongly typed field loaded with no diagnostic, and
  C4 couldn't pass. `check_table` now checks each key `properties` doesn't
  list against an object `additionalProperties`, as it checks listed keys:
  type errors at the value, unknown keys as warnings. The test is
  `kiln/crates/kiln-core/tests/extension_manifest.rs`
  `an_app_kinds_map_values_are_checked`. fe-config's `schema_for`, which
  lists each `lsp.servers` name as a property to get around this, still
  works, but its comment now gives an outdated reason.
- `fe_lsp::extension_spec` also returns `None` for a table with
  `enabled = false`. fastedit skips those quietly, and warns about an enabled
  table that has no `command`.
- The registry ignores an extension spec named like a built-in, a host spec
  or an earlier extension spec, with a `log::warn!` when the specs are set
  (and when a host spec takes over an extension's name). It filters again
  in `specs()`, so a host spec registered later still wins.
- `fe_workbench::set_extension_servers` reaches live managers through the
  workbenches `Shutdowns` already tracks. It must not be called while a
  workbench is being updated. kiln's `on_extensions` callback isn't.
