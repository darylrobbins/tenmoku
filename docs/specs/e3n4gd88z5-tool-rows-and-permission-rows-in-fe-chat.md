---
task_uid: "01m7azsfcgwb97wxe3n4gd88z5"
task: "T622"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "New test file `crates/fe-chat-view/tests/tool_rows.rs` covers tool rows with status, folded output, diff previews, plan checklists, the permission row with Allow once, Allow for this chat and Deny"
    required: true
    manual: false
---

# Tool rows and permission rows in fe-chat-view: status, folded output, diff previews, plan checklists, and Allow once / Allow for this chat / Deny (AI agents AG9, T1779) (Mac T1790)

# Tool rows and permission rows in fe-chat-view: status, folded output, diff previews, plan checklists, and Allow once / Allow for this chat / Deny (AI agents AG9, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG9** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG9. Tool rows and permission rows in fe-chat-view.** Size **m**. Deps:
AG3, T604. Linux-OK.

- Files:
  - `crates/fe-chat-view/src/tool_rows.rs` (new: tool rows with status,
    folded output, diff previews, plan checklists, the permission row with
    Allow once, Allow for this chat and Deny);
  - `crates/fe-chat-view/src/view.rs`;
  - `crates/fe-chat-view/src/lib.rs`;
  - `crates/fe-chat-view/l10n/en-US/fe-chat-view.ftl`.
- New test file: `crates/fe-chat-view/tests/tool_rows.rs`.

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

- `crates/fe-chat-view/l10n-allow.toml`: in case element ids or debug selectors trip the crate's `all = true` lint.

## Notes

- § 13 gives AG9 no separate Tests list; the criterion is the card's `tool_rows.rs` file description.

## Implementation notes

- `tool_rows.rs` holds plain row types (`ToolRow`, `ToolTitle`, `ToolStatus`,
  `ToolKind`, `ToolLocation`, `DiffPreview`, `PlanEntry`, `PermissionPrompt`,
  `PermissionChoices`, `PermissionAnswer`), so AG10's loop and AG12's ACP
  transcript both fill them and fe-chat-view depends on neither.
- The host sets them per turn: `ChatView::set_tool_row` (in place by id),
  `set_plan`, `ask_permission` / `withdraw_permission`. The view draws a
  turn's plan, then its rows (each prompt under its row; a prompt with no
  row after the rows), between the turn's thinking and its reply.
- Answers come back as `ToolEvent::PermissionAnswered { turn, tool,
  answer }`, and Review on a proposal row as `ToolEvent::ReviewProposal`:
  a second event type the view emits (`EventEmitter<ToolEvent>`), so
  fe-workbench's exhaustive `ChatViewEvent` match is untouched until AG10
  subscribes. Location links are the existing `ChatViewEvent::OpenPath`.
- Keys, with no new commands or bindings: Enter (`chat.send`) in the empty
  composer allows the first waiting prompt once; Escape (`chat.stop`) denies
  it. While a prompt waits, `chat.send` sends nothing, and Stop shows and
  closes every waiting prompt as `PermissionAnswer::Stopped`.
- `PermissionChoices::Once` leaves out Allow for This Chat, for requests
  whose answer is never remembered (F7's `SandboxNetworkAccess`, AG11);
  `Offered` shows an ACP request's own options when it has no `allow_once`.
- Diff previews are compact: each hunk's removed then added rows, no
  context, at most `PREVIEW_ROWS` (40) rows and a "… N more rows" line;
  they are diffed (fe-editor's `TextDiff`) when the row is set, not each
  frame.
- Carried T604 F2 is fixed: `turn_display`'s fallback attribution and the
  NotFound banner use `Conversation::asked_model(turn)`; tested in
  `tests/tool_rows.rs` before and after a picker change.
