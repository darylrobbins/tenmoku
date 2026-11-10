---
task_uid: "01m712rq7bbqvmnacp31jkc1z4"
task: "T1666"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "New test crates/fastedit/tests/reopen_last_session.rs: after a session with two folder windows and one rootless window quits, a no-argument launch reopens all three on their folders with their tabs"
    required: true
    manual: false
  - id: "C2"
    text: "A no-argument launch whose last-quit folder no longer exists skips it and opens the rest; with nothing to restore it opens today's rootless restore or an empty window"
    required: true
    manual: false
  - id: "C3"
    text: "Folder and file launches restore exactly as before (existing launch tests unchanged and passing)"
    required: true
    manual: false
---

# A launch with no folder reopens the windows open at the last quit on their own folders (VS Code's restoreWindows), not only the last no-folder session

# A launch with no folder reopens the windows open at the last quit, whatever their folders

## User request (2026-11-09)

On v0.1.3 the user reopened Tenmoku (no folder argument: Dock, Finder or `open -a`) and their project did not come
back; only files from the last no-folder session did. They want the previous project back, as VS Code does
(`window.restoreWindows`, default: the last session's windows).

## Today

crates/fastedit/src/main.rs `launch_windows`: "A folder launch restores the root's saved windows, and a launch with
no argument (no root) those of the last such launch". `layout_store(None)` reads only the rootless key's layouts, so a
no-argument launch never reopens a folder window. kiln-core `LayoutStore::restore_last_session` (T1259) already
limits a root's restore to the last session's windows (at most 8).

## Change

A launch with no argument reopens every window that was open when the app last quit, each on its own folder (or
none), at most MAX_RESTORED, with their tabs, layouts and backups as a folder launch restores them today. A folder
launch and a file launch keep today's behaviour. When the last quit left no windows (or a saved folder is gone),
fall back to today's rootless restore, then to an empty window; a missing folder is skipped, not an error.
The workspace trust prompt still applies to each restored folder as on a folder launch.

## Implementation notes

- **Finding the last quit's windows.** Layout files are keyed by a hash of their root, so a no-argument launch had
  no way back from a file to its folder. Every write now records the store's `root` in the file (when it's UTF-8;
  the rootless store records `""`). kiln-core `restore_last_quit(dir, max, there)` reads every `<key>.json` in the
  layouts directory and claims each entry a folder launch may restore (not a file launch's) that no live window
  holds. The last session is the session of the newest claimed entry; a store is restored if its newest claimed
  entry is of it. The claims are held from discovery through restoring, so another process can't take a
  last-session window in between and leave an older session's in its place (review F3). Of a restored store, the
  older sessions' windows are deleted and the last session's re-written, as `restore_last_session` does. Other
  stores are untouched, and so are file-launch entries. At most `MAX_RESTORED` windows open in all, newest first;
  the ones past the cap are left as they are.
- **Each window reopens on the folder it was on** (review F1). The workbench records its current root in its entry
  (`LayoutKeys`' `root`), set on open and on every `set_root` (Open Folder), which also re-saves the layout. A
  restored window uses its entry's `root`, falling back to its store's, and keeps saving in its original store, so
  its layout and hot-exit backups stay where they were. A window whose folder is gone (`there` false) is skipped
  and its entry kept.
- **Files from before roots were recorded** (v0.1.3 and older) are matched by key: the empty root's key is the
  rootless store's; otherwise the ancestors of the absolute paths an entry holds (an editor's file) are hashed
  until one matches the file's key. A file with no such path is skipped until a window on its root saves again.
- **The host** (`crates/fastedit/src/reopen.rs`, split out of main.rs so the integration test runs the host's own
  choice of windows): a launch with no root and no file reopens those windows. With none, it falls back to the
  rootless store's launch as before, then to a fresh window. The first window's root becomes the launch root, so
  its settings and workspace extensions load as a folder launch's do. The other windows open each on its own root
  and get their settings through `watch_window`/`settle`, as Open Folder in New Window's do. A rootless window
  beside a folder launch root now reads the user's settings alone, rather than the launch root's project layer
  (`settle`, review F2). Each workbench resolves its own root's trust (`Workbench::new`), so the trust prompt
  applies to each restored folder. Folder and file launches choose their windows exactly as before, with the
  window root as given on the command line.
