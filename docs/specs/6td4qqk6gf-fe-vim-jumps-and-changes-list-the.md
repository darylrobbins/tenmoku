---
task_uid: "01m55mj822p8ypaw6td4qqk6gf"
task: "T160"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test jumps_and_changes_list_as_nvim (top level, in crates/fe-vim/tests/ex_jumps.rs) passes: through HeadlessVim, after a series of jumps and changes, :jumps and :changes leave HostCommand::Pick with nvim 0.12.6's probed header and rows (hard-coded with the version in a comment), and dispatching a row's invocation moves the cursor and the list position as the same count of \u003cC-o\u003e or g; does on a twin HeadlessVim."
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test workbench::vim_host_tests::pick_tests::jumps_opens_a_picker_and_jumps (in crates/fe-workbench/src/vim_pick_tests.rs) passes: in a real workbench with vim on, :jumps shows the picker and Enter on an older entry puts the cursor there."
    required: true
    manual: false
---

# fe-vim: :jumps and :changes list the jumplist and changelist in a picker with nvim's columns, and picking an entry goes there (Mac T907)

Filed on argon from Mac T907 (card ~/coordination/argon-cards/zzp/T907.json rev 1; note zzp, the Vim parity track part 2). Mac task uid 01m55mez2p00vmmdxgv9mz2wdj. Base: argon trunk (mac/main synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T159 (Mac T906), T158 (Mac T905). Title the host commit "... (Mac T907)".

## Problem

At Mac main 6f032b12 `:jumps` and `:changes` are E492 in fe-vim. T865 (lent) lists `:marks` in a picker
through a generic `HostCommand` (`crates/fe-vim/src/ex/pick.rs`, run by fe-workbench's `vim_pick.rs`), and
T866 does `:registers` the same way. The user approved `:jumps` in the "Vim parity" track on 2026-10-17; the
changelist card adds the changelist `:changes` lists.

## Change

- New `crates/fe-vim/src/ex/jumps.rs` with table entries in `crates/fe-vim/src/ex/table.rs`: `:ju[mps]` and
  `:changes` list their entries through T865's pick seam (`crates/fe-vim/src/ex/pick.rs`), with nvim's columns
  and order (`jump line col file/text`, the `>` row for the current position; probe the exact layout), each
  row's text the row's content.
- Picking a `:jumps` row goes there as `<C-o>`/`<C-i>` with that count would (so the list position moves);
  picking a `:changes` row as `g;`/`g,` would.

Tests: a new `crates/fe-vim/tests/ex_jumps.rs` (the rows each lists after a series of jumps and changes, and
what picking does), and in T866's `crates/fe-workbench/src/vim_pick_tests.rs` a test that `:jumps` shows a
picker in a real workbench and picking jumps.

Depends on the `:delmarks` card (table.rs), the changelist card, and T866.
