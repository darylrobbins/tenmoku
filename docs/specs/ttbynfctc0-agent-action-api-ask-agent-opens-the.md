---
task_uid: "01m7ac1ty1m8svfvttbynfctc0"
task: "T609"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::agent_actions (cargo nextest run -p fe-workbench, the chat's fake provider) pass: ask_agent_attaches_items_and_fills_the_composer, send_true_sends_when_a_key_is_set, with_no_key_the_draft_stays_and_a_toast_says_why, denied_files_are_refused_as_attachments, an_untrusted_folder_keeps_it_from_sending"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench chat passes (chat_panel.rs behaviour unchanged)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Agent action API: ask_agent opens the chat with context attached and a prompt, and sends only when the chat can (Mac T1588)

# Agent action API: ask the chat agent about something from anywhere

Spec: `docs/design/design-system.md` Problems ("Ask an agent, Explain"), Runs
("Ask an agent" on a test failure) and Terminal ("Send to an agent"). Each
needs the same thing: open the chat with some context attached and a prompt,
and optionally send it. Today only `chat.ask_about_selection` does that, and
only for the editor's selection (`Workbench::ask_about_selection` in
`crates/fe-workbench/src/chat_panel.rs`).

## Change

- `crates/fe-workbench/src/agent_actions.rs` (new): `Workbench::ask_agent(AgentAsk {
  prompt: String, context: Vec<fe_chat::ContextItem>, send: bool }, window,
  cx)`: shows the Chat panel (as `open_chat`), closes the key form, attaches
  the items through `ChatView::attach` (so the chat's size budget and its
  deny list apply as for any attachment), sets the composer text, and sends
  only when `send` is true **and** the chat has a key and the folder allows
  chat; otherwise it leaves the draft in the composer and focuses it, with a
  toast saying why. Reuses `chat_context`'s refusal rules for files.
- Helpers to build items: `text_item(label, text)` (a block of output or a
  message, capped at the chat's per-item limit) and `rows_item(path, rows)`.
- `crates/fe-workbench/src/chat_panel.rs`: the few private helpers it needs
  become `pub(crate)`; no behaviour change.

## Not in this card

Any button that calls it (the three cards after), agent lanes, a second agent.

Platform: **Linux-OK** (the chat's fake provider, as `chat_apply_tests.rs` uses).

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-chat-…` in
`crates/fe-workbench/l10n/en-US/chat.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.
