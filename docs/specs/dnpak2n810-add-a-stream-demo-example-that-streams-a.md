---
task_uid: "01m4p2a03was8c93dnpak2n810"
task: "T9"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo build -p fe-gpui --example stream_demo succeeds and cargo clippy -p fe-gpui --examples -- -D warnings is clean"
    required: true
    manual: false
  - id: "C2"
    text: "Run with no arguments or an unreadable file, the example exits with status 2 and prints a usage line"
    required: true
    manual: false
  - id: "C3"
    text: "The example uses only public API of fe-buffer, fe-editor and fe-gpui: no file under crates/*/src changes, and crates/fe-gpui/Cargo.toml doesn't change"
    required: true
    manual: false
---

# Add a stream_demo example that streams a file's text into the editor at the caret from stdin commands, so typing, cmd-z and undo AI can be checked during a stream (Mac T385)

Filed on helium from Mac T385 (~/coordination/helium-cards/T385.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-b.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The plan's M8 names a "demo host" (`~/.claude/plans/glittery-foraging-graham.md` line 112). The user (2026-10-11):
"Go ahead with the M8 drafters". T260's `proposal_demo` covers proposals, but it waits behind T259, T258 and T255,
and T255 is still in the inbox. Nothing in a running window streams an AI edit. So you can't check by hand that
typing during a stream works, that cmd-z skips the AI's text, and that "undo AI" takes the whole stream back. Those
are what the previous card's `EditStream` promises.

## Change

Add the example `crates/fe-gpui/examples/stream_demo.rs`. Cargo discovers it on its own (the `screenshots` example
has no `[[example]]` entry either), so `crates/fe-gpui/Cargo.toml` doesn't change and this card stays clear of T260.
It's a separate example on purpose: it needs no proposal or diff-block work.

`cargo run -p fe-gpui --example stream_demo -- <file> <source-file>`:

- **Window.** It opens `<file>` in one window with an `EditorView` (`EditorView::new`, the default dark theme, Menlo,
  soft wrap off), after `fe_gpui::init` so the default keymap and cmd-z work. Window and app setup follow
  `crates/fe-gpui/benches/keystroke/harness.rs:248` and `crates/fastedit/src/main.rs`.
- **Commands.** The example stands in for an agent host, so it takes one-letter commands on stdin, read on a
  background thread and sent over a channel to a foreground task:
  - `s` starts an `EditStream` (`Origin::Ai { agent: "demo", session: "stream-<n>" }`) on the primary selection (a
    caret is an empty range). It pushes `<source-file>`'s text in 16-byte chunks (split on char boundaries), one
    every 60 ms, from a GPUI timer loop;
  - `c` stops the stream, keeping what landed (`finish`);
  - `u` runs "undo AI" (`buffer.undo(|o| o.is_ai())`);
  - `r` runs redo AI;
  - `q` quits.
- **Edits.** It edits through `view.model_mut().buffer_mut()` and calls `cx.notify()`, so the model catches up and
  redraws.
- **Output.** It prints one line per event:
  - each chunk pushed, with the region's range;
  - each `StreamError`;
  - the result of each undo or redo, including `UndoError::Conflict` after you edit inside the streamed text;
  - each `BufferEvent::Edited`, with its origin and kind, from `Buffer::subscribe`, so your cmd-z shows up as
    `User`/`Undo` and leaves the AI's text alone.
- **Bad arguments.** With missing or unreadable arguments it exits with status 2 and prints a usage line.

It uses only the public API of fe-buffer, fe-editor and fe-gpui. No library code changes.

A by-hand check in a running window (type before, after and at the end of the streaming text; cmd-z; `u`; `r`; `c`
mid-stream) is a separate action card, like T261.
