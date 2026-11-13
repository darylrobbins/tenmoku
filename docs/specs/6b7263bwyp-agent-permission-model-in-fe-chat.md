---
task_uid: "01m7azsap5npean86b7263bwyp"
task: "T614"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test in crates/fe-chat/tests/permissions.rs: strictest wins"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-chat/tests/permissions.rs: allow patterns never beat deny"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-chat/tests/permissions.rs: shell strings, and first words with a `/`, match no allow pattern"
    required: true
    manual: false
  - id: "C4"
    text: "Test in crates/fe-chat/tests/permissions.rs: a request carries its resolved program path. Allow patterns and session memory match only bare names, and memory only while the resolved path is unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "Test in crates/fe-chat/tests/permissions.rs: session memory scoped to a chat"
    required: true
    manual: false
---

# Agent permission model in fe-chat: classes, allow/ask/deny levels, command patterns, shell-syntax detection and per-chat session memory (AI agents AG1, T1779) (Mac T1782)

# Agent permission model in fe-chat: classes, allow/ask/deny levels, command patterns, shell-syntax detection and per-chat session memory (AI agents AG1, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG1** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG1. Permission model.** Size **s**. Deps: none. Linux-OK.

- Files:
  - `crates/fe-chat/src/permissions.rs` (new: `Class`, `Level`, `Request`,
    `Policy::decide`, command patterns, shell-syntax detection, session
    memory);
  - `crates/fe-chat/src/lib.rs`.
- New test file: `crates/fe-chat/tests/permissions.rs`.
- Tests:
  - strictest wins;
  - allow patterns never beat deny;
  - shell strings, and first words with a `/`, match no allow pattern;
  - a request carries its resolved program path. Allow patterns and session
    memory match only bare names, and memory only while the resolved path
    is unchanged;
  - session memory scoped to a chat.

## Carried findings, verbatim from § 14

The fourth review of this record (round 4) found F6–F11. The approver
(Mac lead) took the record as proposed, with these carried to the cards
named below. Each is an acceptance criterion of that card in § 13. The
text of §§ 3, 6 and 12 above is not yet amended for them: where they
differ, the card's criterion wins.

F7's card is AG11, but § 14 puts its classification in AG1's `Policy`: a request whose tool is `SandboxNetworkAccess` is its own request, never class `other`. AG11's tests check it end to end.

- **F7 (high): `SandboxNetworkAccess` requests.**
  - **Gap.** Claude Code's live sandbox network requests reach
    `request_permission` with kind `Other`. § 7's fallback classes them
    `other`, so policy or a remembered class-wide answer could approve one,
    localhost included. `allow_once` then opens that host for the session.
  - **Fix.** A request whose tool is `SandboxNetworkAccess` is its own
    request, never class `other`:
    - always prompted, never decided by policy;
    - never remembered;
    - refused without a prompt for a loopback host (the § 3 loopback list).
  - **Card:** AG11 (`acp_host.rs`, permission requests), with the
    classification in AG1's `Policy`.

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

- `crates/fe-chat/l10n-allow.toml`: fe-chat's literal lint is `all = true`; command-pattern and class-name literals in `permissions.rs` may need listing or `verbatim!`.
