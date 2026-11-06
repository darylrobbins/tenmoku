---
task_uid: "01m6j08h2r7cdh9rvr4fq2fz2t"
task: "T281"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "plugin_dev::plugin_dev_tests::a_linked_folder_installs_as_a_link: the command creates \u003cextensions dir\u003e/\u003cid\u003e as a symlink to the folder after checking its manifest, and refuses when the id is taken by anything but that link"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_dev::plugin_dev_tests::rebuilding_a_linked_plugin_reloads_it: rewriting the linked .wasm with the same capabilities reloads the plugin without a prompt; one asking for more prompts and keeps the old version meanwhile"
    required: true
    manual: false
  - id: "C3"
    text: "plugin_log::plugin_log_tests::log_lines_reach_the_plugin_log: lines from the log import, with level and time, and load and unload diagnostics are kept per plugin, the last 1000"
    required: true
    manual: false
  - id: "C4"
    text: "plugin_log::plugin_log_tests::show_plugin_log_opens_a_following_tab: the command picks a plugin and opens its log read-only; a new line appears in the open tab"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Develop a plugin in place: link a folder as an installed extension that reloads when its .wasm is rebuilt, and read each plugin log with Extensions: Show Plugin Log (Mac T1350)

## Problem

A plugin author can't iterate in place: each rebuild means reinstalling, and the plugin's `log`
import goes only to Tenmoku's own log, mixed with everything else.

## Change

- `crates/fe-workbench/src/plugin_dev.rs` and `plugin_dev_tests.rs` (`mod` in `lib.rs`): the typed
  command `workbench.link_extension_folder`, "Extensions: Install Folder for Development…", asks for a
  folder, checks its manifest as the install card does, and creates the symlink
  `<extensions dir>/<id>` pointing at it (refused if `<id>` exists and isn't that same link). The
  linked extension reloads when its manifest or `.wasm` changes, through T1062's reload (`wasm` in
  `extension_files`); if the watcher doesn't see changes behind the link, the plugin host watches the
  linked plugin's `.wasm` itself, or the 3 s reconcile catches it; say which. A rebuild with the same
  capabilities asks nothing; one asking for more prompts as any escalation. Show Installed lists it
  with its linked path as the source. Uninstall (T1063) removes the link, never the folder.
- `crates/fe-workbench/src/plugin_log.rs` and `plugin_log_tests.rs`: a per-plugin log of the last
  1,000 lines (time, level, message) from the plugin's `log` import (the runner's log sink, as
  `Plugin::load_logging_to` takes one), plus its load, reload and unload diagnostics. The typed
  command `workbench.show_plugin_log`, "Extensions: Show Plugin Log", picks a plugin and opens its log
  in a read-only tab that follows new lines, as `lsp.show_log` shows a server's.
- Command ids where `typed_command_ids_tests.rs` and `command_catalog_tests.rs` require.

## Notes

Host-independent: symlinks and the watcher behave on Linux; tests bound any wait on reload well under
2 s or drive the reconcile directly.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- **Which watch.** The extensions service's own watcher sees changes behind the link: kiln's
  `TreeWatcher` follows a symlink under a root to a directory outside it and watches that
  recursively, so a rebuilt `.wasm` is a batch naming the extensions directory, and T1062's
  reload (fingerprint, hash, `set_plugin_extensions`) reloads the plugin. The plugin host adds no
  watch of its own; the 3 s re-stat is only the usual fallback. `rebuilding_a_linked_plugin_reloads_it`
  checks the watcher reports a rewrite behind a link made after it started (each wait bounded at
  1.5 s), then drives kiln's reload with the test's batch channel.
- **Check.** `link` reuses the install card's `check` (now `pub(crate)` in `extension_install.rs`,
  with `installed_with`, `extensions_dir` and a few text helpers). It also refuses an id installed
  in another directory, and a folder that is, holds or is inside the extensions directory.
- **Show Installed.** A row whose directory is a symlink shows `Linked from <folder>` as its
  source (`extensions_picker.rs`). Uninstall already removed a link, never its target.
- **Show Plugin Log** reuses the extensions picker over the user's plugins (a new `Purpose`), as
  Revoke and Uninstall do. The tab's buffer is marked saved after each appended line, so closing
  it never asks to save.
- **Diagnostics logged:** `loaded`/`reloaded <version>` (Info), `failed to load: …` and a manifest
  or grant failure (Error, once per change of state), `stopped: …` when a call unloads it (Error),
  `its shutdown failed: …` (Error), `can't record the answer to its prompt: …` (Error), and
  `shut down` with why (Info) when the host unloads it.
- **The tab** opens active over the pane's editor, drops its oldest lines past 1,000 as the log
  does, and follows its buffer, not its first view: lines go through whichever editor still shows
  it, in any window, so a split survives closing the original.
- Scope widened by respec to `extension_install.rs` and `extensions_picker.rs` for the above.
