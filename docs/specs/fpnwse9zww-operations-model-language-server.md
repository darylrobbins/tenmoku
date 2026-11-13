---
task_uid: "01m7ac1tedr3r8w2fpnwse9zww"
task: "T608"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::progress (cargo nextest run -p fe-lsp) pass: open_tokens_report_title_message_and_percentage, ended_tokens_disappear, the_64_token_cap_still_holds; the existing readiness tests pass unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in tests::operations (cargo nextest run -p fe-workbench) pass: lsp_progress_and_running_tasks_aggregate, the_primary_is_the_oldest_running, end_removes_an_operation"
    required: true
    manual: false
---

# Operations model: language-server progress, indexing and running tasks as one list (Mac T1585)

# Operations model: language-server progress, indexing and tasks as one list

Spec: `docs/design/design-system.md#notifications-and-agents` ("Operation in
progress: the status bar's left side, right after the Vim mode: spinner,
label, a 72 px bar, a count, and +N for other background tasks"). Today the
status shows a server's state as text only: `ServerStatus::Indexing { title }`
(`crates/fe-lsp/src/event.rs`, `readiness.rs`) has no message or percentage.

## Change (headless)

- `crates/fe-lsp`: keep what `$/progress` reports for each open work-done
  token (title, message, percentage) and expose it with the server's identity:
  `LspManager::progress()` plus a change event the workbench already
  subscribes to for status (`readiness.rs`, `event.rs`, `router.rs`). Existing
  readiness behaviour and `ServerStatus` are unchanged; a token cap of 64
  holds as today.
- `crates/fe-workbench/src/operations.rs` (new): `Operations`, one list of
  `Operation { id, label, fraction: Option<f32>, source }` aggregated from the
  language servers' progress, running task tabs (`task_run.rs`'s `TaskTabs`),
  and a `begin`/`end` API for other background work. Ordered by start; the
  primary one is the oldest still running.
- `crates/fe-workbench/src/lsp.rs` feeds the LSP part.

## Not in this card

The status bar UI (next card), cancelling an operation, LSP `window/workDoneProgress/create` UI changes.

Platform: **Linux-OK**.

## Implementation notes

- **The token cap.** fe-lsp's readiness tracker caps open tokens at 256
  (`readiness.rs` `TOKENS_MAX`), not 64, and that is unchanged. The 64 is a
  new cap on what's *reported*: each server lists at most its 64 oldest-begun
  open tokens (`readiness::WORK_MAX`); one ending lets the next oldest in.
  Readiness still counts every token up to 256.
- **fe-lsp.** Readiness keeps each open token's title (from its begin) and the
  message and percentage its latest begin or report said (a report leaving one
  out keeps it, per LSP); tokens matching `ready_ignores_progress` are listed
  too: the prefixes only exempt them from readiness. The router sends
  `LspEvent::Progress { server, name, work }` (the server's whole list, oldest
  begun first) when what it shows changes. The manager records it before the
  host hears it, for `LspManager::progress()`, and when a server goes
  `Restarting` or `Stopped` with work open, sends an empty `Progress` first.
  The tests' shared `assert_quiet` passes over `Progress` events, so the
  readiness tests read as before.
- **Running tasks come from T605's `Runs`**, which task tabs now feed
  (`task_run.rs` is T605's): `Operations` observes the workbench's `Runs` and
  lists each running run, with its progress, in place of reading `TaskTabs`.
- **Start order (review F1).** Each operation carries `started: Instant`,
  by the real clock, and the list is kept sorted by it: a run's
  `Run::started`, a token's `WorkProgress::started` (stamped by fe-lsp when
  it heard the begin, and kept while the 64 cap hides it; never the
  manager's injected clock), and the call to `Operations::begin`. So a run
  whose `RunsEvent` arrives after later work begun in the same update, or a
  token the cap revealed late, goes in at its start, not at the end. Ties
  keep the order heard.
- **`Operation`** also has `detail: Option<String>`, a progress's latest
  message, for the status bar. `Operations::update` sets a begun operation's
  fraction and detail. `begin`/`update`/`end` act only on operations begun
  with `begin`; the others end where they come from.
