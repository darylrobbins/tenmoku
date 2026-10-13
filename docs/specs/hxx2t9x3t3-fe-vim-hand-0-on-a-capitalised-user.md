---
task_uid: "01m4sd818r43j509hxx2t9x3t3"
task: "T67"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test ex_user_zero_range::zero: ':0Gclog' gives User { name: Gclog, bang: false, range: Some(0..0), args: '' }; ':0Gitsigns x' gives Some(0..0); ':1Gclog' gives Some(0..1); ':Gclog %' gives range None, args '%'"
    required: true
    manual: false
  - id: "C2"
    text: "Every existing fe-vim test passes unchanged (ex_user_commands, ex, ex_host included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: hand :0 on a capitalised user command to the host as the empty range at row 0, for fugitive's :0Gclog (Mac T589)

Filed on argon from Mac T589 (card ~/coordination/argon-cards/T589.json rev 1). Mac task uid 01m4scszntn9cx646n6tw61a14. No dependencies (builds on Mac T548, on argon main since the 81efccb sync). Title the host commit "... (Mac T589)".

## Problem

The user, 2026-10-13, wants the current file's history from vim as fugitive's `:0Gclog`. In fugitive the
`0` is what makes it the file's history. `:{range}Gclog` is the history of those lines (`git log -L`), and a
plain `:Gclog` is the repository's log.

fe-vim hands a capitalised user command to the host as `HostCommand::User { name, bang, range, args }`
(`crates/fe-vim/src/ex.rs` :258). `range` is the line range's buffer rows. `:0Gclog` gets `lines` (0, 0),
which `counted_lines` (:1022) clamps to line 1. So `:0Gclog` and `:1Gclog` both reach the host as
`Some(0..1)`, and the host can't tell them apart.

## Change

In `crates/fe-vim/src/ex.rs`, in the `ExCommand::Host` arm (:884):

- A `HostCommand::User` whose range is exactly the single address 0 (`lines == Some((0, 0))`) gets
  `range: Some(0..0)`, the empty range at row 0.
- Every other range is unchanged.

Update `User`'s doc comment to say so. Only `User` changes: `:0d`, `:0diffget` and the other commands behave as
today.

## Tests

New file `crates/fe-vim/src/tests/ex_user_zero_range.rs`, declared in `crates/fe-vim/src/tests/mod.rs`. It uses
`HeadlessVim` and `take_host_command`, as `crates/fe-vim/src/tests/ex_user_commands.rs` does.
