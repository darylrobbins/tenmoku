---
task_uid: "01m5r01bz918p7de6ms77h4s37"
task: "T334"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Buffer ex commands validate their ranges and counts as nvim does (E16 backwards, errors for 0 and past the highest buffer, overflowing counts rejected), $ addresses the last known buffer, deletion waits for its replacement, synchronously completed path switches record history, escaped filenames are decoded. Tests buffer_ranges_match_nvim, dollar_addresses_last_known_buffer, delete_waits_for_async_replacement, sync_path_switch_records_history, escaped_filename_target. Known limits (T344): when Save As cancels the replacement a deletion waiter can stay pending after an intervening activation or open (review F2); range validation is coupled to numeric count extraction so :%bf, :$bl and :.bn 2 are rejected and symbolic backwards ranges like :.,1bn are not (F3); and backwards-order errors do not precede endpoint validity as in nvim, e.g. :2,0b gives E16 rather than E493 (F4). T344 fixes all three."
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Fix buffer ex-command range validation, $, async deletion and history recording (T326 F8-F12)

## Problem
T326 (Mac T1136, vim buffer ex commands) review round 3 left these findings, carried here as documented limits of T326:
- F8 (medium, `crates/fe-vim/src/ex/buffers.rs`): buffer commands discard range validation when choosing their target: `:0bd 2`, `:9bd 2`, `:0,2b` run where nvim gives an error, backwards `:2,1bn` / `:2,1bf` run where nvim gives E16, and an overflowing prefix count becomes 1.
- F9 (medium, `vim_buffers.rs`): `$` resolves to the highest listed buffer, not the last known one; nvim's `$` addresses a deleted highest buffer.
- F10 (medium): deletion assumes its replacement tab has already opened; when the open is asynchronous, intermediate tab activations can cancel the deletion.
- F11 (medium): the path-opening branch returns before recording history even when the switch completed synchronously.
- F12 (low): escaped filenames are passed to literal substring matching unchanged.

## Change
Validate ranges exactly as nvim (E16 for backwards, errors for 0 and over the highest number, overflowing counts rejected); resolve `$` against the last known buffer; coordinate async replacement completion with deletion; record synchronously completed path switches before returning; decode filename escapes for name targets.

## Tests
Regressions in `crates/fe-vim/tests/ex_buffers_ranges.rs` and `crates/fe-workbench/src/vim_buffers_followup_tests.rs`, one per finding above, checked against the local nvim where possible.
