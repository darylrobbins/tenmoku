---
task_uid: "01m7tygehsps99nrbcbp4xqve4"
task: "T2026"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln fingerprint records each listed path relative to the extension dir with its entry type (file, directory, symlink, other, missing, error) and an unfollowed symlink target; a listed directory also records whether it can be listed. Tests in new kiln/crates/kiln/tests/extensions_fingerprint_entries.rs show each of these calls on_extensions again: a retargeted directory symlink, a retargeted escaping or dangling link, an added or removed listed directory, and a directory that becomes unlistable (symlink cases cfg(unix)). Existing kiln extension tests pass unchanged."
    required: true
    manual: false
  - id: "C2"
    text: "Through the real watcher and reload path (config::with_extensions, batch_channel), tests in new crates/fastedit/tests/extension_consent_tree.rs: a_retargeted_directory_symlink_asks_consent_again (cfg(unix)) retargets a package directory symlink between two internal module dirs and gets one pending consent, with script and arguments unchanged and a different package_hash; an_added_or_removed_empty_directory_asks_consent_again re-asks once on the add and once on the removal; an unchanged batch asks nothing."
    required: true
    manual: false
  - id: "C3"
    text: "The Config::extension_files callback and the whole fingerprint walk never run on the UI thread, at install, on reload or on apply: extension_files_are_listed_off_the_main_thread in extensions_fingerprint_entries.rs records executor.is_main_thread() inside the callback and asserts every value is false."
    required: true
    manual: false
  - id: "C4"
    text: "Stale-result guard: a test in extensions_fingerprint_entries.rs shows that a walk whose result returns after a newer reload has started is dropped. on_extensions sees only the newer listing, and a following unchanged batch does not call it again."
    required: true
    manual: false
  - id: "C5"
    text: "fastedit lists every package directory (empty ones included) within T1978 PACKAGE_MAX_FILES cap; config::package_files_limited keeps its signature and results, and crates/fastedit/tests/extension_consent.rs is not edited and passes. Every test in the two new files compiles against base APIs. cargo clippy -p kiln and -p fastedit are clean with -D warnings, and scripts/check_kiln_boundary.py passes."
    required: true
    manual: false
  - id: "C6"
    text: "kiln never holds more than a fixed-size buffer of a listed file while hashing it (streamed, or size+mtime only above PACKAGE_MAX_FILE_BYTES): a test in extensions_fingerprint_entries.rs hashes a sparse file over 256 MiB, a symlink to it (cfg(unix)) and a named argument path, and shows peak read size stays bounded. Through the real watcher in extension_consent_tree.rs, an_added_oversized_file_reevaluates_consent (package incomplete, never granted) and a_removed_oversized_file_clears_the_incomplete_package each re-ask once (T1978 F8/F9)."
    required: true
    manual: false
---

# Watch extension package entries by relative path, type and link target, off the UI thread (review follow-up of T1978 F2, F6)

Review follow-up of T1978 F2 (high) and F6 (medium), codex review sa3m52 on candidate 6rgsqw. T1978 binds language-server consent to the whole installed package (`Invocation::package_hash`, which already covers directories, symlink targets and failures) and has fastedit list every file under the extension dir for kiln's watcher (`server_files` -> `package_files_limited`, crates/fastedit/src/config.rs ~687-745 on T1978's lane). Two gaps stay:

- **F2.** kiln's `ExtensionSource::fingerprint` (kiln/crates/kiln/src/extensions.rs ~632) canonicalizes each listed path and keeps only in-root regular files; a directory symlink, a dangling link and an escaping link all become `None`. Retargeting a directory symlink from one internal module dir to another changes `package_hash` and module resolution but not the fingerprint or the content hashes. fastedit never lists directories, so an empty directory being added or removed is not seen either. No consent re-check runs after any of these.
- **F6.** `files_of` (`Config::extension_files`, kiln/crates/kiln/src/setup.rs ~471) and `fingerprint()` run on the UI thread: at install (`Shared::install` ~995), on each reload (`reload` ~920) and in `apply` (~681). With T1978's whole-package listing, a big `node_modules` means up to `PACKAGE_MAX_FILES` (200k) read_dir, canonicalize and stat calls on the UI thread. T1978's cap bounds this; this card moves it off the UI thread.

Do:

- **kiln: record entries, not just files.** Each listed path is recorded under its path relative to the extension dir, joined lexically and not canonicalized, with its entry type from `symlink_metadata`: file, directory, symlink, other, missing, or error (keep the `io::ErrorKind`). A symlink also records its `read_link` target, unfollowed. A directory also records whether it can be listed (a `read_dir` open, not a walk). A file keeps size and mtime. For content hashing, a regular file, or a symlink that resolves to a regular file inside the root, keeps its canonical path, as now, so `hash_files` still sees edits behind a link. Compare paths as `PathBuf`; no native-byte encoding is needed, and no OS cfg goes outside kiln-core/src/platform or kiln-platform/src (`scripts/check_kiln_boundary.py`; files under `tests/` are exempt).
- **kiln: walk off the UI thread.** `FilesOf` becomes `Arc<dyn Fn(&Manifest) -> Vec<PathBuf> + Send + Sync>`, so `Config::extension_files` takes `Send + Sync + 'static`. All existing callers are non-capturing closures or plain fns: fastedit config.rs, crates/fe-workbench/src/plugin_dev_tests.rs and kiln/tests/extensions_fingerprint.rs. Update its doc. Install, reload and apply clone the enabled manifests and build the fingerprint and the hashes together in the one background task `hash` already spawns. The `stamps != fingerprints` dirty check moves into the completion. `apply` no longer fingerprints on the UI thread: the completion stores what was walked, as it does now. The existing `generation` counter is the stale guard: a result for an older generation is dropped, and `dirty` stays set until a current one applies. Install still publishes the set before the walk, as now.
- **fastedit: list directories too.** `server_files` also lists every directory under the package, empty ones included, through a sibling walk that shares T1978's `PACKAGE_MAX_FILES` cap. `config::package_files_limited(dir, max)` keeps its signature and current results: `listing_a_package_stops_at_the_entry_limit` in crates/fastedit/tests/extension_consent.rs pins them, and that file belongs to T2024. Don't edit it.

Tests:

- New kiln/crates/kiln/tests/extensions_fingerprint_entries.rs. Put symlink cases under `#[cfg(unix)]`. Record `executor.is_main_thread()` inside the callback, following kiln/tests/model_persist.rs ~51.
- New crates/fastedit/tests/extension_consent_tree.rs. Use the same `#[path = "../src/config.rs"] mod config;` / theme harness as extension_consent.rs (install, load through `batch_channel`, grant, send a batch for the extensions dir, then `run_until_parked`).
- Regatta overlays `**/tests/**/*.rs` onto the base, so every test in both new files must compile against base APIs only. A test that needs a new seam goes in a src-declared module instead. In the regression tests, make the first check that fails at base an `assert_eq!`/`assert_ne!`.

Gate notes: kiln clippy needs `cargo clippy --workspace` or `-p kiln`. Existing kiln extension tests (extensions_fingerprint.rs, extensions_reload.rs, extension_themes.rs, extension_keys_menus.rs), fe-workbench plugin_dev_tests and crates/fastedit/tests/extension_*.rs must pass unchanged.

Out of scope: snapshots and the decision file (T2024 owns lsp_consent*.rs, extension_consent.rs and vmtnmkay62). If the record needs a line about the watcher, report it to the lead.

Also from T1978's codex review ztsmwc, F8 and F9 (both medium), moved here by lead decision because only kiln's fingerprint and hashing can fix both:

- **F8.** kiln's `hash_files` (kiln/crates/kiln/src/extensions.rs) reads each listed file whole with `fs::read`. A very large file in the package, a symlink inside the package to one, or a command/argument path that `server_files` names is allocated in full before consent rejects the package (T1978 refuses a package with a file over `PACKAGE_MAX_FILE_BYTES`, 256 MiB, but only after the hash). Bound the read where kiln reads: hash through a fixed-size buffer (streamed), or above the limit record size and mtime only and never read contents.
- **F9.** T1978 dropped its config.rs size filter, so oversized files are listed again. Keep it that way: an oversized file must stay in the fingerprint, by size and mtime without reading its contents, so adding an unnamed oversized file re-evaluates consent (the package becomes incomplete and is never granted), and removing it from an incomplete package re-evaluates it again (the incomplete prompt clears).
