---
task_uid: "01m789gyq8trr67sdwcyrbxt09"
task: "T331"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C2"
    text: "en-US byte-equal; cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# fe-workbench chat and AI strings (i18n W7) (Mac T1562)

# fe-workbench chat and AI strings (W7)

Spec: card **W7** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9, for the fe-workbench files the B3 surface
cards don't restyle (they move their own files' text). Sections 3–8 are the
design. en-US stays byte-equal to today's, so no golden and no behaviour test
changes except tests that construct removed literals.

Files: `chat_panel.rs`, `chat_apply.rs`, `chat_changeset.rs`, `chat_context.rs`, `chat_store_worker.rs`, `ai_changes_panel.rs`, `ai_changes_buffers.rs`, `inline_edit.rs`.

Add `crates/fe-workbench/l10n/en-US/chat.ftl` (keys `workbench-chat-…`) to
`resources!` in `crates/fe-workbench/src/l10n.rs`, and these files to
`l10n-allow.toml`'s `migrated` list. English plural helpers become plural
messages; fragments become whole messages.


Platform: **Linux-OK**.
