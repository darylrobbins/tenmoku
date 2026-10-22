---
task_uid: "01m5f6t89x1n6bwfgg7knv1rsg"
task: "T252"
rev: 6
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-keymap test chat_keys_and_menu (tests/chat_bindings.rs): DEFAULT_MACOS binds ctrl-cmd-i and DEFAULT_LINUX ctrl-alt-i to chat.open in workbench; enter is chat.send and shift-enter a newline in chat_composer; escape is chat.stop in chat_composer; the View menu has Chat (chat.open) after Toggle Terminal; the keymap reports no conflict for them"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test chat_panel::chat_panel_tests::commands::ask_about_selection_attaches_it: in an untrusted root chat.ask_about_selection opens the panel, attaches the selection (the current row when it's empty) and focuses the composer"
    required: true
    manual: false
---

# Bind the chat's keys and menu item and add Chat: Ask About Selection (Mac T972)

# Bind the chat's commands to keys and menus and add Ask About Selection

## Problem

The AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) lists the
chat's commands (record §7, §13): open, new, send, stop, retry, ask about the selection. The earlier cards made `chat.open`, `chat.new`, `chat.send`,
`chat.stop` and `chat.retry` with no keys or menu items, and there's no Ask About Selection. Setting or clearing the API key (masked field, `chat.set_api_key`/`chat.clear_api_key`) is Mac T1021, not this card.

## Change

- **`chat.ask_about_selection`** ("Chat: Ask About Selection", context `editor`): opens the
  chat panel, attaches the active editor's selection (the whole current row if it's empty) as
  a chip even in an untrusted folder, and focuses the composer.
- **Keys** in `crates/fe-keymap/keymaps/fastedit.toml`:
  - `chat.open`: `keys = { macos = "ctrl-cmd-i", default = "ctrl-alt-i" }` (VS Code's
    chat keys), context `workbench`.
  - `chat.send`: `enter`, context `chat_composer`; `editor.newline` (or the editor's newline
    command id) on `shift-enter` in `chat_composer`, so shift-enter breaks the line.
  - `chat.stop`: `escape`, context `chat_composer` (does nothing unless a reply streams).
  - Check each against the existing bindings in its context with the keymap's own conflict
    report; change the key, not another binding, if one clashes, and say so.
- **Menus** in `crates/fe-keymap/menus/fastedit.toml`: View → "Chat" (`chat.open`) after
  "Toggle Terminal"; the palette lists every chat command by its title.

Tests: `crates/fe-keymap/tests/chat_bindings.rs` (top-level fns) for the keys and the menu;
`crates/fe-workbench/src/chat_commands_tests.rs`, declared from `chat_panel_tests.rs` as
`#[path = "chat_commands_tests.rs"] mod commands;`, on GPUI's test platform.

Order: after Mac T1021 is in the trunk (it edits chat_panel.rs and chat_panel_tests.rs too).

## Implementation notes

- The composer is an editor nested in `chat_composer`, and the deepest context wins, so the
  chat's `enter`, `shift-enter` and `escape` are bound in `chat_composer && editor`. The
  editor's own `enter` and `escape` now say `!chat_composer`, so the keymap's conflict report
  has nothing for the chat's four bindings (the key can't change: enter must send).
- `chat.send`/`chat.stop` live in fe_chat_view, so the workbench's catalog now includes that
  crate's recorded commands.
