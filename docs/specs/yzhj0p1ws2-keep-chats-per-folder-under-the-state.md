---
task_uid: "01m5f6t8j0krr0fqyzhj0p1ws2"
task: "T253"
rev: 6
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-chat test chats_save_atomically_and_list_newest_first (tests/store.rs): save writes \u003cid\u003e.json through a .tmp rename, 0600 in a 0700 directory on unix; load gives an equal Conversation whose thinking blocks and signatures re-serialize byte-identically; list is newest first; delete removes the file"
    required: true
    manual: false
  - id: "C2"
    text: "fe-chat test bad_chat_files_are_skipped_not_deleted (tests/store.rs): an unparsable file and a version 2 file are skipped by list and kept on disk, and load's error names them"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test chat_panel::chat_panel_tests::store::the_folders_last_chat_reopens: after a turn finishes the chat is under \u003cstate\u003e/chats/\u003clayout_key(root)\u003e/; a new panel for that root opens it with its model and effort; a window with no folder uses chats/no-folder"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test chat_panel::chat_panel_tests::store::history_lists_and_delete_removes: the History button lists the folder's chats newest first by title; picking one opens it; chat.delete, confirmed, removes its file and starts a new chat"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test chat_panel::chat_panel_tests::store::untrusted_folders_keep_chats_in_memory_only: in an untrusted root a finished turn creates no chats directory or file under the state dir, a new panel for that root starts empty, History lists the window's in-memory chats, and chat.delete drops one; after the root becomes trusted the next finished turn writes the chat 0600"
    required: true
    manual: false
---

# Keep chats per folder under the state directory, reopen the folder's last chat, list them in the panel, and delete one (Mac T973)

# Keep chats per folder in the state directory, reopen the last one, and list and delete earlier ones

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) keeps each
chat as `<state_dir>/chats/<layout_key(root)>/<chat id>.json` (`no-folder` for a window without
one), files 0600 and directories 0700, written atomically after each finished turn, reopened
per folder and listed newest first, kept until deleted (record §11), **except in a folder the user marked untrusted, where chats are
memory-only** (record decision 6). Today a chat lives only
in memory: closing the window loses it.

## Change

**Headless, `crates/fe-chat/src/store.rs`** (re-exported from `lib.rs`):

- `ChatStore::new(dir)` for one folder's directory. `save(&Conversation)` writes
  `{"version": 1, "conversation": …}` to `<id>.json.tmp`, then renames over `<id>.json`;
  creates the directory 0700 and the file 0600 on unix (`std::os::unix::fs` permissions;
  nothing extra elsewhere). `load(id)`, `list() -> Vec<ChatSummary { id, title, updated }>`
  newest first by the file's modified time, `delete(id)`.
- A file that doesn't parse, or has a newer `version`, is left on disk, skipped by `list`, and
  logged once with its path; `load` returns an error naming it.

**In the workbench** (`chat_panel.rs`):

- `chats_dir(state_dir, root: Option<&Path>) -> PathBuf`: `<state_dir>/chats/<layout_key>`
  with `kiln_core::layout::layout_key` of the canonical root, else `<state_dir>/chats/no-folder`
  (`fe_config::paths::state_dir()` in the app, a temp dir in tests). Here, so fe-chat needn't
  depend on kiln-core.
- **Untrusted folders** (record decision 6): when the workbench's folder trust
    (`crates/fe-workbench/src/trust.rs`) says the root is untrusted, the panel never creates the
    chats directory or writes a chat file, never reads one, and History lists only that window's
    in-memory chats; they are gone when the window closes. A root that is later trusted starts
    saving the shown chat at its next finished turn; one that is later untrusted stops saving
    (files already written stay until deleted). `chat.delete` of an in-memory chat just drops it.
    The decision is the panel's, not `ChatStore`'s: `ChatStore` has no trust logic.
- `ChatViewEvent::TurnFinished` saves the chat on the background executor (trusted roots only); a failed save is
  logged and shown once per chat as a toast.
- When the panel is first made, it loads the folder's newest chat if there is one (its model,
  effort and thinking setting kept, record §4), else starts a new one.
- The panel header gets a History button listing the folder's chats by title and time; picking
  one opens it in the view (a streaming reply is stopped first). `chat.delete` ("Chat: Delete
  Chat", context `chat`) asks to confirm, deletes the shown chat's file and starts a new chat.

Tests: `crates/fe-chat/tests/store.rs` (top-level fns, temp dirs); the panel side in
`crates/fe-workbench/src/chat_store_tests.rs`, declared from `chat_panel_tests.rs` as
`#[path = "chat_store_tests.rs"] mod store;`, with a temp state dir and
`fe_chat::ScriptedProvider`.

## Round 3 design (redesign; Mac y34 process: respec instead of an override)

Review rounds 1-2 found that window-local queues cannot make folder ownership, authorization at execution time, shutdown and list/pending-write consistency hold together (5 HIGH: F1 folder ownership across root switches; F2/F3 authorization checked when queued, not when the filesystem work runs; F5 shutdown replay racing the original write; F7 list vs pending writes). The worker's redesign is accepted, and the card is re-sized to `l`:

- **One app-owned, ordered store worker** (a single thread/task owning all chat file I/O for the app, not the window's panel): every save, load, list and delete is a request to it, executed strictly in order, so a list or load always observes every earlier accepted write (F7) and nothing is replayed (F5).
- **Per-folder authorization tokens** held by the worker: a request carries the folder it was made for and the token's generation; the worker re-checks trust/grants at the moment it executes the filesystem operation, and refuses with a typed error when the token is stale (F2/F3). Revoking trust or switching roots bumps the folder's generation.
- **Chats are tagged with their origin folder** when created; switching folders never carries a non-empty conversation into the new folder's store (F1): it is saved (if authorized) under its origin folder or kept memory-only, and the panel starts the new folder's own last chat.
- **Joined at quit**: the worker drains its queue and is joined on app shutdown exactly once; a drop of the panel does not re-execute inflight work (F5).
- Unique temp file per save (already done) stays; memory history is ordered newest-first by accepted order (F6).
- Tests for each finding in the existing test modules (root switch A->B with a non-empty conversation, trust revoked between queue and execution, quit while a write is inflight, list immediately after a save).
- The scope may grow with the new worker's file(s) in `crates/fe-chat/src` and `crates/fe-workbench/src` (use `rgt task respec --scope` listing the whole scope when you add one). Criteria are unchanged.

## Round 4 note (one HIGH left after 11 fixes; respec instead of an override)

Round 3 left one HIGH, F12: a retained trusted snapshot makes History reopen a stale conversation. Window A finishes chat C and selects New Chat; window B reopens C and adds a turn; A's History lists the updated disk entry, but `open_history_chat` unconditionally selects A's older memory snapshot, and sending another turn overwrites C's file and loses B's additions. Fix it as the finding says:

- Distinguish **retained fallback snapshots** (window-owned copies kept only for pending or rejected writes and for untrusted History) from **authoritative trusted History entries** (what is on disk, as listed by the ordered store worker). When the folder is trusted and the chat has a persisted entry, `open_history_chat` loads it **through the ordered worker** and uses that; a memory snapshot is used only while its write is pending or was rejected, or while the folder is untrusted.
- Add the two-window regression test: update a chat in window B, reopen it through window A's History, send another turn in A, and verify all turns (B's and A's) survive on disk.
- Nothing else changes; criteria are unchanged.

## Round 5: preserve a diverged continuation inside the accepted worker operation

Round 4's divergence fork made two HIGH findings: F14 (conflict recovery replaces the current view using the snapshot submitted to an earlier save, checking only the chat ID, so a newer turn or draft disappears) and F15 (persisting the conflicting continuation depends on a later window callback and a second worker request, so closing the window or quitting loses finished turns). Fix both by making the preservation part of the one worker operation and keeping the window out of it:

- A save is compare-and-swap on `base_revision` inside the ordered worker. When the disk revision differs from the snapshot's base, **the same worker operation durably writes the incoming conversation as a new chat file** (fresh id, title suffixed ' (copy)') and returns `Diverged { copy_id }`; the original chat's file is untouched. Nothing depends on a later window callback or a second request, and the queue drains this operation on close and on quit like any other save.
- The window **never replaces its current conversation or draft** because of a `Diverged` reply: it only adds the copy to its History list and shows a notice. Resolve against the window's *latest* revision, never the snapshot that was submitted. A delayed reply for a deleted chat creates nothing in the view (the copy file is simply another chat in History).
- F16/F17 (a delayed conflict reply resurrecting a confirmed delete; the replacement delete callback silently discarding results): with the change above there is no recovery callback to resurrect anything; make the delete callback surface a rejected delete instead of discarding it.
- Tests: two windows, equal-length different continuations: the diverged one becomes a copy file on disk and the original is intact; close the window (and quit) with the diverged save queued: the copy is still written; a diverged reply arriving after the user started another turn leaves that turn and the draft untouched; a rejected delete is reported.
- Criteria are unchanged; do not widen scope.
