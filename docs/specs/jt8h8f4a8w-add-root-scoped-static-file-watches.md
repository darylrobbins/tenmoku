---
task_uid: "01m78tht2f5mg7qwjt8h8f4a8w"
task: "T364"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`tests::registration::static_watch::change_under_root_b_reaches_only_root_b_session` and `tests::registration::static_watch::blacklisted_folder_change_reaches_neither` pass"
    required: true
    manual: false
  - id: "C2"
    text: "`tests::registration::static_watch::lookup_files_change_changes_what_is_sent` passes (watch_files_from follows settings.cssVariables.lookupFiles)"
    required: true
    manual: false
  - id: "C3"
    text: "`tests::registration::index_events::notified_after_each_notification` and `tests::registration::index_events::repoll_due_on_manual_clock` pass"
    required: true
    manual: false
  - id: "C4"
    text: "`tests::registration::index_events::repoll_ms_parsed_and_defaults_off` passes and settings.schema.json and settings-template.toml list watch_files, watch_excludes and repoll_ms"
    required: true
    manual: false
---

# Add root-scoped static file watches, Notified events and a repoll timer for language servers (LSP add-ons A8, T1645) (Mac T1738)

# Add root-scoped static file watches, Notified events and a repoll timer for language servers (LSP add-ons A8, T1645)

Spec: §5 (polling, watches, index-change events), §2.4 (what it scans), §7 (Watches), §8 row A8.

Row: root-scoped static watches and index-change events. `ServerSpec::watch_files` / `watch_excludes` and `[lsp.servers.<name>]` keys of the same names hold globs relative to the session's root; a path outside the root never matches. Alternatively `watch_files_from` / `watch_excludes_from` are JSON pointers into the spec's `settings`, read again at each configure. Either way the result is held as a permanent `didChangeWatchedFiles` registration of the session. Adds `LspEvent::Notified { server, what }` after each `didChangeWatchedFiles` and `didChangeConfiguration` sent, and `ServerSpec::repoll` / `[lsp.servers.<name>] repoll_ms` with `LspManager::repoll_due(server, now)` on the manager's `Clock`.

Test files: crates/fe-lsp/src/tests/registration/static_watch.rs (two roots with the same variable names, excluded folders, a `lookupFiles` change) and index_events.rs (`Notified` after each notification; `repoll_due` on a manual clock; `repoll_ms` parsed, defaulting off). If the new keys need a settings .ftl label outside --scope, stop and say so.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

Lead addition (2026-11-12): the settings label .ftl files (crates/fe-config/l10n/en-US/settings.ftl, settings-template.ftl, crates/fe-workbench/l10n/en-US/settings.ftl) are in scope for the new keys' labels.

## Implementation notes

- **Spec and settings.** `ServerSpec` gains `watch_files`, `watch_excludes`,
  `watch_files_from`, `watch_excludes_from` and `repoll: Option<Duration>`;
  `LspServerSettings` gains the same five keys, with `repoll_ms` (0 or unset
  is off). In `registry::apply`, a list set in settings clears the spec's
  pointer of that list, unless settings set a pointer too, so a user's
  `watch_files` beats a built-in `watch_files_from`. The project-layer filter
  already drops every key but `enabled`; `crates/fe-config/tests/lsp_watch_settings.rs`
  pins it for the five. They aren't `settings`, so `needs_restart` restarts
  on a change to any of them, as for `add_on`.
- **The watch** (`registration::StaticWatch`) is built at each session's
  start and again at each `Configure`, before its `didChangeConfiguration`
  goes, so a change on disk queued after a settings change is filtered by
  the new pointers' lists. It lives in `Registrations` beside the id map:
  `registrations()` doesn't list it and `unregisterCapability` can't remove
  it. Globs are matched against the path relative to the session's root
  (a leading `./` dropped); a path outside the root, or with a `.`/`..`
  component, never matches. An exclude matching the path or any folder above
  it, up to the root, leaves it out, as fast-glob's `ignore` does with
  `**/node_modules`. The globs see the path's parts joined by `/`, so a
  Windows path's backslashes separate folders as `/` does (review round 1,
  F1), while on Unix a backslash stays part of a name. Every kind of change
  counts. A files glob that doesn't
  build is skipped; an exclude that doesn't build leaves no watch, rather
  than a wider one; a pointer at anything but an array of strings gives an
  empty list. Each is logged.
- **Known difference from the server's scan:** globs are LSP globs, so `*`
  and `**` match dot-folders, where fast-glob's default (`dot: false`)
  doesn't scan them. So a change under, say, `.storybook/` is sent though
  the startup scan skipped it; the server then reads that file. A9's
  `blacklistFolders` lists the dot-folders that matter (`.git`, `.cache`,
  `.next`, `.worktrees`, …).
- **`LspEvent::Notified { server, what: Notice }`** (`Notice::WatchedFiles`
  or `Configuration`, re-exported) is sent on the server's thread right after
  the notification is handed to async-lsp: for each `Configure`, and for each
  `notify_sessions` notification sent as `didChangeWatchedFiles` (filtered
  `files_changed` included) or `didChangeConfiguration`. Nothing sent,
  nothing reported.
- **`LspManager::repoll_due(server, now) -> Option<Instant>`** is the latest
  poll due by `now`: the session's initialization plus the most whole
  `repoll` periods before `now`, `None` before the first, without `repoll`,
  before initialization, or once the session has ended: `end_session` clears
  it before a shutdown, a crash's kill and backoff, and after the last crash
  (review round 1, F2). A host polls each time
  the value changes. Each session counts from its own initialization. It
  lives in watched_files.rs and reads the session through `manager.rs`'s
  `session`, now `pub(crate)`.
