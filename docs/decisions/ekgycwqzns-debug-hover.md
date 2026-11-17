# Hover evaluation while stopped (debugger D11, T1854)

The debugger record (`docs/decisions/r1b9dw3g0g-debugger.md` §5 "Hover to
evaluate", §11 D11, §13) asks for the value under the pointer while stopped,
above the language server's hover. The calls made building it
(`crates/fe-workbench/src/debug_hover.rs`):

## Calls

- **Frame: the topmost frame of the stopped thread in the hovered file.** The
  call stack's selected frame lives in `DebugPanel` (private, and
  `debug_panel.rs` is outside this card's scope). A selected caller shows
  its own file, so hovering there finds that caller's frame; a file no frame
  is in gets no value (`Refusal::NoFrame`). Known limit: a recursive function
  evaluates in its topmost frame, not the selected one.
- **Provider part first.** `LspHover::hover` awaits the debugger's value before
  the server's reply: it answers at once or not at all, so the server's wait
  (up to `lsp.timeouts.hover`) never delays it. `merged` puts the value above
  everything else and uses the identifier's extent as the popover's range.
- **Identifier from the syntax tree.** `fe_syntax::larger_syntax_node` at the
  offset, kept only if its text is an identifier (not a number or keyword
  punctuation). A buffer with no tree gets nothing; `debug.evaluate` with a
  selection evaluates its text as written.
- **Capability gate before any request.** `supportsEvaluateForHovers` is
  checked before `Session::evaluate` is called, so no `evaluate` is sent
  without it; both commands say so in a toast.
- **Stale values.** `evaluate` carries the stop's epoch; the session rejects a
  stale request, and the reply is dropped unless the debugger is still in
  that epoch when it lands (children included).
- **Children.** Fetched once with `variables`, the first 20 listed, then a
  catalogue line (`workbench-debug-hover-more`, plural) for the rest.
- **Commands.** `debug.hover` shows the caret's identifier's value;
  `debug.evaluate` the selection's (the caret's word with none) with
  `Editor::show_popover`. `debug_show_value` is `pub` for the audit scene.

## Review round 1 (codex)

- **A stop is a session and an epoch (`StopId`).** Epochs count from 1 in each
  session, so a replacement session reaches the old stop's number. The plan,
  the children's request and every freshness check use both.
- **Revalidated after every await, and swept.** `LspHover::hover` checks the
  value's stop after the server's wait, just before merging: a value of a stop
  that has ended is left out and the server's text shows alone. A popover
  that shows a value is listed (`debug_shown`) and the frame hook
  (`sweep_debug_hover`, called from `sync_lsp`) closes it once its stop is
  over, by setting the editor's hover provider again, which drops the popover
  (fe-gpui has no public close; out of this card's scope).
- **Commands answer only the question asked.** `debug.hover` and
  `debug.evaluate` keep the buffer version and selection they were run at and
  show nothing if either changed; the task is held in `debug_command`, so a
  later command replaces (cancels) it.
- **Text is lossless.** Type, value and children are each in a fence longer
  than any run of backticks inside them; nothing is collapsed. The type is in
  a fence too, not prose, since its text is the adapter's.
- **Tests with a controlled language server.** `LspHover::server` (test-only)
  stands in for the server's answer, held back on a channel when a test
  wants it late (C3, F1, F5).

## Mac look check (C6)

`cargo run -p fe-workbench --example screenshots -- audit target/ui-audit` on
this Mac (2026-11-17): the full run wrote 79 captures; `debug_hover_dark`
(Yohen) and `debug_hover_light` (Hakuji) rendered and their probe
(`surface.raised` on the popover's ground) passed, and the scene's `check`
step failed the run unless the popover's text was the type, value and both
children. The scene does not skip on `SCREENSHOTS_ALLOW_OTHER_SCALE`.
Looked at: the popover sits below the caret on `counts` with the type line,
the type, the value and the children each in a code block, over the
bottom panel's top edge.

- **The scene sets the popover with `show_popover`, not through the
  command.** The fake adapter answers on its own thread and its answer wakes
  nothing, so a frame-chained scene never saw the command's async popover
  settle (it ended "still changing after 10 frames" or with no popover). The
  scene uses the same markdown the command builds, which
  `tests::debug_hover` reads through `popover_text`; the path through the
  command and provider is the tests' (C3, C4).
- **Flake seen, not ours.** Two full audit runs aborted with "still changing
  after 10 frames" in `debug_panel_dark` and in this scene's earlier
  command-driven form while rust-analyzer's "Indexing" progress was
  notifying; the run above passed whole.
