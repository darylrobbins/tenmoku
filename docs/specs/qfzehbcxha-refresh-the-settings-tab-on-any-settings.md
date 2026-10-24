---
task_uid: "01m5nmjny5erxn6hqfzehbcxha"
task: "T303"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The Settings tab re-reads its files on any change to a settings file, including an external edit that leaves every effective value unchanged, and for a workspace file under any open window's root; no second watcher is added. Tests external_edit_with_unchanged_effective_values_refreshes_the_tab, workspace_edit_under_a_second_windows_root_refreshes_the_tab"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Refresh the Settings tab on any settings file change, including unchanged effective values and other windows' roots (follow-up to Mac T1096)

## Problem
T295 (Mac T1096) narrowed C3 at review round 3: the Settings tab re-reads on the existing settings-reload notification, on activation, on scope switch and after its own writes, but not on an external edit that leaves effective values unchanged, nor for a workspace file under a root the existing watcher does not cover (a second window on another root). Review findings F2 and F3 asked for this.

## Change
- Forward file-change notifications from the kiln config service independently of effective-value deduplication (the config-service file-changed hook).
- Extend the existing watcher's paths to cover every current workbench root, per window, and update them when roots change.
- The Settings tab refreshes from both.

## Tests
`external_edit_with_unchanged_effective_values_refreshes_the_tab` and `workspace_edit_under_a_second_windows_root_refreshes_the_tab`, headless on Linux.

## Implementation notes

- kiln-core: `ConfigWatcher::extra_files()` hands out an `ExtraFiles` handle whose `set` replaces the files watched besides those given, on the running watcher's own thread (woken by an empty event). The next batch names each newly set file, or every file if a new directory was watched, as the first batch does. Directories stay watched once watched.
- kiln-platform: every batch the config service applies is published as the `ChangedFiles` global after the sources reload, whatever they changed: the file-changed hook, independent of `GlobalSettings` deduplication. `watch_files(cx, files)` returns a `FileWatch` guard that keeps files on the service's one watcher (no second watcher); `watched_files(cx)` lists them.
- fe-workbench: each `Workbench` holds a `FileWatch` on its root's `.fastedit/settings.toml`, made in `new` and replaced in `set_root`, dropped with the window. The Settings tab observes `ChangedFiles` and re-reads when a batch names the user's file or its root's.
- Scope widened to `crates/fe-workbench/src/workbench.rs` for the guard's field.
- The service still loads only the launch root's project settings; other windows' effective settings follow as before.
