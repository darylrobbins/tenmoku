---
task_uid: "01m55sbppj8vbd6ypv5rf8cpc3"
task: "T170"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::router::message_request::show_message_request_answers_the_hosts_choice passes: the sink gets ShowMessageRequest with actions [Retry, Ignore]; choose(1) answers Ignore with its properties; dismiss() answers null; dropping the event answers null; choose(5) answers null; a second answer through a clone changes nothing"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp's existing tests::router showMessageRequest step is edited only to take the event from the sink and drop it, and still sees null; fe-workbench's tests pass unchanged (it ignores the event, which answers null)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no other existing test edited; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: hand a server's window/showMessageRequest to the host as an event with its actions, and answer with the action the host chooses, null if it's dismissed or ignored (Mac T919)

## Problem

LSP gap audit (2026-10-17, main c7b6a04a): "showMessageRequest. It is answered null with only a log line
(router.rs:153-156), so servers waiting on a user choice silently get 'dismissed'". Re-verified at main
40608138: `client_router` answers `window/showMessageRequest` with `Ok(None)` after an `info` log
(`crates/fe-lsp/src/router.rs:153-156`), and `crates/fe-lsp/src/tests/router.rs:121-126` pins that `null`.
The user never sees the question. Servers ask this way for choices like reloading a workspace or picking a
project, and VS Code shows them as a notification with a button per action.

`null` is a valid answer ("dismissed"), so this is a feature. This card hands the request to the host and
answers with what it chooses. The toast with buttons is the next card (6b).

## Change

1. `crates/fe-lsp/src/event.rs`:
   - New variant `LspEvent::ShowMessageRequest { server, name, typ: lsp_types::MessageType, message: String,
     actions: Vec<String>, reply: MessageReply }`. `actions` are the request's action titles, in order.
   - New `MessageReply`, Clone, with `PartialEq` by identity, like `ApplyReply`:
     - `choose(self, index: usize)` answers the action at `index`: the server gets that
       `MessageActionItem`, as it sent it, `properties` included. An index out of range answers `null`.
     - `dismiss(self)` answers `null`.
     - Once every clone has dropped unanswered, it answers `null`. A host that ignores the event, as the
       workbench does until 6b, keeps today's behaviour.
     - Only the first answer counts.
   - There is no timeout: VS Code has none, and an unanswered request blocks nothing in async-lsp's main
     loop. If the session ends first, the answer goes nowhere.
2. `crates/fe-lsp/src/router.rs`: the `ShowMessageRequest` handler keeps its log line, reports the event
   through the session's sink (as `window/showMessage` is reported), and returns a future of the answer.
3. `crates/fe-lsp/src/lib.rs`: export `MessageReply`.
4. Tests:
   - `crates/fe-lsp/src/tests/router.rs`: the existing showMessageRequest step (`:121-126`) now takes the
     event from the sink and drops it, then expects `null`. This is the only existing test edited. Declare
     `mod message_request;` in the same file.
   - New `crates/fe-lsp/src/tests/router/message_request.rs`,
     `show_message_request_answers_the_hosts_choice`. The fake sends a request with actions `Retry` and
     `Ignore` (with `properties` on `Ignore`) from a thread. The sink gets
     `ShowMessageRequest { actions: ["Retry", "Ignore"], .. }`:
     - `reply.choose(1)` → the fake gets `Ignore` with its properties;
     - a second request answered with `dismiss()` → `null`;
     - a third whose event is dropped → `null`;
     - `choose(5)` → `null`;
     - answering a clone after the first answer changes nothing.
