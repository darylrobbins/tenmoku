---
task_uid: "01m6aknn04mebj31eye3cme3xa"
task: "T262"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "A replayed \u003cCR\u003e or \u003cBS\u003e held back while a held prefix resolves is dropped once the line it was typed on has closed, even if the prefix's keys open another line or a command suspends the replay meanwhile"
    required: true
    manual: false
  - id: "C2"
    text: "A recording's diffs of the command line are grapheme-aligned: deleting or replacing a combining character records what a replay takes out"
    required: true
    manual: false
  - id: "C3"
    text: "A \u003cCmd\u003e continuation whose tail leaves keys held for a longer map keeps the host's line until they resolve, and what they resolve to shows in the input"
    required: true
    manual: false
  - id: "C4"
    text: "Existing tests pass unedited; clippy passes"
    required: true
    manual: false
---

# Command-line maps replay: a held-back line key keeps its line's lifetime across reopen and suspension, grapheme-aligned recording diffs, a <Cmd> continuation owns its line until held keys resolve (T222 review F4 F5 F9)

Follow-up to T222's review round 2 (medium findings F4, F5, F9; T222's C2, C3 and C4 narrowed to documented limits). Needs T222's code (`replay_from`'s held-back line key, `live_lines`, `EditorView::line_backspace`).

- F4 (`crates/fe-gpui/src/vim.rs` `replay_from`): a replayed `<CR>`/`<BS>` held back while a held prefix resolves is dropped only if no command line is open once the prefix's keys have run. If they cancel the line and open another (`j -> <BS>/a`, `jk -> Z`, replay `:j<CR>`), the key goes to the new line, where the live line drops it. A suspension (`<Cmd>fe:editor.save<CR>` among the prefix's keys, or a host command) turns it into an ordinary queued key (`put_back`), losing the boundary. Track the original line's lifetime through reopening and suspension; tests for Enter and Backspace.
- F5 (`vim.rs` `record_command_line`, `sync_line_text`, the literal-key diffs): replay now deletes graphemes, but the recording diffs emit one `<BS>` per scalar and take the common prefix by scalars. Record a paste of `s/a/b/gé`, one physical Backspace, then a map to Enter: two `<BS>`s are recorded, and the replay also deletes the `g`. Make the diffs grapheme-aligned and consistent with replay; cover combining-character deletion and replacement before a terminating map.
- F9 (`vim.rs` `end_live_line`, predates T222's round 2): a `<Cmd>` continuation is declared over even when its recursive tail leaves a prefix held (`zz -> <Cmd>s/a/b/<CR>j`, `jk -> Z`): the visible text is published, and the later resolution of `j` replays with no line snapshot and never reaches the input. Keep the live line's ownership until the held keys resolve, or resolve them through the bar against the input.
