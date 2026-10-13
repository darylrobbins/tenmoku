---
task_uid: "01m4rqejsxnvq008ncypxz12w4"
task: "T54"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo build -p fe-gpui --example proposal_demo succeeds and cargo clippy -p fe-gpui --examples -- -D warnings is clean"
    required: true
    manual: false
  - id: "C2"
    text: "Run with no arguments, or with an unreadable file, the example exits with status 2 and prints a usage line before opening any window (output quoted in the decision file)"
    required: true
    manual: false
  - id: "C3"
    text: "The example prints proposal.*, editor.undo and editor.redo invocations through fe_gpui::observe_invocations, and ProposalEvents with the accept's transaction id (the source lines quoted in the decision file)"
    required: true
    manual: false
  - id: "C4"
    text: "The example uses only public API of fe-buffer, fe-editor and fe-gpui: no file under crates/*/src changes"
    required: true
    manual: false
---

# Add a proposal_demo example that opens a file with another file's text as a ProposedChange, and prints its dry runs, accept/reject events and proposal commands (Mac T260)

Filed on helium from Mac T260 (~/coordination/helium-cards/T260.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1): an example; it builds and runs on Linux here (X11). Leave anything that needs macOS to the Mac and say so in the docs/specs file.

## Problem

fastedit has no AI host, so nothing in the running app creates a `ProposedChange`, and the diff blocks, their
Accept/Reject buttons, cmd-z on an accepted hunk and the `proposal.*` keyboard commands (the previous cards) can't
be checked by hand. The plan's M8 names a "demo host".

## Change

Add the example `crates/fe-gpui/examples/proposal_demo.rs` and its `[[example]]` entry in
`crates/fe-gpui/Cargo.toml` (`test = false`).

`cargo run -p fe-gpui --example proposal_demo -- <file> <proposed-file>` opens `<file>` in one window with an
`EditorView` (Menlo, the default theme, soft wrap off), after `fe_gpui::init` so the default keymap (folding,
undo/redo and the `proposal.*` bindings) works, with the language set from the file's extension
(`EditorView::set_language`), and shows
`ProposedChange::from_text(&snapshot, 0..len, &read_to_string(proposed_file), Origin::Ai { agent: "demo", session: "demo" })`
through `EditorView::set_proposals`. It:

- prints `DryRun` for the proposal at start, one line per hunk: index, outcome, reason;
- subscribes to `ProposalEvent` and prints each event as it comes, with the transaction id for an accept;
- prints every `proposal.*`, `editor.undo` and `editor.redo` invocation it sees through
  `fe_gpui::observe_invocations`, so a keyboard command and a button click are told apart;
- on every buffer change, prints the dry run again (typing inside a hunk shows it going stale; cmd-z after an accept
  shows the text coming back);
- prints `all hunks resolved` once `is_resolved()`;
- exits with status 2 and a usage line on missing or unreadable arguments.

It uses only public API of fe-buffer, fe-editor and fe-gpui; no library code changes.

## Implementation notes

- On Linux the example needs gpui's x11 feature, which only `crates/fastedit` enables: build with
  `cargo build -p fe-gpui -p fastedit --example proposal_demo` to run it under Xvfb.
- Checked under Xvfb: the start dry run and `invoked: proposal.next_hunk` print. Accept/reject events from
  keyboard accept and button clicks were not driven by xdotool here (ctrl-alt-y didn't register); they, and
  the macOS side (cmd-z on an accepted hunk, buttons), are left to the Mac.
