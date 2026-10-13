---
task_uid: "01m4rqek0e6nj0bx5qjntgx74b"
task: "T55"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo build -p fe-gpui --example ai_demo succeeds and cargo clippy -p fe-gpui --examples -- -D warnings is clean"
    required: true
    manual: false
  - id: "C2"
    text: "Run with one argument, three arguments, an unreadable file, or a script file with a bad line, the example exits with status 2 after printing the error (line N: … for the script) and a usage line"
    required: true
    manual: false
  - id: "C3"
    text: "The example's //! header lists the run lines, what each seam shows and the host API it uses, and points to fe_editor::scripted_ai for the grammar; docs/ai-host.md has the same calls, the undo rule and the run lines"
    required: true
    manual: false
  - id: "C4"
    text: "The example uses only public API of fe-buffer, fe-editor and fe-gpui: nothing outside crates/fe-gpui/examples/ai_demo.rs and docs/ai-host.md changes (crates/fe-gpui/Cargo.toml included), apart from this lane's own docs/specs file"
    required: true
    manual: false
---

# Add an ai_demo example that plays a scripted fake AI in a window: ghost text, a streamed edit while typing, proposal diff blocks with accept and reject, and touched-row decorations (Mac T502)

Filed on helium from Mac T502 (~/coordination/helium-cards/T502.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1): an example; it builds and runs on Linux here (X11). Leave anything that needs macOS to the Mac and say so in the docs/specs file.

## Problem

The plan's M8 row (`~/.claude/plans/glittery-foraging-graham.md` line 112) names a "demo host". The user approved
(2026-10-12) "one example host app that drives ALL the AI seams together in a real window against a scripted fake AI
(no network, no model), as the reference for how an embedding host uses fastedit's AI APIs": ghost text, a streamed
AI edit into the buffer while the user types (one AI-origin undo group), a `ProposedChange` shown as diff blocks with
accept/reject, and a host-registered decoration provider marking the rows the AI touched; scripted from a file or
stdin so it is reproducible; documented in the example's header and a short docs section.

T501 (this one depends on it) adds `fe_editor::scripted_ai`: the script parser, `ScriptedAi`, and
`DEMO_SCRIPT`/`DEMO_TEXT`. The window halves come from T255 (`EditorView::set_proposals`, diff blocks), T258
(`accept_hunk`/`reject_hunk`, `ProposalEvent`), T259 (`proposal.*` keys), T390 (`EditorView::set_ghost_text`) and
T391 (`ghost_text.*` commands, tab, `GhostTextEvent`). The single-seam examples stay as they are:
`crates/fe-gpui/examples/stream_demo.rs` and T260's `proposal_demo`.

## Change

Add the example `crates/fe-gpui/examples/ai_demo.rs`. Cargo discovers it (as it does `stream_demo`), so
`crates/fe-gpui/Cargo.toml` doesn't change and this card stays clear of T260. Add `docs/ai-host.md`.

`cargo run -p fe-gpui --example ai_demo [-- <file> <script>]`:

- **Arguments.** None: an untitled buffer holding `scripted_ai::DEMO_TEXT`, playing `scripted_ai::DEMO_SCRIPT`.
  Two: open `<file>` and play `<script>`, where `-` reads steps from stdin one line at a time as they arrive (each
  line parsed on its own, so a typo is reported and skipped). Any other count, an unreadable file, or a script file
  that fails `scripted_ai::parse` exits with status 2 after printing the error (`line N: …`) and a usage line.
- **Window.** One window with an `EditorView`, after `fe_gpui::init` so the default keymap (tab for ghost text, the
  `proposal.*` keys, cmd-z) works, with the language set from the file's extension (Rust for the default text). Window
  and app setup follow `stream_demo.rs`.
- **Playing.** A foreground task runs the steps in order: `wait` pauses with a GPUI timer; the steps a user does in
  the window go through the view as a user would: `accept`/`accept-word`/`accept-line`/`dismiss` dispatch the
  `ghost_text.*` commands to the view, and `accept-hunk`/`reject-hunk` call `EditorView::accept_hunk`/`reject_hunk`
  with the latest proposal's hunk id; every other step runs `ScriptedAi::step` on `view.model_mut()`. After a step,
  the view shows the driver's state: `set_ghost_text(driver.ghost_text().cloned())` after `suggest`,
  `set_proposals(driver.proposals().to_vec())` after `propose`, then `cx.notify()`. The `ScriptedAi` is created on
  the view's model, so its touched-rows provider paints in the window.
- **Printing.** Each step's line and its `Report` or `StepError` (an error never stops the script); every
  `GhostTextEvent` and `ProposalEvent`; on `ProposalEvent::Accepted` it calls `ScriptedAi::note_accepted` so a hunk
  the user accepts by button or key is marked as touched too; `script done` at the end. The window stays open for
  hand play until closed or `q` on stdin (when the script isn't stdin).
- **Header.** The example's `//!` docs say what it shows, the run lines, and which host API each seam uses, with a
  pointer to `fe_editor::scripted_ai` for the grammar.
- **Docs.** `docs/ai-host.md`, at most about 80 lines: how an embedding host drives each seam, naming the calls the
  demo makes (`GhostText::new` + `EditorView::set_ghost_text` + `GhostTextEvent`; `EditStream` on
  `model_mut().buffer_mut()` and undo AI; `ProposedChange::from_text` + `set_proposals` + `ProposalEvent`;
  `add_decoration_provider` with a `DecorationSet`), the undo rule (accepts are the user's edits, streams are one AI
  group), and how to run `ai_demo` with the default or a custom script.

It uses only public API of fe-buffer, fe-editor and fe-gpui; no library code changes.

## Implementation notes

- **Commands into the view.** `accept`/`accept-word`/`accept-line`/`dismiss` dispatch
  `fe_gpui::commands::RunCommand` to the window. Its payload, `Invocation`, and the `ghost_text`-flagged
  `ContextStack` a key would carry are fe-command (kiln-command) types, which fe-gpui depends on but doesn't
  re-export; the example names them through `fe_command`, a dependency fe-gpui already has, so Cargo.toml
  doesn't change. `ghost_text.accept_line` has no default key, so a keystroke couldn't stand in.
- **A suggestion the caret left.** The view drops a suggestion that isn't live at the caret before the next
  frame (`GhostTextEvent::Dismissed`), so DEMO_SCRIPT's "move away, come back" would lose it. The host shows
  the driver's suggestion again after a step once it's live at the caret and the view shows none, and drops
  the driver's on a `Dismissed` while it's still live (turned down: `dismiss`, escape, blur, leaving insert
  mode).
- **Pace.** Steps run 250 ms apart so each can be watched; `wait` adds its pause on top.
- **Language.** `EditorView::new` detects it from the file's path; the untitled default is set to Rust.
- **Linux.** gpui's x11/wayland features are on only through kiln (the app), so the example needs
  `--features gpui_platform/x11,gpui_platform/wayland` to open a window here; the docs and header say so.
  Checked under Xvfb (X11): DEMO_SCRIPT plays to `script done` with every report and event printed; the
  ghost text, both diff blocks with buttons, and touched rows (stream and accepted hunk) render; a hand click
  on Accept prints `ProposalEvent::Accepted` and marks the hunk's rows; `tab` accepts the suggestion; a
  stdin script reports a bad line (`line 2: …`) and goes on; `q` quits.
- **Quitting** panics at exit with GPUI's "Exited with leaked handles: EditorView": fe-gpui's dev-dependency
  turns on gpui's test-support leak check, and `stream_demo` does the same on `q`. Not this card's (library
  code); left as is.
- **Left to the Mac:** running it with Metal/AppKit and cmd-z on an accepted hunk by hand.
