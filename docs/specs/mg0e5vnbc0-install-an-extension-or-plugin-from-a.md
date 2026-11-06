---
task_uid: "01m6j08gxtc2vgwjmg0e5vnbc0"
task: "T280"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "extension_install::extension_install_tests::installs_a_zip_a_tar_gz_and_a_folder: each lands as \u003cextensions dir\u003e/\u003cid\u003e with its files, from extension.toml at the root or in one top-level directory, and hot reload lists it"
    required: true
    manual: false
  - id: "C2"
    text: "extension_install::extension_install_tests::refuses_entries_that_escape_or_link: an absolute entry, a .. escape, a symlink, a hard link and a special file are each refused and leave nothing in the extensions directory"
    required: true
    manual: false
  - id: "C3"
    text: "extension_install::extension_install_tests::refuses_a_bad_manifest_or_an_oversized_archive: no extension.toml, a manifest error, a bad [contributes.plugin] entry, over 10000 entries or over 64 MiB unpacked are refused with the reason and leave nothing behind"
    required: true
    manual: false
  - id: "C4"
    text: "extension_install::extension_install_tests::replacing_an_installed_id_asks_first: the prompt names both versions; Cancel keeps the old directory, Replace swaps it"
    required: true
    manual: false
  - id: "C5"
    text: "extension_install::extension_install_tests::an_installed_plugin_waits_for_its_grant: with the preview on, an installed plugin shows its grant prompt and loads nothing before Allow; with it off, the toast says to turn on extensions.plugins_preview"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; cargo deny check bans licenses sources passes"
    required: true
    manual: false
---

# Install an extension or plugin from a local .zip, .tar.gz or folder with Extensions: Install from File or Folder, checked and unpacked safely into the user extensions directory (Mac T1349)

## Problem

An extension is installed today by copying its directory into `<state dir>/extensions/` by hand. The
record (§2) gives an archive form, the directory zipped with `extension.toml` at its root, and leaves
installing it to a later card. For the preview (user, 2026-11-03) installs are local and unsigned:
the grant prompt is the trust gate for a plugin, the consent prompt for a language server.

## Change

- `crates/fe-workbench/src/extension_install.rs` and `extension_install_tests.rs` (`mod` in
  `lib.rs`): `install_extension(source, extensions_dir) -> Result<Installed, InstallError>`, run off
  the main thread, for any extension (declarative or plugin):
  - **Sources.** A `.zip`, a `.tar.gz` or `.tgz`, or a folder (copied, not linked; linking is the
    development card's). `extension.toml` at the root, or inside a single top-level directory.
  - **Safety.** Unpack into a fresh temporary directory inside the extensions directory, then rename
    into place. Refuse, leaving nothing behind: an absolute entry, a `..` escape, a symlink or
    hard link, a device or other special file, more than 10,000 entries, or more than 64 MiB
    unpacked.
  - **Check.** The manifest goes through kiln's extension check (id, name, version, licence,
    `[engine] tenmoku`), and a plugin entry through `PluginManifest::from_extension` with
    `ExtensionOrigin::User`; any error refuses the install with the diagnostic's text.
  - **Place.** `<extensions dir>/<id>`. An installed id asks first with a native prompt naming both
    versions ("Replace" or "Cancel"); replacing swaps the directories, so a failure leaves the old
    one. Hot reload picks the result up; a plugin then waits for its grant as any new plugin does.
- The typed command `workbench.install_extension`, "Extensions: Install from File or Folder…"
  (`workbench_commands.rs`, ids where `typed_command_ids_tests.rs` and `command_catalog_tests.rs`
  require), asks for a path (`prompt_for_paths`, files and directories) and toasts the outcome:
  "Installed <name> <version>", plus for a plugin either "It asks before it runs" or, with the
  preview off, "Turn on extensions.plugins_preview to run it".
- Dependencies: `zip` (deflate only) and `tar` with `flate2` (already in `Cargo.lock`), smallest
  feature sets, in the root `[workspace.dependencies]` and `crates/fe-workbench/Cargo.toml`. Tests
  build their archives at run time with the same crates; no binary fixtures.

## Notes

Archive handling lives app-side, not in kiln: it keeps the archive crates out of kiln and kiln/**
untouched while T1268 holds it; move it to kiln if a second kiln app needs it. The root `Cargo.toml`
edit is confined to `[workspace.dependencies]`, apart from T1267's `[workspace.package]` licence
line. Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- kiln exposes neither the `ExtensionApi` it loads with nor its extensions directory, and kiln and
  the app crate are outside this card's scope, so the workbench builds the check's API itself
  (`extension_install::check_api`): the app's id, API 1.1, `[contributes.plugin]`, and the reserved
  namespaces from the live command lists, as kiln computes them. The app's other kinds (`lsp`,
  `grammars`, `snippets`) are unknown to that check, a warning only; the loader checks them as they
  load. The directory is `fe_config::paths::app_paths().extensions_dir()`, the one the app hands kiln.
- The staging directory, `<extensions dir>/.installing-<pid>-<nanos>-<n>`, unpacks into an
  `unpacked/` folder inside it, so it never holds an `extension.toml` of its own and a rescan while it
  exists doesn't take it for an extension. A `__MACOSX/` folder in an archive is skipped.
- Unpacked size counts the bytes actually written, not what headers claim; executable bits are kept
  (for scripts a language server runs), other modes are the umask's.
- A tar is read header by header (raw), so its long-name and pax headers count as entries, each at
  most 64 KiB, and every entry's declared size counts against the 64 MiB before it's read or skipped;
  a pax `size` must agree with the entry's own header. A zip whose names repeat is refused. A
  replaced id is found by manifest in any directory, compared by file identity. Known limit, filed as
  T292: a zip's central directory is indexed whole before its entry count is checked.
