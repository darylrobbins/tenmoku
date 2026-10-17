---
task_uid: "01m55c0xtc22d5vh0s8cresmca"
task: "T140"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "a_valid_manifest_loads: id, name, version, engine, the themes/keymaps/menus paths resolved under the extension's directory, and an app kind's table kept as written, with no diagnostics"
    required: true
    manual: false
  - id: "C2"
    text: "manifest_errors_name_their_key_and_position: a bad id, an id starting with a reserved namespace, a malformed version, a missing [engine], a contributed path that is absolute, escapes with .., is a symlink out of the directory or is missing, each give no manifest and an Error naming \u003cdir\u003e/extension.toml, the dotted key and its line:column"
    required: true
    manual: false
  - id: "C3"
    text: "the_engine_must_be_this_app_and_api: at API 1.3, engine 1.2 loads, 1.4 and 2.0 are errors naming both versions, and an [engine] for another app is an error"
    required: true
    manual: false
  - id: "C4"
    text: "plugins_and_unknown_kinds: a [plugin] table is an error, an unknown contributes kind a warning (the manifest loads without it), an app kind failing its schema an error at the field, and grammars without a license an error"
    required: true
    manual: false
  - id: "C5"
    text: "AppPaths::extensions_dir is \u003cstate dir\u003e/extensions, and scripts/check_kiln_boundary.py passes"
    required: true
    manual: false
  - id: "C6"
    text: "Test kiln-core extension licence handling passes (in the card's manifest test file, named by its full module path): a manifest with no license, or license = \"MIT AND (\" , is refused with a diagnostic naming the key; 'MIT' and 'Apache-2.0 OR MIT' classify Permissive, 'GPL-3.0-only' and 'MIT AND GPL-2.0-only' Copyleft, 'LicenseRef-acme' Unknown, 'GPL-2.0-only OR MIT' Permissive; with allowed_licenses = [\"MIT\"] a 'GPL-3.0-only' extension is refused and an 'Apache-2.0 OR MIT' one loads"
    required: true
    manual: false
---

# Parse and validate extension.toml manifests in kiln-core, with the engine version check and errors as config diagnostics (Mac T840)

Filed on argon from Mac T840 (card ~/coordination/argon-cards/zzn/T840.json rev 4; notes zzn and zzo, kiln extensions stage 1). Mac task uid 01m55bz3b4pvdgz8cjh0nsb0sd. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced). Updated per note zzo (the user's decisions): every extension carries an SPDX license checked against extensions.allowed_licenses; installed extensions live in the state dir. Respect scripts/check_kiln_boundary.py and give every new kiln file its SPDX header line. Title the host commit "... (Mac T840)".

## Problem

The user approved (2026-10-17) declarative extensions as stage 1 of plugin support: data-only
packages with an `extension.toml` manifest. Nothing in kiln reads one. The extensions decision
record (the decision task on this work, §2) fixes the format; this card is its parser and
validator, GPUI-free so headless tools can load extensions as the app does.

Reuse: `kiln_core::layers::{load_layer, Layer::check_schema, Layer::key_diagnostic}` for
positioned diagnostics (`kiln/crates/kiln-core/src/layers.rs`), `kiln_command::CommandId`'s
segment rule for the id (`kiln/crates/kiln-command/src/id.rs`), `kiln_core::platform::fs`
for opening untrusted files (`layers.rs` `read`).

## Change

New `kiln/crates/kiln-core/src/extension.rs`, `pub mod extension;` in
`kiln/crates/kiln-core/src/lib.rs`. No new dependency (no `semver`: versions parse by hand).

- `pub const MANIFEST_FILE: &str = "extension.toml";`
- `pub struct ApiVersion { pub major: u32, pub minor: u32 }` and
  `pub struct ExtensionApi { pub app: String, pub version: ApiVersion, pub reserved: Vec<String>, pub kinds: Vec<AppKind> }`:
  what the app accepts. `app` is its `AppId::short`; `reserved` the command namespaces an
  extension id may not start with; `AppKind { name: &'static str, schema: serde_json::Value }`
  each app-defined contribution kind.
- `pub struct Manifest { pub dir: PathBuf, pub id: String, pub name: String, pub version: (u32, u32, u32), pub description: Option<String>, pub license: License, pub engine: ApiVersion, pub themes: Vec<PathBuf>, pub keymaps: Vec<PathBuf>, pub menus: Vec<PathBuf>, pub app: BTreeMap<String, toml::Value> }`
  with contributed paths absolute (resolved under `dir`).
- `pub fn load_manifest(dir: &Path, api: &ExtensionApi) -> (Option<Manifest>, Vec<Diagnostic>)`.
  `None` with at least one `Severity::Error` when:
  - the file is missing, unreadable, over the layer size cap, or not TOML;
  - `id` is missing or not 2+ segments of `[a-z][a-z0-9_]*`, or its first segment is in
    `reserved`;
  - `name`, `version` (`MAJOR.MINOR.PATCH`, digits only) or `[engine]` is missing or malformed;
  - `[engine]` names another app than `api.app` ("is for notes, not this app"), or a version
    this app can't serve: major differs, or minor above `api.version.minor`
    ("needs extension API 1.4; this app has 1.3");
  - a `contributes.themes|keymaps|menus` entry is absolute, or resolves (symlinks followed,
    `fs::canonicalize`) outside `dir`, or is missing;
  - a `[plugin]` table is present: "code plugins aren't supported by this app";
  - an app kind's table fails its schema (`check_schema` over a manifest schema with
    `contributes.<kind>` set to the kind's schema);
  - `license` is missing or isn't a parsable SPDX expression (every extension, not only grammar ones);
  - `extensions.allowed_licenses` is set (passed in on `ExtensionApi` as `allowed_licenses: Option<Vec<String>>`)
    and the expression isn't satisfied by it ("<id>: licence <expr> isn't in extensions.allowed_licenses").
- `pub struct License { pub expr: String, pub class: LicenseClass }`, `pub enum LicenseClass { Permissive,
  Copyleft, Unknown }`, from a small hand-written SPDX expression parser (AND, OR, WITH, parentheses,
  `+`; no new crate dependency) and a built-in table: Permissive = the ids deny.toml's `[licenses] allow`
  lists; Copyleft = GPL-*, LGPL-*, AGPL-*, MPL-*, EPL-*, CDDL-*, EUPL-*, OSL-*; Unknown = LicenseRef-*,
  NOASSERTION and anything else. `OR` takes the most permissive branch, `AND` the least. (USER decision
  2026-10-17, licence option 3; the one-time notice for copyleft/unknown extensions is T841's.)
  Warnings (the manifest still loads): an unknown top-level key, an unknown `contributes.<kind>`
  (ignored).
  Every diagnostic is a `kiln_core::Diagnostic` naming `<dir>/extension.toml`, the dotted key
  and the line:column of the offending token.
- `pub const MAX_CONTRIBUTED_FILES: usize = 256` per extension; more is an error.

`kiln/crates/kiln-core/src/paths.rs`: `pub const EXTENSIONS_DIR: &str = "extensions";` and
`AppPaths::extensions_dir() -> Option<PathBuf>` (`<state dir>/extensions`: USER decision 2026-10-17, installed extensions live in the state dir, not config), and
`AppPaths::project_extensions_dir(root)` (`<project dir>/extensions`).

Every new kiln file starts with `// SPDX-License-Identifier: LicenseRef-Proprietary`, and
nothing in `kiln/` names fastedit or its crates (`python3 scripts/check_kiln_boundary.py`).
Tests are integration tests in the new `kiln/crates/kiln-core/tests/extension_manifest.rs`,
on manifests written into a `tempfile` directory.

## Out of scope

Discovering directories, reloading, applying any contribution (later stage-1 cards).

## Implementation notes

- The manifest format, as the later cards use it: top-level `id`, `name`, `version`, `description`,
  `license`; `[engine] <app> = "MAJOR.MINOR"` (several apps may be named; this app's key is the one
  read, and with none of its own the first other key gets "this extension is for <other>, not this
  app"); `[contributes]` with `themes`, `keymaps`, `menus` arrays of paths and a table per app kind.
- `ExtensionApi` gained `allowed_licenses: Option<Vec<String>>` (per note zzo). An allowed entry
  matches a licence term ignoring case; `<id> WITH <exception>` entries match only that term.
- Class order is Unknown < Copyleft < Permissive, so `MIT AND LicenseRef-x` is Unknown and
  `GPL-2.0-or-later OR LicenseRef-x` Copyleft. `WITH` keeps the base licence's class. Operators are
  uppercase only (SPDX), so `MIT and Apache-2.0` is refused.
- A contributed path written to climb out (`../x`) is refused before touching the disk; otherwise
  the path as written is canonicalized (so `link/..` is the link target's parent, and a missing
  directory before `..` is missing), must land inside the canonical `dir` and be a file, and is
  kept as `dir.join(<that file, relative to the canonical dir>)`: the file that was checked, under
  `dir` as given (made absolute). A path error points at the array entry, under the key
  `contributes.<kind>`. Missing keys point at 1:1.
- The 256-file cap is checked before any entry, and entry errors share one line index, so a huge
  array costs one diagnostic.
- An app kind's `$defs` move to the manifest schema's root as `<kind>/<name>`, its `$ref`s
  rewritten to match, so referenced definitions are checked and two kinds' names can't collide.
- The SPDX parser follows SPDX 2.3 annex D: `idstring`s, `LicenseRef-`/`DocumentRef-…:LicenseRef-`
  references (no `+`), `+` only directly after an ID, exception IDs plain `idstring`s. Expressions
  over 1024 bytes or nested more than 16 parentheses deep are refused; AND/OR chains are n-ary
  nodes, so nothing recurses deeper than the nesting cap.
- `AppPaths::project_extensions_dir(root)` is `<root>/.<short>/extensions`.
- The boundary check runs in the gate's `no_gpui` stage (`scripts/check-no-gpui.sh`).
