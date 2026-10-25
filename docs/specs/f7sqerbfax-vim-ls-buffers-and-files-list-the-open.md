---
task_uid: "01m5qphy16xnp5vvf7sqerbfax"
task: "T328"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "C1: fe-vim tests/ex_ls.rs: :ls, :buffers and :files (and :ls!) parse to the buffer-list command with nvim's abbreviation boundaries; the Pick the host builds from rows has the columns number, flags, name, line and header text 'Buffers'"
    required: true
    manual: false
  - id: "C2"
    text: "C2: vim_ls_tests::rows: with three files open, one dirty and one active and one the alternate, the rows are in buffer-number order with flags '%a' for the active, '#' for the alternate, '+' for modified, a name relative to the root, and the caret line as 'line N'"
    required: true
    manual: false
  - id: "C3"
    text: "C3: vim_ls_tests::choose: choosing a row runs :b N for it (so it activates or opens as the buffer card says) and focus returns to the editor; dismissing the picker changes nothing and focuses the editor"
    required: true
    manual: false
  - id: "C4"
    text: "C4: vim_ls_tests::fuzzy: typing in the picker filters by name as the other vim picks do (vim_pick.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "C5: cargo test -p fe-vim -p fe-workbench and cargo clippy -p fe-vim -p fe-workbench --all-targets -- -D warnings pass; no existing test is edited except that ex_buffers.rs other_b_names_are_unchanged drops ':buffers' from its list of unsupported names, which :buffers now is a supported command"
    required: true
    manual: false
---

# Vim :ls, :buffers and :files list the open files in a picker with their numbers, flags and lines, and choosing a row switches to it (Mac T1142)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" A vim user lists buffers with :ls and jumps with :b N. The :b commands (T1136) are useless without seeing the numbers, and fastedit's open tabs can be many.

## Current state

- `:marks` and `:registers` already show a picker via `HostCommand::Pick` (fe-vim ex/pick.rs, host vim_pick.rs `show_vim_pick`), with rows, columns and a `PickThen` action. :ls needs a PickThen that runs a buffer switch.
- The buffer registry comes from T1136 (vim_buffers.rs).

## Change

- Extend ex/pick.rs with the :ls picker rows (built host-side because only the host knows tabs), a `PickThen::Buffer(n)` arm; ex.rs parses :ls/:buffers/:files. nvim prints a multi-line message; a picker is the fastedit equivalent already used for :marks, as VS Code's vim does with its quick pick.
- Flags: `%` current, `#` alternate, `a` active (shown), `+` modified, `h` hidden (not shown in any pane); `:ls!` also lists unlisted nothing (accept, same as :ls).

## Tests

New files fe-vim tests/ex_ls.rs and fe-workbench vim_ls_tests.rs (declared in lib.rs). Depends on T1139 only to serialise ex.rs and vim_host.rs.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.
