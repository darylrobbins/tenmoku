---
task_uid: "01m6w0ancasahavqg0wm97yb1b"
task: "T1597"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in terminal_panel::terminal_send_to_agent_tests (cargo nextest run -p fe-workbench; the module is crates/fe-workbench/src/tests/terminal_send_to_agent.rs, declared with #[path] from terminal_panel.rs) pass: a_failed_block_offers_the_control_and_a_passed_one_does_not, the_command_and_last_200_rows_are_attached, an_agents_own_terminal_does_not_offer_it"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Send a failed terminal command's output to the agent

# Send failed command output to the agent from the terminal

Spec: `docs/design/design-system.md#terminal` ("Send to an agent on failures").

## Change

- On a command block that ended non-zero (the command-blocks cards), a
  **Send to an agent** control at the end of its prompt line:
  `ask_agent` (T1588) with `send: false`, a text item holding the command
  line and the block's output (the last 200 rows, then cut to the item limit),
  and the exit code in the prompt.
- Command `terminal.send_block_to_agent` for the block under the terminal
  cursor or selection; palette title "Terminal: Send Command to Agent".
- Not offered for an agent's own terminals.

## Not in this card

Sending a selection (the editor's Ask About Selection covers editors), automatic sends.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-terminal-…` in
`crates/fe-workbench/l10n/en-US/terminal.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**
