---
task_uid: "01m4t3qw161tsnkyzvwafwdj0f"
task: "T629"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test tests::ex_user_commands::diffsplit_refuses_a_range: :3diffsplit a.rs, :3diffs a.rs, :.diffsplit a.rs, :0diffsplit a.rs and :%diffsplit a.rs send no host command and show 'E481: No range allowed'; :3diffsplit (no file) and :3diffsplit! a.rs show E481 too, not E471 or E477"
    required: true
    manual: false
  - id: "C2"
    text: "the existing diff_commands test still passes unchanged: ranged :diffget and :diffput still send their rows, and unranged :diffsplit a.rs still sends DiffSplit"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-vim and cargo clippy -p fe-vim --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# fe-vim: refuse a range on :diffsplit with E481, before its bang and argument, as nvim does (T548 review F1)

## Problem

Review of T548 (codex, review egvgnj, 2026-10-12), F1 medium, never filed: `diff_command`
(`crates/fe-vim/src/ex.rs:557`, the `:diffsplit` branch at :565) never sees the parsed range, so
`:3diffsplit a.rs` and `:3diffs a.rs` silently drop the range and hand `HostCommand::DiffSplit` to
the host. `:diffsplit` takes no range (unlike `:diffget`/`:diffput`, whose rows come from it), and
nvim refuses every ranged form before reading the bang or the argument. Checked with nvim 0.12.6
on 2026-10-13:

- `:3diffsplit a.rs`, `:3diffs a.rs`, `:.diffsplit a.rs`, `:0diffsplit a.rs`, `:%diffsplit a.rs`
  give `E481: No range allowed`;
- `:3diffsplit` (no file) gives E481, not E471;
- `:3diffsplit! a.rs` gives E481, not E477.

The difference also changes whether later macro keys run, since an error ends a replay.

## Change

In `crates/fe-vim/src/ex.rs`, pass the parsed range (`&[Address]`) from `command` into
`diff_command`, and for `:diffsplit` (any abbreviation `is_diff_name(name, "diffsplit", 5)` accepts)
return `ExError::NoRange` when the range isn't empty, before the `!` check and the argument check,
as `host_command` does. `:diffget` and `:diffput` keep taking a range unchanged.

Test in `crates/fe-vim/src/tests/ex_user_commands.rs` (T548's own test file), beside
`diff_commands`: a new `diffsplit_refuses_a_range` that feeds each ranged line above through the
existing `host` helper and asserts no host command and the E481 message.
