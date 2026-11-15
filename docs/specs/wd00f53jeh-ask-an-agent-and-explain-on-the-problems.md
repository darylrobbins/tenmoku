---
task_uid: "01m7ffdvec004e6awd00f53jeh"
task: "T392"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::problems_ask_agent (cargo nextest run -p fe-workbench) pass: ask_prefills_the_chat_without_sending, explain_sends, both_are_disabled_with_a_reason_when_chat_is_unavailable, the_prompt_carries_message_code_source_and_the_surrounding_rows"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Ask an agent and Explain on the Problems detail pane (Mac T1589)

# Ask an agent and Explain from the Problems detail pane

Spec: `docs/design/design-system.md#problems` ("Apply fix, Ask an agent and
Explain").

## Change (`crates/fe-workbench/src/problems_detail.rs`)

- Two buttons in the detail pane's action row, secondary style:
  - **Ask an agent**: `ask_agent` (T1588) with a prompt asking how to fix
    the problem, the message and `code · source` in the prompt, a `rows_item`
    for the problem's row plus three either side, `send: false` (the user
    adds to the draft and sends).
  - **Explain**: the same context with a prompt asking what the problem means
    and why it happens, `send: true`.
- The same two commands, `problems.ask_agent` and `problems.explain`, in the
  palette when a problem is selected.
- Both are disabled with a tooltip reason in an untrusted folder or when chat
  is off (the same condition `ask_agent` reports).

## Not in this card

Fix application, multi-problem prompts, "Explain" for hover text.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-problems-…` in
`crates/fe-workbench/l10n/en-US/problems.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**
