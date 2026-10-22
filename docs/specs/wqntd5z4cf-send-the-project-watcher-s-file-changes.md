---
task_uid: "01m55v3d6rskwhfxwqntd5z4cf"
task: "T183"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::lsp::watched_files::watcher_batches_reach_registered_servers passes: a batch of a new src/new.rs, an edited Cargo.toml, a deleted src/old.rs, README.md and .git/objects/ab/cd sends one didChangeWatchedFiles with Created (or Changed where the filesystem has no creation time), Changed and Deleted for the first three only; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test tests::lsp::watched_files::a_rescan_sends_nothing passes; stats run off the UI thread; workbench.rs changes by the one FsChanged-arm line"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Send the project watcher's file changes to language servers that registered for them, as created, changed or deleted, skipping VS Code's watcher excludes (Mac T943)

Filed on argon from Mac T943 (card ~/coordination/argon-cards/zzr/T943.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txxxjvjfngbbf13k6gtb5. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T182 (Mac T942). Title the host commit "... (Mac T943)".

## Problem

LSP completeness track, item 5 (user-approved 2026-10-17). Once the card this depends on lands, fe-lsp accepts
servers' `workspace/didChangeWatchedFiles` registrations and sends matching changes given to
`LspManager::files_changed(Vec<(PathBuf, FileChangeType)>)`. Nothing gives it any. The workbench already gets
the project watcher's batches: `PanelEvent::FsChanged(changes)` in `Workbench::follow_root`
(`crates/fe-workbench/src/workbench.rs:667-675`), handed to quick open, navigation, git and disk sync. Each is
`fe_project::FsChange::Path(path)` ("may have been created, changed or removed") or `Rescan`
(`crates/fe-project/src/watcher.rs:29-35`): no kind. VS Code sends Created, Changed and Deleted, skipping its
`files.watcherExclude` defaults (`**/.git/objects/**`, `**/.git/subtree-cache/**`, `**/node_modules/*/**`,
`**/.hg/store/**`).

Watcher facts that bound this (memory `fe-project-fs-facts`, verified 2026-10-05/10): FSEvents reports a
directory's own path only for changes to the directory itself; it can lose events silently under a write flood,
with no flag; a new stream may report writes made just before it started.

## Change

1. New `crates/fe-workbench/src/lsp_watched_files.rs`, declared from `crates/fe-workbench/src/lsp.rs` as
   `#[path = "lsp_watched_files.rs"] mod watched_files;`:
   - `Workbench::lsp_fs_changed(&mut self, changes: &[FsChange], cx)`: with a manager, takes each `Path`
     outside the excluded globs above, resolves it as `lsp::resolve` resolves paths (the manager's root is
     canonical, `lsp.rs:812`), and off the UI thread stats it: missing → `Deleted`; present and created (its
     `Metadata::created()`) at or after the previous batch's arrival (the manager's start for the first) →
     `Created`; present otherwise, or where the platform gives no creation time → `Changed`. Then
     `files_changed` with the batch, in order, from the main thread. A batch with only excluded paths sends
     nothing.
   - `Rescan` is logged at debug and sends nothing (a known limit, in the module docs: a server that missed
     a change sees it at its next watched event or restart; VS Code has the same gap when its watcher
     overflows).
   - Module docs: the kind rule and its limit (a file replaced by an atomic save, a new inode, reads as
     `Created`; servers generally re-read a file on either).
2. `crates/fe-workbench/src/workbench.rs` `follow_root`: one line in the `FsChanged` arm calling it.
3. Tests: new `crates/fe-workbench/src/tests/lsp_watched_files.rs`, declared at the end of
   `crates/fe-workbench/src/tests/lsp.rs` as `#[path = "lsp_watched_files.rs"] mod watched_files;`, with the
   fake of `tests/lsp.rs` registering `**/*.rs` and `**/Cargo.toml`, and batches fed through
   `ProjectPanel::apply_fs_changes` (`crates/fe-workbench/src/project_panel.rs:294`) rather than the real
   watcher, so no timing on FSEvents:
   - `watcher_batches_reach_registered_servers`: after a first batch, writing a new `src/new.rs`, editing
     `Cargo.toml` (which existed before that batch) and deleting `src/old.rs`, a batch of the three paths plus
     `README.md` and `.git/objects/ab/cd` makes the fake receive one `workspace/didChangeWatchedFiles` with
     `src/new.rs` Created (Changed where the test's filesystem gives no creation time, checked with
     `fs::metadata(..).created()`), `Cargo.toml` Changed and `src/old.rs` Deleted, nothing else.
   - `a_rescan_sends_nothing`: a batch of only `Rescan` sends no notification.

Linux-OK.
