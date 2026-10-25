---
task_uid: "01m5stdafm7m1rbprzhahnbtvw"
task: "T344"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "A buffer deletion waiting for its replacement is cancelled at once when Save As cancels that replacement, after another open, a tab activation, or the cancellation itself; reopening the same path never executes the cancelled deletion. Tests deletion_waiter_cancelled_by_save_as_cancel, deletion_waiter_cancelled_by_other_open, deletion_waiter_cancelled_by_activation"
    required: true
    manual: false
  - id: "C2"
    text: "Buffer ex commands validate ranges independently of count extraction and as nvim 0.12.4 does: :%bf, :$bl and :.bn 2 are accepted, :.,1bn on line 3 and :2,0b, :3,0bd 1, :9,1b give E493 (backwards before endpoint validity). Tests symbolic_ranges_match_nvim, backwards_error_precedes_endpoint_error. Known limit (T350): with the corrected $ count, the current-line address is not clamped to the same logical line count, so :.,$bf with the caret on the final empty row of newline-terminated text resolves 2,1 and returns E493 where nvim accepts it (review F4); T350 clamps it."
    required: true
    manual: false
  - id: "C3"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Cancel a vim buffer deletion waiter when Save As cancels its replacement (T334 F2)

## Problem
T334 (T326 F8-F12 buffer ex-command fixes) review left F2 (medium, `crates/fe-workbench/src/vim_buffers.rs:622`), carried as a documented limit of T334: Save As cancellation leaves a buffer-deletion waiter pending. `open_once_saved` removes the read without notifying the pane after an intervening activation, so reopening the same path can satisfy the stale waiter and execute the cancelled deletion.

## Change
Track and notify the replacement operation's completion or cancellation explicitly, and cancel its deletion waiter immediately on cancellation.

## Tests
Command-to-waiter tests in `crates/fe-workbench/src/vim_buffers_cancel_tests.rs` (declared by `#[path]` from vim_buffers.rs): another open, tab activation, and cancellation while Save As delays the replacement.

## Added (review round 4 of T334)
- F3 (medium, `crates/fe-vim/src/ex/buffers.rs:176`): range validation is coupled to extracting a numeric count, so valid commands such as `:%bf`, `:$bl`, `:.bn 2` are rejected, and symbolic first addresses escape backwards validation (`:.,1bn` on line 3 must give E493). Separate validation from count extraction, resolve symbolic addresses with nvim's semantics, validate ranges even when the count is ignored or overridden; regressions for valid symbolic ranges and symbolic backwards ranges.
- F4 (low, `buffers.rs:325`): backwards-order error must take precedence over endpoint validity (`:2,0b`, `:3,0bd 1`, `:9,1b` give E493 in nvim 0.12.4, not E16); correct the existing test that asserts E16 and extend parser and host regressions.
