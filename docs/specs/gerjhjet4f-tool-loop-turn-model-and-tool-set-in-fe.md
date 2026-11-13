---
task_uid: "01m7azsbsnv43ngqgerjhjet4f"
task: "T616"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test in crates/fe-chat/tests/tool_loop.rs: byte-identical prefixes across rounds"
    required: true
    manual: false
  - id: "C2"
    text: "Test in crates/fe-chat/tests/tool_loop.rs: one result message per round"
    required: true
    manual: false
  - id: "C3"
    text: "Test in crates/fe-chat/tests/tool_loop.rs: stop and round limit leave valid history"
    required: true
    manual: false
  - id: "C4"
    text: "Test in crates/fe-chat/tests/tool_loop.rs: `max_tokens` and `refusal` run no tools"
    required: true
    manual: false
  - id: "C5"
    text: "Test in crates/fe-chat/tests/tool_loop.rs: a later round's refusal keeps every earlier round, and a failed round's Retry is the identical request"
    required: true
    manual: false
  - id: "C6"
    text: "Test in crates/fe-chat/tests/tool_loop.rs: the next user text goes in as a block after the last results"
    required: true
    manual: false
  - id: "C7"
    text: "Test in crates/fe-chat/tests/tool_loop.rs: the `tool_removal` message, last, after the user's text"
    required: true
    manual: false
  - id: "C8"
    text: "Carried finalisation keeps tool calls and results matched one to one"
    required: true
    manual: false
  - id: "C9"
    text: "Carried an uncommitted streaming reply keeps finished text only, no `tool_use` and no results"
    required: true
    manual: false
  - id: "C10"
    text: "Carried a reply with invalid tool input is retried once as the identical request, and never enters history. Known limit (follow-up Mac T1815): at `max_tokens`, a finished last call whose input is a JSON string rather than an object is taken as cut short (finished text only, turn done, no resend), because the provider keeps input that isn't JSON as a raw string."
    required: true
    manual: false
  - id: "C11"
    text: "Carried prior committed rounds are unchanged"
    required: true
    manual: false
  - id: "C12"
    text: "Carried Exact-request tests: Stop, trust withdrawal, and `max_tokens`, each during tool-input streaming"
    required: true
    manual: false
---

# Tool-loop turn model and tool set in fe-chat: rounds and tool results in append-only history, the five tool schemas, frozen tools and task budget, multi-round fake scripts (AI agents AG3, T1779) (Mac T1784)

# Tool-loop turn model and tool set in fe-chat: rounds and tool results in append-only history, the five tool schemas, frozen tools and task budget, multi-round fake scripts (AI agents AG3, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG3** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG3. Tool-loop turn model and tool set.** Size **m**. Deps: T604.
Linux-OK.

- Files:
  - `crates/fe-chat/src/conversation.rs` (`Round`, `ToolResult`, history per
    § 6, appending to a pending result message, old files loading);
  - `crates/fe-chat/src/request.rs` (frozen tools, task budget);
  - `crates/fe-chat/src/provider.rs` (`ToolSpec::eager_input_streaming`);
  - `crates/fe-chat/src/tools.rs` (new: the five schemas and descriptions,
    typed inputs, validation);
  - `crates/fe-chat/src/fake.rs` (multi-round scripts);
  - `crates/fe-chat/src/lib.rs`.
- New test file: `crates/fe-chat/tests/tool_loop.rs`.
- Tests:
  - byte-identical prefixes across rounds;
  - one result message per round;
  - stop and round limit leave valid history;
  - `max_tokens` and `refusal` run no tools;
  - a later round's refusal keeps every earlier round, and a failed round's
    Retry is the identical request;
  - the next user text goes in as a block after the last results;
  - invalid input becomes an `INVALID_JSON` error result;
  - the `tool_removal` message, last, after the user's text.
- **Carried acceptance criterion F9** (§ 14):
  - finalisation keeps tool calls and results matched one to one;
  - an uncommitted streaming reply keeps finished text only, no `tool_use`
    and no results;
  - a reply with invalid tool input is retried once as the identical
    request, and never enters history;
  - prior committed rounds are unchanged.

  Exact-request tests: Stop, trust withdrawal, and `max_tokens`, each during
  tool-input streaming.

## Carried findings, verbatim from § 14

The fourth review of this record (round 4) found F6–F11. The approver
(Mac lead) took the record as proposed, with these carried to the cards
named below. Each is an acceptance criterion of that card in § 13. The
text of §§ 3, 6 and 12 above is not yet amended for them: where they
differ, the card's criterion wins.

- **F9 (medium): tool-loop recovery for unfinished tool blocks.**
  - **Gap.** § 6's rules don't keep tool calls and results matched in two
    cases, so the next request can fail:
    - Stop while streaming keeps text only, yet another rule answers every
      call;
    - trust withdrawal doesn't say which streamed `tool_use` blocks survive.

    Separately, invalid JSON is kept as a string-valued `tool_use.input`,
    which can't be echoed as the object the API requires.
  - **Fix.** Separate finalisation rules for each case. Prior committed
    rounds stay unchanged in every case:
    - **an uncommitted streaming reply:** drop every `tool_use` and keep only
      finished text, with no results;
    - **a committed round:** exactly one result per retained call.

    A reply with invalid tool input is not committed. It is retried once as
    the identical request, then the turn fails without entering history.
  - **Card:** AG3 (`conversation.rs`), with exact-request tests for Stop,
    trust withdrawal, and `max_tokens` during tool-input streaming.

## § 12 The scripted provider for tool loops, verbatim

### The scripted provider for tool loops

`fe_chat::ScriptedProvider` (`crates/fe-chat/src/fake.rs:25`) already plays
one reply per request and records the requests. Tests script multi-round
turns with `tool_use` blocks and assert:

- each request's `tools` bytes equal the first's;
- request N+1 extends request N's messages byte for byte;
- all results are in one user message;
- `is_error` results appear for denied, invalid, stopped and limited calls;
- stop mid-tool leaves a valid history;
- **refusal in a later round:** tools complete in round 1, round 2 is
  refused, and the next request keeps round 1's reply and results byte for
  byte, with the new text as a block after them;
- a failed later round's Retry resends the identical request.

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

- `crates/fe-chat/l10n-allow.toml`: the five tool descriptions in `tools.rs` are model-facing English literals under fe-chat's `all = true` lint.
- `crates/fe-chat-anthropic/tests/provider.rs`: it builds a `ToolSpec { .. }` literal (`:374`), which breaks when `ToolSpec` gains `eager_input_streaming`. Only that literal may change.

## Notes

- Where F9 and the invalid-input bullet above differ (F9: a reply with invalid tool input is retried once and never enters history; the § 13 bullet: invalid input becomes an `INVALID_JSON` error result), § 14 says the carried criterion wins.

## Implementation notes

- **Shape.** `Turn` keeps `reply` (the last round's reply, so fe-chat-view and
  existing tests are unchanged) and gains `rounds: Vec<Round>` (committed
  rounds, each `Round { reply, results }` with one result per call),
  `results` (the pending result message) and `removes_tools`. Old files load
  with no committed rounds and their one reply in `reply` (`#[serde(default)]`).
  `Conversation` gains `tools`, `task_budget` and `tools_withdrawn`, frozen by
  `set_tools` before the first send.
- **Loop API** (for AG10): `apply` → `pending_calls` / `add_result` /
  `next_round` → `round_request`, turns begun with `begin_turn_with`; `stop`, `withdraw_tools`,
  `end_round(NotRun::RoundLimit | TimeLimit)`, `resume` (Continue),
  `retry_round` (Retry), `needs_resend`. `round_request` rebuilds the last
  turn's request from the conversation, so a resend, a Retry and round 1 are
  byte-identical to the request before.
- **F9 over the § 13 INVALID_JSON bullet** (as the task notes say), after
  review round 1:
  - a reply that ended inside a call's input (a `tool_use` block that never
    finished, or `max_tokens` with the last block a call cut short, its
    input the raw non-JSON string the provider keeps) isn't committed: finished text only, no thinking, no `tool_use`, no results;
  - else any kept call whose input isn't JSON, isn't an object (derived
    structs also take arrays, so objects are required, edits included) or
    doesn't validate makes the reply uncommitted whatever the stop reason:
    resent once, then the turn fails (`ErrorKind::Stream`);
  - a `max_tokens` reply with only valid calls is kept and its calls
    answered "Cut off at the token limit".
- **Identical resends:** a tool-loop turn begins with
  `Conversation::begin_turn_with(content, attachments, &settings)`, which
  records the turn's `max_tokens` and effort-message choice
  (`Turn::sent_with`); every `round_request` for the turn uses them, so a
  resend or Retry of any round, the first one built by `build_request`
  included, is the same request whatever the settings become. (A turn begun
  with plain `begin_turn`, as fe-chat-view's tool-less chat does, records
  them at its first `round_request`.) Complete JSON that fails validation
  is invalid under `max_tokens` too, and resent.
  `retry_round` refuses a round sent before a withdrawal unless a sent turn
  already carries the `tool_removal` message.
- **`tool_removal`** is `Role::System { effort: <in effect> }` with one
  `tool_removal` block per tool (`ApiMessage::tool_removal`), since `Role`
  can't gain a variant without editing `wire.rs`. `effort_change()` is `None`
  for it. **AG4 must**: leave `output_config` off a system message with
  content, add the `mid-conversation-tool-changes-2026-07-01` beta when
  `removed_tools()` is non-empty, and let `request_body` accept a request
  whose last message is that system message (it rejects any non-user last
  message today). `task_budget` is on `ChatRequest` for AG4's
  `output_config.task_budget`.
- **Effort messages and joining:** when the next turn's effort changes, the
  effort-only message goes after the results message and the text is a new
  user message; otherwise the text joins the results message.
- `crates/fe-chat/src/inline.rs` was added to scope for one line:
  `task_budget: None` in its `ChatRequest` literal.
