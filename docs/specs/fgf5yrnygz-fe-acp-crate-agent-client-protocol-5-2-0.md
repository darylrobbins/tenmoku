---
task_uid: "01m7azscy7y7m371fgf5yrnygz"
task: "T618"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "New test file: `crates/fe-acp/tests/client.rs`, with an in-process fake over `Channel::duplex()`, covering connection, initialize, new, load, prompt, cancel, set config option, set mode, the handler that hands out permission requests, and `AcpEvent` from `SessionUpdate`"
    required: true
    manual: false
  - id: "C2"
    text: "The card runs `crates/fe-chat/tests/request.rs`, `crates/fe-chat-anthropic/tests/provider.rs` and fe-config's settings schema tests unedited, and fixes any order-dependence it finds within its scope"
    required: true
    manual: false
  - id: "C3"
    text: "`cargo deny check licenses` passes"
    required: true
    manual: false
---

# fe-acp crate: agent-client-protocol 5.2.0 dependency and the ACP client core (initialize, new, load, prompt, cancel, config options, modes, permission handler, AcpEvent) (AI agents AG5, T1779) (Mac T1786)

# fe-acp crate: agent-client-protocol 5.2.0 dependency and the ACP client core (initialize, new, load, prompt, cancel, config options, modes, permission handler, AcpEvent) (AI agents AG5, T1779)

Source: `docs/decisions/50t8wt9we2-ai-agents.md` § 13, card **AG5** — T1779's decision record (AI agents: Claude Agent over ACP, a tool loop for the API chat, permissions and review), approved by the lead with round-4 findings F6–F11 carried into card criteria (§ 14). Read the record's sections the card cites before starting.

Linux-OK: the card can be built and tested on Linux (§ 13; no card in the plan is Mac-only).

## The card, verbatim from § 13

**AG5. fe-acp: crate, dependency and client core.** Size **m**. Deps: none.
Linux-OK. **[Cargo deps]**

- Files:
  - `Cargo.toml` (workspace dependencies: `agent-client-protocol = "=5.2.0"`
    with default features off, and `fe-acp = { path = "crates/fe-acp" }`);
  - `Cargo.lock`;
  - `deny.toml` (only if `cargo deny check licenses` fails; § 2's research
    found every new crate MIT or Apache-2.0);
  - `crates/fe-acp/Cargo.toml`;
  - `crates/fe-acp/src/lib.rs`;
  - `crates/fe-acp/src/client.rs` (new: connection, initialize, new, load,
    prompt, cancel, set config option, set mode, and the handler that hands
    out permission requests);
  - `crates/fe-acp/src/events.rs` (new: `AcpEvent` from `SessionUpdate`).
- New test file: `crates/fe-acp/tests/client.rs`, with an in-process fake
  over `Channel::duplex()`.
- The 5.2.0 crate turns on serde_json's `preserve_order` feature
  (its `Cargo.toml:382-387`). Features unify, so this changes JSON key order
  workspace-wide. The card runs `crates/fe-chat/tests/request.rs`,
  `crates/fe-chat-anthropic/tests/provider.rs` and fe-config's settings
  schema tests unedited, and fixes any order-dependence it finds within its
  scope (chat record § 4 already requires order-independent output).

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

## Notes

- The test-coverage list in the first criterion is the card's `client.rs` file description; § 13 gives AG5 no separate Tests list.
- The licence criterion is `cargo deny check licenses`, not bare `cargo deny check`, which fails on advisories the gate skips.

## Implementation notes

- `fe_acp::connect(transport)` returns the `AcpClient` handle, an unbounded
  stream of `Incoming` (updates as `AcpEvent`s, and `PermissionRequest`s, in
  the agent's order) and the connection future for the caller to drive. The
  future ends on transport close, `AcpClient::shutdown`, or when every handle
  is dropped; the stream ends after it.
- Neither handler awaits anything (the crate's ordering rule). The permission
  handler holds the responder and hands the request out. A request is answered
  exactly once: `select(option_id)`, `cancel()`, `AcpClient::cancel` for its
  session, or `Cancelled` on drop. An option id the request didn't offer is
  never sent; the request is answered `Cancelled`.
- `AcpClient::cancel` sends `session/cancel`, then answers the session's
  pending requests `Cancelled`. Until every turn running at the cancel has
  ended (its prompt's response, error or drop), later requests for the
  session are answered `Cancelled` at once and never reach the stream, even
  while a prompt queued after the cancel runs (review F2).
- Every answer and the `session/cancel` notification are queued under one
  lock (queueing only pushes onto the connection's FIFO outgoing channel), so
  on the wire an allow is before the cancel or never sent, and an automatic
  `Cancelled` is after it (review F1; tested with a wire tap and a two-thread
  race).
- `AcpEvent` maps the updates § 5 renders, plus `UserMessage` for the replay
  on `session/load`. The ignored ones (commands, usage, notices, compaction, a
  title-less session info, unknown variants) are dropped with a debug log. An
  update whose `sessionUpdate` the schema doesn't know fails to parse in the
  crate and is dropped; later updates still arrive (tested).
- `preserve_order`: `crates/fe-chat/tests/request.rs`,
  `crates/fe-chat-anthropic/tests/provider.rs` and all of fe-config's tests
  pass unedited when built together with fe-acp (`cargo tree -e features`
  shows the feature unified there). No order-dependence was found, so nothing
  outside fe-acp changed. `deny.toml` is unchanged: `cargo deny check
  licenses bans sources` passes.
