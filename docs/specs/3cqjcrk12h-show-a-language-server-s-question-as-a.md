---
task_uid: "01m55sbptn8jfy3y3cqjcrk12h"
task: "T171"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::lsp_message_request::a_server_question_shows_as_a_toast_and_answers_the_choice passes (new crates/fe-workbench/src/tests/lsp_message_request.rs): a Warning request with Reload and Ignore shows the server's name, the message and both buttons; clicking Reload answers {title: Reload} and the toast goes; two identical Info requests show two toasts that are still shown after LIFETIME plus 1 s; closing one answers null, toast.dismiss_all answers the other null"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (toast_tests.rs unchanged); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show a language server's question as a toast with a button per answer that stays until one is clicked or it's closed, and send the server the answer (Mac T920)

## Problem

Once the card this depends on lands, fe-lsp reports a server's `window/showMessageRequest` as
`LspEvent::ShowMessageRequest { server, name, typ, message, actions, reply }` and answers with what
`reply` is told. Dropped unanswered, it answers `null`. The workbench ignores the event, so the question
still never reaches the user, and the server is told "dismissed".

The toast layer can't ask the question yet. A `Toast` (`crates/fe-workbench/src/toast.rs:58-66`) has at most
one action, which runs a `CommandId`. Identical toasts merge into one with a count (`ToastQueue::push`,
`:162-189`), and info and warnings expire after `LIFETIME` (6 s). VS Code shows such a request as a
notification with a button per action, which stays until one is clicked or it is closed. Gap audit
2026-10-17.

## Change

1. `crates/fe-workbench/src/toast.rs`: a toast can carry **choices**: the titles of its buttons and a
   reply that receives the chosen index, or none.
   - It renders one button per title, in order, where the action button goes.
   - Clicking one answers its index and dismisses the toast.
   - Its close button, `toast.dismiss_all`, being dropped from a full waiting queue, or its layer going
     with its window all answer none.
   - It never expires, whatever its level, and never merges with another toast (each request is its
     own), even an identical one.
   - Existing toasts keep their behaviour.
2. `crates/fe-workbench/src/lsp.rs`: on `LspEvent::ShowMessageRequest`, `toast::show` a toast:
   - level from `typ` as `ShowMessage` maps it (Error → error, Warning → warning, others → info);
   - the server's name as its message, the request's message as its detail, the actions as choices;
   - the reply wired to `MessageReply::choose(index)`, or `dismiss()` for none.

   With no window to show it in, it is dismissed (answered `null`) and logged.
3. Tests:
   - New `crates/fe-workbench/src/tests/lsp_message_request.rs`, declared in
     `crates/fe-workbench/src/tests/mod.rs`, using the fake server harness of `tests/lsp.rs`.
     `a_server_question_shows_as_a_toast_and_answers_the_choice`: the fake sends a Warning
     `showMessageRequest` with actions `Reload` and `Ignore`, from a thread.
     - A toast shows the server's name, the message, and the two buttons.
     - Clicking `Reload` → the fake's request returns `{"title": "Reload"}`, and the toast is gone.
     - An Info request with the same text twice shows two toasts. Both are still shown after `LIFETIME`
       plus 1 s of the test clock.
     - Closing one → its request returns `null`.
     - `toast.dismiss_all` → the other returns `null`.
   - The toast tests in `crates/fe-workbench/src/toast_tests.rs` pass unchanged.

Linux-OK (headless GPUI, no goldens).
