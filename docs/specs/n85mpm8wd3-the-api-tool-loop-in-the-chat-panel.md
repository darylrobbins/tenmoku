---
task_uid: "01m7azsfywh44v45n85mpm8wd3"
task: "T623"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "the trust-withdrawal cases of § 12: Trust withdrawn during a permission wait, during a read and during a running command. Each ends the turn, kills or cancels the work, and sends nothing. The next request holds the `is_error` results, the user's text and the `tool_removal` message last"
    required: true
    manual: false
---

# The API tool loop in the chat panel: rounds, limits, Policy decisions and prompts, stop mid-tool, trust withdrawal, set_changeset at turn end and Continue (AI agents AG10, T1779) (Mac T1791)

# The API tool loop in the chat panel: rounds, limits, Policy decisions and prompts, stop mid-tool, trust withdrawal, set_changeset at turn end and Continue (AI agents AG10, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG10** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG10. The API tool loop in the chat panel.** Size **m**. Deps: AG4, AG7,
AG8, AG9, T604. Linux-OK. **[chat_panel.rs]**

- Files:
  - `crates/fe-workbench/src/chat_tool_loop.rs` (new: rounds, limits,
    decisions through `Policy`, prompts, stop mid-tool, trust generations
    and withdrawal, `set_changeset` at turn end, Continue);
  - `crates/fe-workbench/src/chat_panel.rs` (grants, the loop's hook into
    `view_event`, tools off in untrusted folders);
  - `crates/fe-workbench/src/lib.rs`;
  - `crates/fe-workbench/l10n/en-US/chat.ftl`.
- New test file: `crates/fe-workbench/src/chat_tool_loop_tests.rs`.
- Tests: the trust-withdrawal cases of § 12.

## § 12 workbench loop tests: trust withdrawn, verbatim

The workbench loop tests (AG10) add:

- **Trust withdrawn** during a permission wait, during a read and during a
  running command. Each ends the turn, kills or cancels the work, and sends
  nothing. The next request holds the `is_error` results, the user's text
  and the `tool_removal` message last.

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

## § 12 Workbench tests, verbatim

### Workbench tests

Workbench tests use GPUI's test platform, and Linux temp folders (`.txt` and
`.go` files, not `.rs`, so no language server starts).

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

- `crates/fe-workbench/l10n-allow.toml`: the `all = true` literal lint.

## Notes

- Tests go in `crates/fe-workbench/src/chat_tool_loop_tests.rs`.

## Implementation notes

- **Scope widened to `crates/fe-chat-view/src/view.rs`** (respec, rev 2): the
  view owns the conversation and its stream, so the loop needs a host hook.
  The view now records each turn's request settings (`begin_turn_with`),
  resends a reply dropped for invalid tool input, and, when a reply's calls
  wait, emits `ChatViewEvent::ToolCallsWaiting` instead of `TurnFinished`.
  It stays busy (`is_streaming`) until the host answers. The host's API is
  `set_tools`, `add_tool_result`, `next_round`, `end_round`,
  `withdraw_tools`, `continue_turn` and `can_continue`. Retry on a later
  round's failure resends that round's request (`retry_round`). A chat
  without tools behaves as before, and no existing view test changed.
- **`ToolLoop`** (`chat_tool_loop.rs`) is an entity made and dropped with
  each chat's view. It subscribes to the view's events, and the panel tells
  it the workbench and each root or trust change. `ChatPanel::root_changed`
  calls `folder_changed` before anything else, so a withdrawal ends the turn
  before the panel clears or departs anything.
- **Trust generations:** the loop counts withdrawals. Every task captures
  the count it was started under, and results, command ends, preparations
  and permission answers land only under it. On a withdrawal, reads and
  proposals in flight are dropped, and searches and listings are cancelled
  through their `AtomicBool`. A command is killed through its canceller,
  and its task runs on to write its audit line (`killed: trust_withdrawn`).
- **Continue** is a button under the transcript, in the panel
  (`workbench-chat-continue`), shown while the last turn ended at the round
  or time limit with its result message last. Continue counts the turn's
  limits again from then.
- **`process.spawn:*`** joins `CHAT_CAPABILITIES`, in the order `Grants`
  lists capabilities, so `chat_panel_tests`' grants check holds.
- Each call run is reported to the app's invocation observers as
  `chat.tool.<name>` by `Actor::Ai { agent: "fastedit.chat", session }`,
  `Source::Script`.
- **Test seams** (`#[cfg(test)]`): `CommandPath` fixes the `PATH` commands
  resolve on, and `Held` holds a tool's outcome before it lands, so a test
  can withdraw trust with a read in flight.
- **Tests** (C1), in `chat_tool_loop_tests.rs`:
  `trust_withdrawn_during_a_permission_wait`,
  `trust_withdrawn_during_a_read` and
  `trust_withdrawn_during_a_running_command`. Each fails if the panel
  doesn't withdraw the tools on the trust change.
- **Review round 1** (F1–F7). Each fix has a test in `chat_tool_loop_tests.rs`:
  - F1: a turn's hand-in (`agent_tools::end_turn`) is held in
    `ToolLoop::handing_in`. A turn that begins meanwhile begins its
    `ChatProposals` turn, and runs its edits, only once the hand-in is done
    (`a_hand_in_under_way_ends_only_its_own_turn`). A `TurnFinished` that
    arrives after the next turn began no longer ends that turn.
  - F2: `ChatPanel::show_chat` calls `ToolLoop::shut_down` before replacing
    the chat. It ends the turn as Stop does and detaches the hand-in, so the
    proposals still reach the AI Changes panel. Dropping the loop also sets
    the cancel flags of searches and listings
    (`a_chat_replaced_mid_turn_hands_in_and_kills_its_work`).
  - F3: edits run in the calls' order. The first edit not yet done runs once
    it's allowed, and the ones after it wait
    (`edits_run_in_the_calls_order_whatever_is_answered_first`).
  - F4: a command's audit line is written through the app, not the loop
    entity, so it's written even once the loop is gone
    (`a_root_change_mid_command_still_records_the_kill`).
  - F5: each call run has its own number (`Call::run`). Every completion
    lands only on the run it belongs to, so a stopped command's late end
    never lands on a later call with the same id
    (`a_stopped_commands_end_never_lands_on_a_later_call`).
  - F6: `ChatView::new` stops a turn still streaming in the conversation it's
    given (`a_copy_taken_mid_turn_opens_stopped`).
  - F7: a withdrawal discards the row output of every finished call but a
    proposal, whatever its status
    (`withdrawal_discards_every_finished_calls_output`).
