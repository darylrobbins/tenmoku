---
task_uid: "01m69eqqt4kwgmqhyqdnbz9c8b"
task: "T1259"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::window_restore::a_folder_launch_restores_only_the_last_session passes (fails at base): last session only, older sessions pruned, crash recovery, at most 8 legacy entries, a file launch's window never restored, live windows' entries untouched"
    required: true
    manual: false
  - id: "C2"
    text: "LayoutStore::write stamps session = kiln_core::layout::session(); LayoutStore::restore_last_session(max) keeps the newest session's non-file-launch claimable entries (at most max, re-written under this session) and deletes the other claimable entries with release(true); restorable() unchanged, newest_restorable skips restore:false"
    required: true
    manual: false
  - id: "C3"
    text: "kiln_shell::layout::launch(store, true) uses restore_last_session(MAX_RESTORED = 8) and logs the dropped count once; launch(store, false) windows write \"restore\": false; WindowLayout::new's signature is unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "New-API tests in kiln/crates/kiln-core/src/layout_session_tests.rs (declared from layout.rs) pass; layout tests that strip id/saved also strip session; module docs of both layout.rs files describe the last-session rule"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p kiln-core -p kiln-shell -p fe-workbench passes and cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Folder launch reopens every window ever saved for it: restore only the last session's windows (at most 8), never file-launch windows

## Problem

`fastedit .` in `~/code/fastedit` (release build of main 83f5445) opens dozens of windows. The
saved layout `~/.local/state/fastedit/layouts/347dae04c9299978.json` held 55 window entries for
that folder, 29 of them at the default bounds (x 264, y 175, 1200x800): windows opened fresh and
never placed (or killed before a quit save wrote their bounds). The layouts dir holds 56
`347dae04c9299978.<id>.lock` files created one at a time on 56 different occasions between
Oct 11 and Oct 31, so a fresh window id was claimed and kept roughly once per run.

## How entries accumulate (verified in code)

A folder launch restores **every** entry no live window holds
(`kiln/crates/kiln-shell/src/layout.rs:470` `launch` -> `kiln/crates/kiln-core/src/layout.rs:173`
`LayoutStore::restorable`), and nothing ever removes an entry except a window closing while
another window **of the same process** stays open (`layout.rs:580` `WindowLayout::close`,
`others_open` from `kiln/crates/kiln-shell/src/workspace.rs:839`). Every window writes its entry
as soon as it opens (`workspace.rs:602` `set_layout` defers a first `save_layout`), quitting keeps
every entry (`mark_quitting` / `save_at_quit`, `layout.rs:501-575`), and so does closing the last
window or the process dying. So every window that ever got a **fresh id** and was still open at
quit, crash or kill stays in the file forever and comes back at every later folder launch.
Fresh ids are claimed by:

1. **File launches.** `fastedit <file>`: `resolve_launch` (`crates/fe-workbench/src/workbench.rs:88`)
   roots the window at the file's parent, and `launch_windows(root, folder = false)`
   (`crates/fastedit/src/main.rs:285`, called at `main.rs:457` with `reading.is_none()`) calls
   `launch(store, false)`, which always `claim_new`s a fresh id in that folder's store
   (`layout.rs:477-481`). The window saves its entry like any other and keeps it at quit, so
   every `fastedit regatta.toml` / `fastedit TESTING.md` / `fastedit --perf <file in the root>`
   (perf quits through `cx.quit()`, `crates/fastedit/src/perf.rs:551`, which runs `save_at_quit`)
   adds one window that the next `fastedit .` restores. The one entry left today shows
   `regatta.toml`.
2. **A second process on the same root.** There is no single-instance forwarding. `fastedit .`
   while another fastedit process has the folder open finds every entry claimed, so `launch`
   falls back to `claim_new` (`layout.rs:477`); each process's last window keeps its entry
   (`others_open` only sees its own process's windows). Each overlap adds a window for good.
3. **Killed or crashed processes** (`--perf` runs killed under a screen lock, agents' runs killed by
   process group, crashes) keep every entry with its first-save bounds. That is crash recovery
   working as intended, but combined with 1 and 2 nothing ever trims the pile.
4. **New windows** (`open_window_on`, `main.rs:243` -> `new_window`, `layout.rs:488`) claim the
   newest restorable entry, else a fresh id; one still open at quit keeps its entry. That alone is
   correct (it was open when the app quit), but under today's rule it is never retired either.

Not a cause: the screenshot examples (`fe-gpui`, `fe-workbench`, `kiln` `examples/screenshots`)
never create a `LayoutStore` or call `set_layout`; regatta lanes are other clones, so other
layout keys; `set_root` (`crates/fe-workbench/src/workbench_root.rs:121`) doesn't move a window's
entry to another store.

## Fix: restore the last session, like VS Code and JetBrains

A folder launch restores the windows of the **last session** on that folder (the windows open when
the app last quit, or when it crashed or was killed), never every entry ever saved. File-launch
windows and older sessions' windows are pruned, and a cap bounds what one launch reopens.

### kiln-core (`kiln/crates/kiln-core/src/layout.rs`)

- `pub fn session() -> u64`: this process's session id, random, fixed for the life of the process
  (a `OnceLock` over `fresh_id()`).
- `LayoutStore::write` stamps `"session": session()` beside `id` and `saved`.
  `pub fn entry_session(entry: &Value) -> Option<u64>`; an entry saved before this change has no
  `session` and counts as `None` (all such entries are one legacy session).
- New `LayoutStore::restore_last_session(&self, max: usize) -> LastSession`
  (`pub struct LastSession { pub windows: Vec<(Claim, Value)>, pub dropped: usize }`):
  1. claim every entry no live window holds, newest first (as `restorable` does). Entries a live
     window holds, in this process or another, are never touched;
  2. of those, drop every entry marked `"restore": false` (a file launch's, below);
  3. the last session is the `entry_session` of the newest remaining entry; keep that session's
     entries, newest first, at most `max`;
  4. re-write each kept entry at once with `write` (oldest first, so their order stays), so they
     carry this process's session: a crash right after launch still brings them back, and later
     windows of this run join the same session;
  5. delete every other claimed entry (`delete`) and release its claim with `release(true)` (lock
     file removed); `dropped` counts them.
- `restorable()` is unchanged (tests use it). `newest_restorable()` skips `"restore": false`
  entries.
- Module docs: describe sessions, the last-session rule, file-launch entries and pruning.

### kiln-shell (`kiln/crates/kiln-shell/src/layout.rs`)

- `pub const MAX_RESTORED: usize = 8;` the safety net.
- `launch(store, true)` uses `store.restore_last_session(MAX_RESTORED)`; when `dropped > 0` it logs
  once at info: `"dropped {n} saved windows of {file}: older sessions, file launches, or beyond
  {MAX_RESTORED}"`. Falling back to one fresh window when nothing is kept is unchanged.
- `launch(store, false)` (a file launch) gives its window a layout marked as a file launch:
  `WindowLayout` gains a private `restores: bool` (`true` from `WindowLayout::new`, whose signature
  is unchanged); when `false`, every entry it writes (`save`, `save_if_changed`, `save_at_quit`,
  `close`) carries `"restore": false`. Such an entry still names the window's hot-exit backups
  until the next folder launch prunes it.
- `new_window` is unchanged apart from `newest_restorable` skipping file-launch entries.
- Module docs: the launch rule above (last session, cap, file launches never restored).

### Hot exit

No change. Backups of a pruned entry become backups "no saved layout names", which
`crates/fe-workbench/src/hot_exit.rs` (module docs lines 36-41, `named` at ~170) already opens read
as tabs in a restored window, so pruning loses no unsaved text. Pruning happens in `launch`, before
any window opens and computes `named`.

### Existing tests

Entries now carry `session`. Tests that compare whole entries after stripping `id` and `saved` also
strip `session`: `kiln/crates/kiln-shell/tests/layout.rs` (~387),
`kiln/crates/kiln-shell/tests/workspace_layout.rs` (~176),
`crates/fe-workbench/src/layout_compat_tests.rs` (~172). These edits must compile at base (they only
add `object.remove("session");`).

New tests of new APIs (`session()`, `restore_last_session`) go in a new
`kiln/crates/kiln-core/src/layout_session_tests.rs` declared from `layout.rs` with
`#[cfg(test)] #[path = "layout_session_tests.rs"] mod session_tests;` (not under `src/tests/`,
which is carried to base): `restore_last_session` with a live claim held on an older entry leaves
it in the file, and an entry written by this process carries `session()`.

## Regression test (base APIs only)

New file `crates/fe-workbench/src/tests/window_restore.rs`, declared `mod window_restore;` in
`crates/fe-workbench/src/tests/mod.rs`. Id: `tests::window_restore::a_folder_launch_restores_only_the_last_session`.
Uses only `kiln_shell::layout::{launch, LayoutStore, WindowLayout}`, `WindowLayout::id`/`close`,
`LayoutStore::{new, file, entries, claim}`, `serde_json`, `tempfile`, `gpui::TestAppContext`.
A helper writes a layout file by hand (`{"version": 1, "windows": [...]}` at `store.file()`); window
ids are compared sorted.

1. Store A holds id 1 (`saved` 1000, `session` 7) and ids 2, 3 (`saved` 2000, 2001, `session` 9).
   `launch(&store, true)` restores ids `[2, 3]`: **`assert_eq!` first** (base gives `[1, 2, 3]`).
   After dropping the windows, `store.entries()` ids are `[2, 3]` (1 pruned).
   A second `launch(&store, true)` (the first process "crashed": its claims are gone) restores
   `[2, 3]` again: crash recovery holds.
2. Store B holds 20 legacy entries without `session`, ids 100..=119, `saved` 1000+i:
   `launch(&store, true)` restores exactly the newest 8, ids 112..=119 (`assert_eq!`); after
   dropping them, `store.entries().len()` is 8.
3. Store C: `launch(&store, false)` (a file launch) gives one window; `close(json!({"panes": {}}),
   false, cx)` then `cx.run_until_parked()` keeps its entry, as the last window closing does.
   `launch(&store, true)` then gives one window with no entry (`assert!(windows[0].1.is_none())`)
   and an id other than the file launch's (`assert_ne!`).
4. Store D holds id 5 (`session` 7, older) and id 6 (`session` 9, newer); the test holds
   `store.claim(5)` (a live window in another process). `launch(&store, true)` restores `[6]` and
   `store.entries()` still holds id 5. (Passes at base too; it guards against pruning live windows.)

## Out of scope

- Single-instance forwarding (a second `fastedit .` joining the running process). Two processes on
  one root are still two sessions; after both quit, the next launch restores the one that saved
  last and prunes the other's windows.
- Removing stray lock files whose entries are already gone.
- `crates/fastedit/src/main.rs` needs no change (`launch_windows` already passes `folder`); leave it
  alone (T1257, T1258 edit it).
