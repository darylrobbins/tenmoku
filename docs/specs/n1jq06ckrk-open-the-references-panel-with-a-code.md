---
task_uid: "01m55v1fm1s3kttrn1jq06ckrk"
task: "T187"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test lsp::trust_tests::lens_references::reference_lenses_open_the_references_panel passes: clicking a 2 references lens (rust-analyzer.showReferences, then editor.action.showReferences) shows References focused with both files' references at their rows and previews, and sends no workspace/executeCommand; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "LspNavigation::decode_references uses references' decoder; fe-lsp's navigation tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
  - id: "C4"
    text: "Regression tests fail at the base and pass: an edit landing immediately after a lens request, an edit between the request and the resolve, and an edit to another open document each leave the references previews and origin-first order from the text sent with the request, and a lens whose context is no longer current is dropped, not shown or resolved"
    required: true
    manual: false
---

# Open the References panel with a code lens's locations for editor.action.showReferences and rust-analyzer.showReferences, as VS Code's references peek does (Mac T949)

## Problem

LSP completeness track, item 7 (user-approved 2026-10-17: "mapping client commands like
editor.action.showReferences to the references panel"). Reference and implementation lenses ("5 references",
"3 implementations") carry a client command, not a server one: `editor.action.showReferences` (VS Code's
built-in, used by typescript-language-server and gopls) or rust-analyzer's `rust-analyzer.showReferences`, both
with arguments `[uri, position, locations]`. VS Code opens its references peek on them. Once the cards this
depends on land, a lens's command goes to `lsp.run_lens`, which knows no client command, so clicking "5
references" shows "this command isn't supported".

fastedit's References panel (`crates/fe-workbench/src/references_panel.rs`) only lists the results of its own
search: `Workbench::find_references` (`:555`) runs a `NavRequest` through `ReferencesPanel::run` (`:164`),
appending `fe_nav::FileReferences` batches (`append`, `:216`). fe-lsp's `LspNavigation::references`
(`crates/fe-lsp/src/navigation.rs:190-300`) decodes a server's `Location`s into `FileReferences` with
previews, through its private `Decoder` (`:620-715`: open documents' sent text, else the file read from disk).

## Change

1. `crates/fe-lsp/src/navigation.rs`: `LspNavigation::decode_references(&self, root: &Path, locations:
   Vec<lsp_types::Location>, cancel: &AtomicBool) -> Result<Vec<FileReferences>, NavError>`: the locations
   decoded and grouped exactly as `references` decodes and groups its reply (same `Decoder`, previews, file
   order; no definition marks), for an answer the host already has.
2. `crates/fe-workbench/src/references_panel.rs`: `ReferencesPanel::show_locations(&mut self, word: String,
   files: Vec<FileReferences>, cx)`: replaces the results as a finished search would, with that word in the
   header. `Workbench::show_references(word, locations)`: shows the dock with References, focused, as
   `find_references` does, decodes off the UI thread with `decode_references`, then `show_locations`.
3. `crates/fe-workbench/src/lsp_lenses.rs` `lsp.run_lens`: `editor.action.showReferences` and
   `rust-analyzer.showReferences` take their third argument as `Vec<Location>` and call `show_references`,
   the word being the one at the lens's range start (its symbol); arguments that don't parse show the
   unsupported toast as other commands do. Nothing is sent to the server.
4. Tests:
   - New `crates/fe-workbench/src/lsp_lens_references_tests.rs`, declared at the end of
     `crates/fe-workbench/src/lsp_trust_tests.rs` as `#[path = "lsp_lens_references_tests.rs"] mod
     lens_references;`: `reference_lenses_open_the_references_panel`: the fake's lens `2 references` with
     `rust-analyzer.showReferences` and two locations (one in the open file, one in an unopened file with an
     emoji before it); clicking it shows the References panel focused, listing both files with the
     references at their rows and previews, and the fake receives no `workspace/executeCommand`; the same with
     `editor.action.showReferences`.
   - fe-lsp: `crates/fe-lsp/src/tests/navigation.rs` gains no test; the decoding is covered through the
     workbench test (it shares `references`' decoder, whose tests pass unchanged).

Order: T851 holds `navigation.rs`; T247 and T696 hold `references_panel.rs`. Linux-OK.

## Revision 3 (Mac lead zz78 decision): fix F1, don't defer it

Codex rejected the deferral twice (F1, high, `crates/fe-workbench/src/lsp_lenses.rs` near 434 and 473: lenses are published before admission is checked, so a stale snapshot can be shown or resolved). Fix it, in scope:

- Carry the original request's admitted context (the document version and the sent-text snapshot of it and of the other open documents, `Admitted`) with the lens answer, through `codeLens/resolve` and through display (the click that opens References), in `crates/fe-lsp/src/code_lens.rs` and `crates/fe-lsp/src/manager.rs` (both added to the scope) and `lsp_lenses.rs`.
- `decode_references` decodes against that snapshot, with the producing lens's server encoding and originating document, so previews and origin-first order don't change when the buffer or another open document is edited later.
- Drop any lens whose context is no longer current (its document's version moved on) instead of showing or resolving it; a re-ask replaces it.
- No new retention queue or cache beyond keeping the request's `Admitted` with its answer. Remove the "limit" notes Revision 2 asked for (spec notes, the comment at `show_references`).
- Regression tests (fail at the base): an edit landing immediately after the request (before admission), and an edit between the request and the resolve; plus one for an edit to another open document, each checking previews, ordering and that a stale lens is dropped.

T204 (the former follow-up) is abandoned and folded in here.

## Revision 4 (review round 3, F2): test the mapped navigation, no deferral

F1 is fixed. The one open finding (F2, medium, `crates/fe-workbench/src/lsp_lens_references_tests.rs` near 292) is that the
retained-origin and edited-other-document tests stop after checking displayed rows. Extend both, in scope, to open the
corresponding References row after the edit and assert that the correct symbol is selected at its current, shifted
position while the displayed previews keep the request snapshot. They must fail if the decoded locations lose their
buffer/version metadata and navigation selects the old offset. Test-only change in the already-scoped test file; nothing else.

## Implementation notes

- fe-workbench has no `lsp-types`/`serde_json` dependency (dev only; no Cargo change allowed), so it can't name `Location`. `LspNavigation::lens_locations(&Command)` parses the third argument in fe-lsp, and `Workbench::show_references` takes the locations generically with `LspNavigation::decode_references` as the decoding function.
- Revision 3: each `CodeLens` carries its request's `Admitted` (`CodeLens::context`, shared by the lenses of one answer, kept through `codeLens/resolve` and display). `decode_references(root, origin, locations, cancel)` decodes against it: the server's sent text of the lens's file and of the other open documents as they were when the request was admitted, in the server's encoding, the lens's file first, disk only for files the server doesn't have open. An edit to any buffer after the answer, or a click before anything is re-asked, changes nothing in the previews. A request whose buffer moved on (text version or opening) before its answer is resolved or shown is dropped, and the next ask replaces it. No queue or cache beyond the `Admitted` kept with the answer.
- A references command with unusable arguments shows the unsupported toast and sends nothing, even if the server lists it.
- `references`' grouping, sorting and de-duplication moved into `group_references` / `sorted`, shared with `decode_references`.
- `run_lens` has no window (and `workbench_commands.rs` is out of scope), so the panel is shown from a task in the active window.
- T186 F1: a slot tracks the document's `(session, incarnation)`; another opening retires the lenses and requests, and an answer from a retired opening is dropped. A document with no opening also retires them, and an answer is checked against the manager's current opening. Covered by `a_restarted_server_is_asked_for_lenses_again`.

- Resolve is admitted on the server thread only while the session still has the lens's document at the version the lens was asked on (not just the same opening), so an edit drained between queuing and admission sends nothing.
- `an_edit_drained_before_request_admission_sends_no_request` and `an_edit_drained_before_resolve_admission_sends_no_resolve` ask the manager directly, edit, then admit, so the server thread drains the edit first; the resolve one fails without the version check.
- The retained-origin and edited-other-document tests open the References row after the edit and assert the symbol is selected where it is now; both fail if decoded locations lose their buffer/version snapshot.
