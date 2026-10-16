---
task_uid: "01m516qsevbcsvd2hskn9dbe9n"
task: "T100"
rev: 4
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "The host example's Rust, without blank and comment-only lines, is under 320 lines, using #[kiln::command], #[kiln::panel], #[kiln::status], #[kiln::item(kind = chat.conversation)] and #[kiln::model(persist)], with the kiln_ui::List thread panel and its arrow-key navigation kept"
    required: true
    manual: false
  - id: "C2"
    text: "Its command IDs (chat.*) and key contexts are unchanged, and a layout saved before the rewrite still restores"
    required: true
    manual: false
  - id: "C3"
    text: "tests/host.rs keeps every test function by name and what each asserts, as before this task (the thread selection tests included)"
    required: true
    manual: false
---

# Rewrite the Kiln Chat host example on kiln's macros, under 320 Rust lines, keeping its command IDs, contexts, saved kinds and List thread panel (Mac T688)

Filed on argon from Mac T688 (card ~/coordination/argon-cards/T688.json rev 1; note zj). Mac task uid 01m4vbjgenb9b2ha09ybnffgm3. Base: argon trunk 6a499d8 = mac/main b39ea29 synced (Mac T675 landed). Argon deps: T97 (Mac T687), T99 (Mac T676). Mac lead: T675's host tests now inject a fake config watcher (config.watch_with) instead of starting the real one; keep it that way, since the real watcher thread breaks gpui's test scheduler under load. Title the host commit "... (Mac T688)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2's exit; design record
`docs/decisions/kiln-app-layer.md`). Phase 2's exit criteria include the host example (Kiln Chat) dropping below
~250 Rust lines once the macros exist; phase 1 brought it under ~400 on `kiln::app` and `Workspace` alone.

## Change

Rewrite `kiln/crates/kiln/examples/host/` on the macros: commands as `#[kiln::command]` fns and an
`#[kiln::command(context = "chat")] impl Conversation` (keeping the IDs `chat.*` and the `chat` and
conversation contexts it has today, through `id`/`context`); the conversation list as `#[kiln::panel]`; the count
as `#[kiln::status]`; the conversations as `#[kiln::item(kind = "chat.conversation")]` (the saved kind kept); the
threads as `#[kiln::model(persist)]`. Keep `keymap.toml`/`menus.toml` only for what attributes can't say, or drop
them. `tests/host.rs` keeps every test function and what it asserts.

## Lead's note (argon, 2026-10-16, after the worker's block)

The scope is widened to kiln/crates/kiln/src/item.rs, kiln/crates/kiln-macros/src/item.rs and kiln/crates/kiln/tests/macro_item.rs. Add defaulted, additive hooks to `kiln::Item`: `split` (default None) and `may_close`/`close_revision` (or an equivalent close hook; the default keeps today's behaviour). Make `#[kiln::item]` forward them to PaneItem, and cover each forwarded hook in macro_item.rs. Change no existing Item method's signature or default behaviour. For the window layout's "threads" key: C2 (a layout saved before the rewrite still restores) and C3 (host.rs keeps what each test asserts) decide it. Keep the threads where host.rs asserts them (entry["threads"]), or read the old key through workspace.on_restore, so a pre-rewrite layout restores. Don't change host.rs's asserts.

## Mac lead's answer (2026-10-16, after the second block)

Option (a), with the List restored. C1 is now under 320 Rust lines, counted the same way (non-blank, non-comment,
examples/host/*.rs). Bring back the kiln_ui::List thread panel with arrow-key navigation. Restore tests/host.rs's
selection tests to what they asserted before this task: no clicking thread:N rows in place of the List, and no reading
ConversationList::selected in place of what they checked. C3 stands: every test is kept, with what it asserts, and no
assert is dropped. Add no more Item hooks for size alone. Keep the Item hooks already added (split, may_close,
close_revision), the fake config watcher (config.watch_with) and everything else that holds C2.

## Implementation notes

- `kiln::Item` gained defaulted `may_close`, `close_revision` and `split`, forwarded by `#[kiln::item]` and covered in `tests/macro_item.rs`.
- The host example is `main.rs`, `app.rs` (builder, six `#[kiln::command]` fns, workspace hook), `conversation.rs` (`#[kiln::item(kind = "chat.conversation")]`, `#[kiln::command(context = "conversation")] impl Conversation`) and `threads.rs` (`#[kiln::model(persist)]`, `#[kiln::status]`, `#[kiln::panel]`). `setup.rs` is folded into `app.rs`.
- `keymap.toml` and `menus.toml` stay: `tests/host.rs` counts the chat's six bindings and four menus from `Config` alone, and the Conversation menu must be second.
- The `threads` layout key and the legacy `dock` key are still read and written by the workspace hook, as `host.rs` asserts them.
- The panel is `kiln_ui::List` again (arrow keys, `list.confirm`/`selected` in host.rs as before the task); `KILN_CHAT_THEME` is dropped.
- Measured the way T675 was (non-blank, non-comment lines of `examples/host/*.rs`): 317. Review fixes: `close_revision` hashes the draft, the panel passes focus on to its list (host.rs presses an arrow key after focusing it), and the conversation scrolls to its newest message.
