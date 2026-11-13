---
task_uid: "01m7azsc9v7cx07xvry6yh99za"
task: "T617"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-chat-anthropic/tests/tool_wire.rs: request JSON fixtures"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-chat-anthropic/tests/tool_wire.rs: the beta header joined with the existing ones"
    required: true
    manual: false
---

# Tool wire in fe-chat-anthropic: eager_input_streaming, tool_result blocks, output_config.task_budget and the task-budget and mid-conversation-tool-changes betas (AI agents AG4, T1779) (Mac T1785)

# Tool wire in fe-chat-anthropic: eager_input_streaming, tool_result blocks, output_config.task_budget and the task-budget and mid-conversation-tool-changes betas (AI agents AG4, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG4** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG4. Tool wire in fe-chat-anthropic.** Size **s**. Deps: AG3, T604.
Linux-OK.

- Files:
  - `crates/fe-chat-anthropic/src/wire.rs` (`eager_input_streaming`,
    `tool_result` blocks, `output_config.task_budget`);
  - `crates/fe-chat-anthropic/src/provider.rs` (the
    `task-budgets-2026-03-13` and `mid-conversation-tool-changes-2026-07-01`
    betas in `anthropic-beta`).
- New test file: `crates/fe-chat-anthropic/tests/tool_wire.rs`.
- Tests: request JSON fixtures, and the beta header joined with the existing
  ones.

## Plan-wide rules, verbatim from § 13

Cards are size **s** or **m**. Scopes are exact files, and tests are in new
files. "Linux-OK" means the card can be built and tested on Linux. Marks:

- **[chat_panel.rs]** touches `crates/fe-workbench/src/chat_panel.rs`;
- **[settings.rs]** touches `crates/fe-config/src/settings.rs`;
- **[Cargo deps]** adds Cargo dependencies, and needs the `deny.toml`
  licence check.

Cards that touch T604's files depend on **T604** (the picker card).

New fe-workbench modules are declared from `crates/fe-workbench/src/lib.rs`,
not from `chat_panel.rs`. Each card's tests are declared from its own new
module.

Order: wave 1 in parallel; then AG7, AG8 and AG9; then AG10. AG11 waits for
its lanes cards, then AG12 and AG13 follow.

- **Mac-only cards:** none. Every card is Linux-OK.
- **Mac check:** the sandbox's macOS path (Seatbelt) is checked in the
  manual smoke run on a Mac.

T604 in the record is Mac T1780 (the model and effort picker), already on main.

## Scope beyond the card's file list

- `crates/fe-chat-anthropic/tests/fixtures/tool_wire/**`: the request JSON fixtures, if kept as files.
- `crates/fe-chat-anthropic/l10n-allow.toml`: the beta header names are wire literals under the crate's `all = true` lint.
