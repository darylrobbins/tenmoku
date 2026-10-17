---
task_uid: "01m55c0xzvqptxhfyhf06y7z52"
task: "T141"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "discovery_loads_in_id_order_and_rejects_duplicates: every directory under the root with an extension.toml loads, in id order; a missing root is an empty set; a second directory with the same id is an error naming both"
    required: true
    manual: false
  - id: "C2"
    text: "a_rescan_keeps_the_last_good_manifest: a manifest broken after loading keeps its last good version and reports its errors, a removed directory drops out, and a rescan that changed nothing compares equal"
    required: true
    manual: false
  - id: "C3"
    text: "a_tree_watcher_reports_a_new_extension_dir: a new extension directory under a watched root, and a root created after the watcher started, are each reported within 5 s (the 3 s re-stat plus margin)"
    required: true
    manual: false
  - id: "C4"
    text: "extensions_load_and_reload_through_init: with Config::extensions and watch_with batches, kiln::extensions::loaded lists the extension; adding one and sending the root lists both and calls on_extensions again with an app kind's table as written; a broken manifest's error is in kiln::platform::config::diagnostics and the last good stays; with fail_fast on, nothing panics"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/check_kiln_boundary.py passes and every new kiln file starts with the SPDX line"
    required: true
    manual: false
  - id: "C6"
    text: "Test (in the card's discovery test file, named by its full module path): discovering a 'GPL-3.0-only' extension emits one licence notice naming it and its licence; restarting discovery with the same id@version emits none; a new version emits one again; an 'MIT' extension emits none; the acknowledgement lives in the state dir"
    required: true
    manual: false
---

# Discover extensions in the user extensions directory and hot-reload them through kiln's config service (Mac T841)

Filed on argon from Mac T841 (card ~/coordination/argon-cards/zzn/T841.json rev 2; notes zzn and zzo, kiln extensions stage 1). Mac task uid 01m55bz3mt8rs5j400wdaj6wg4. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced). Updated per note zzo (the user's decisions): every extension carries an SPDX license checked against extensions.allowed_licenses; installed extensions live in the state dir. Respect scripts/check_kiln_boundary.py and give every new kiln file its SPDX header line. Argon dep: T140 (Mac T840). Title the host commit "... (Mac T841)".

## Problem

With the manifest loader (the manifest card this depends on), kiln still can't find
extensions or follow their changes. The extensions record (§3, §5) puts user extensions in
`<state dir>/extensions/<dir>/extension.toml` (AppPaths::extensions_dir; the user chose the state dir), loaded in id order, hot-reloaded like the
user's config files with last-good versions and diagnostics in the config problems list.

Today the config service only watches a fixed list of files: `ConfigSource::paths()` is read
once in `ConfigService::start` (`kiln/crates/kiln-platform/src/config.rs` ~327), and
`ConfigWatcher` watches each file's parent directory non-recursively
(`kiln/crates/kiln-core/src/watch.rs`), so a new extension directory is never seen.

## Change

**kiln-core, `kiln/crates/kiln-core/src/extension.rs`:**
`pub struct ExtensionSet` with `discover(root: &Path, api: &ExtensionApi) -> ExtensionSet`:
every directory directly under `root` with an `extension.toml`, loaded with `load_manifest`,
in id order; a missing `root` is an empty set with no diagnostics. Two directories with one
id: the first by directory name loads, the other gets an error naming both. `rescan(&mut self)`
re-reads it: an extension whose manifest now has errors keeps its last good `Manifest`
(and reports the errors), a removed directory drops out. `extensions() -> &[Manifest]`,
`diagnostics() -> &[Diagnostic]`, and `PartialEq` so a rescan that changed nothing is seen as
such.

**kiln-core, new `kiln/crates/kiln-core/src/watch_tree.rs`** (`pub mod watch_tree;` in
`lib.rs`): `TreeWatcher::new(roots: Vec<PathBuf>, sink: impl FnMut(Vec<PathBuf>) + Send)`:
watches each root recursively with `notify`, and every `watch::RECONCILE` re-stats each root's
tree (directory listing, and each file's `platform::fs::FileStamp`), reporting a root whose
tree changed though no event said so (FSEvents drops events under a flood). A missing root is
picked up by the re-stat once it exists. Dropping it stops its thread.

**kiln-platform, `kiln/crates/kiln-platform/src/config.rs`:** `ConfigSource::trees(&self) ->
Vec<PathBuf>` (default empty): directories watched recursively; a change under one is a batch
naming the root. `ConfigService::start` runs one `TreeWatcher` over every source's trees,
sending into the same batch channel as its `ConfigWatcher`; `start_with` takes the batches as
before, so tests send `[root]` themselves.

**kiln facade:** new `kiln/crates/kiln/src/extensions.rs` (`pub mod extensions;` in
`kiln/crates/kiln/src/lib.rs`), and in `kiln/crates/kiln/src/setup.rs`:
`Config::extensions(dir: PathBuf, app: &str, api_version: ApiVersion)` (`app` the
`[engine]` key manifests must name, the app's `AppId::short`), `Config::extension_kind(name,
schema)` for each app kind, and `Config::on_extensions(f: impl FnMut(&ExtensionSet, &mut App))` (each call adds a
callback, called in the order added: one per app kind is fine).
`init` discovers the set (reserved namespaces: those of every command registered by then),
adds an `ExtensionSource: ConfigSource` whose `trees()` is `[dir]`, calls `on_extensions` once
at install and again after each rescan that changed the set, and publishes the set's
diagnostics through `ConfigDiagnostics` with the other sources'. `kiln::extensions::loaded(cx)
-> Vec<Manifest>` lists what's loaded. `check_fail_fast` never sees extension diagnostics.
`init`'s "nothing to watch" branch (`service.paths().is_empty()`, `setup.rs` ~610) counts the
sources' trees too, or an app with extensions and no user files would never reload them.

Every new kiln file starts with the SPDX line, nothing in `kiln/` names fastedit
(`scripts/check_kiln_boundary.py`). Tests: `kiln/crates/kiln-core/tests/extension_discovery.rs`
(set and rescan, on `tempfile` dirs), `kiln/crates/kiln-core/tests/watch_tree.rs`,
`kiln/crates/kiln/tests/extensions_reload.rs` (headless, `kiln::init` with `Config::watch_with`
batches, as `kiln/crates/kiln/tests/config.rs` does).

## Out of scope

Applying themes, keys, menus (their own cards); workspace extensions, disabling, archives
(later cards); fastedit's wiring of its kinds (the LSP and grammar cards).

## Licence notice (USER decision 2026-10-17, licence option 3)

The first time discovery finds an extension whose `License::class` (T840) is Copyleft or Unknown, at a
given id and version, it emits a one-time notice through kiln's notification/toast path: "<name> (<id>) is
licensed <expr>. fastedit doesn't ship it; you installed it." The acknowledgement is remembered in the state
dir (e.g. `<state dir>/extensions-acknowledged.toml`, id@version entries) so it never repeats for that
version. Permissive extensions show nothing.

## Implementation notes

- kiln has no toast path of its own (fastedit's `ToastQueue` is the app's), so the licence
  notice is `ExtensionSet::license_notices(state_dir) -> Vec<LicenseNotice>` in kiln-core,
  which claims the notices under a lock on `<state_dir>/extensions-acknowledged.toml.lock`
  (`trust::locked`, shared with `TrustStore::update`), re-reading and writing the union of
  `acknowledged = ["id@x.y.z", ...]`, so two processes never both give one or drop the
  other's entries. `pending_license_notices` reads without claiming. The facade hands each
  notice to `Config::on_license_notice` callbacks; with none, notices are only logged (once
  per run) and stay unacknowledged, so a later launch that shows them still does. The state
  dir is the absolute extensions directory's parent (`AppPaths::extensions_dir` is
  `<state>/extensions`). `LicenseNotice::message(app)` takes the app's name, so nothing in
  kiln names fastedit.
- A directory that held an extension keeps its last good manifest until the directory itself
  is gone: a manifest removed or unreadable is an error (review round 1 F1).
- `TreeWatcher` follows symlinks in its re-stat (each directory once, so loops end), stats
  their targets, and watches a linked directory outside the root recursively and a linked
  file's directory non-recursively; events there count for the root. A tree past 10,000
  entries is reported on every re-stat (F2, F4).
- `ExtensionSet::rescan` returns whether the set (extensions or diagnostics) changed, besides
  `PartialEq`. An unreadable (not missing) root keeps what was loaded and reports one error.
- `Config::allowed_extension_licenses(ids)` reaches `ExtensionApi::allowed_licenses`.
- Reserved namespaces: those of the app registry's commands and the typed commands at
  `init`.
- `TreeWatcher`'s first batch names every root that exists, as `ConfigWatcher`'s first names
  every file; a root removed and made again is re-watched once the re-stat sees it.
- Carried T140 F5: the SPDX tokenizer now refuses a `+` run into the next token
  (`GPL-2.0+OR MIT`, `MIT+AND Apache-2.0`, `GPL-2.0+WITH ...`), tested in
  `kiln-core/tests/extension_manifest.rs`.
