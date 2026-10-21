---
task_uid: "01m5fv848ssyvkw94a9px1se23"
task: "T262"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "manifest::a_plugin_entry_parses_wasm_and_capabilities: a [contributes.plugin] with wasm and capabilities buffer.read, commands and net:api.example.com loads through load_manifest with plugin_kind() and gives the matching Grants"
    required: true
    manual: false
  - id: "C2"
    text: "manifest::an_unknown_capability_is_an_error_naming_it: capabilities = [\"buffer.reed\"] is a diagnostic that names buffer.reed"
    required: true
    manual: false
  - id: "C3"
    text: "manifest::the_wasm_path_must_be_a_file_inside_the_extension: ../x.wasm, an absolute path and a missing file are each errors"
    required: true
    manual: false
  - id: "C4"
    text: "manifest::a_workspace_extension_cannot_declare_a_plugin: from_extension with ExtensionOrigin::Workspace and a plugin entry is Err(WorkspaceCannotInstallPlugins); with User it loads; an extension with no plugin entry is Ok(None) in both"
    required: true
    manual: false
  - id: "C5"
    text: "manifest::the_signature_slot_is_accepted_and_never_verified: a signature string parses, is kept in PluginManifest, and signature_verified() is false"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p kiln-plugin and cargo clippy -p kiln-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Declare the plugin manifest kind with capabilities and a reserved signature slot, and refuse plugins from workspace extensions

## Problem

A plugin is an extension whose manifest says so. Stage 1's loader already checks app-defined kinds against a schema
(`ExtensionApi::kinds`, `AppKind`), so a code plugin can be one more kind, `plugin`, without touching
`kiln-core/src/extension.rs`. The record also requires that only the user's own extensions directory can install a
plugin: a workspace can never install or grant one (§3, §7).

## Change

New `kiln/crates/kiln-plugin/src/manifest.rs`:

- `pub fn plugin_kind() -> AppKind` for `[contributes.plugin]`: `wasm` (path of the component inside the
  extension directory), `capabilities` (array of capability strings, parsed by
  `kiln_command::Capability::parse`), and `signature` (optional string, a reserved slot: accepted and kept,
  never verified; the type says so in its doc and `PluginManifest::signature_verified()` is always false).
  The extension's top-level `[engine]` already names the API version needed.
- `pub struct PluginManifest { id, name, version, wasm: PathBuf, capabilities: Grants, signature:
  Option<String> }` and `PluginManifest::from_extension(&Manifest, ExtensionOrigin) -> Result<Option<Self>,
  PluginManifestError>`, where `ExtensionOrigin::{User, Workspace}` and a `Workspace` extension that contains a
  `plugin` entry is `Err(WorkspaceCannotInstallPlugins)` (the fastedit wiring passes the origin; the plugin
  kind is also only registered for the user layer there).
- Errors name the capability or path at fault: an unknown capability string, a `wasm` path that is not a file inside
  the extension directory (reuse the loader's containment rule), more than one plugin entry.

## Notes

Capabilities are only parsed here; granting is P6. Tests build manifests in a temp dir with
`kiln_core::extension::load_manifest` and the `plugin_kind()` schema.

Project rules: keep tests lean and fast (hostile and process fixtures bounded to under 2 s each); new tests go in new files; the scope lists exact files.
