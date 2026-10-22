---
task_uid: "01m5h7r5c4apqv419qqf4vxc78"
task: "T219"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "A regression test in crates/fe-vim/tests/filter.rs runs ! and :r ! from a macro replay, a mapping, a :normal and a nested :normal and shows no host shell command; the user's own keys still produce one; it fails on trunk and passes with the fix"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim passes (normal_stack.rs too) and cargo clippy -p fe-vim --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# fe-vim: the filter's shell guard carries macro and :normal provenance on each invocation, not the per-key from_macro flag (T166 F1, Mac T908 follow-up)

Review follow-up of argon T166 (abandoned as a duplicate of trunk's Mac T908 = T255, 0f44fea5; its round-3 finding F1). Lead decision z39.

## Problem
Trunk guards the vim `!` filter's shell command with the per-key flag `VimState::from_macro` (`crates/fe-vim/src/ex/filter.rs` ~187, `remap.rs` ~752). A per-key "from a macro" flag is bypassed: a `:normal` suspends it, `<Cmd>` and direct invocations skip it, so keys replayed from a macro or mapping can reach the host's shell as if the user typed them. This is a safety hole in the "user's own keys only" guardrail.

## Change
Carry provenance on the invocation instead: `remap::Provenances`, a scoped `fe_vim::dispatch`, and a Nested scope through a suspended `:normal`, so keys replayed from a macro or mapping can never produce `Actor::User` for a shell command. Remove the `from_macro` dependency of the filter guard. Keep nested `:normal` within the stack limit (`tests/normal_stack.rs`; stack frames per level not larger than trunk's: measure with RUST_MIN_STACK).

The abandoned lane's work is the reference: branch `keep/t166`, commits 291b5962, dafc4b19, bc8638fd, 7175ac7c, c746154e, 29b5538d (spec notes). Port it onto trunk; do not cherry-pick the feature commits that already landed.

## Tests
Regression test FIRST, in `crates/fe-vim/tests/filter.rs`: a macro replay, a mapping, a `:normal` (and a nested `:normal`) that run `!`/`:r !` produce no host shell command, while the user's own keys still do. It fails on trunk and passes with the fix.

## Implementation notes

- **Trunk's leak.** Probed on trunk, every route the per-key flag covers was already refused (a macro's keys
  and maps, `<Cmd>fe:…<CR>` taken later, `:normal` and a suspended `:normal`). What got through: `@:`
  runs the last command line as the user's, and a line a macro submits (refused) becomes that line, so
  the user's `@:`, or the user's map to `@:`, ran the macro's `:.!tac` or `:r !date`. The regression test
  `shell_commands_carry_their_provenance` fails there on trunk.
- **Ported from `keep/t166`.** `remap::Provenances` (beside `Origin`) tags the invocation a key commits
  (`feed`), the submit after a command line's `<CR>` (`record_key`), and a map's `<Cmd>fe:…<CR>` command
  the host takes (`take_remap_command`); `fe_vim::dispatch` (now in `lib.rs`, wrapping
  `commands::dispatch`) runs an invocation in its tag's scope; `:normal`'s `drive` runs in a `Nested`
  scope and its own dispatches are scoped; `@:` runs its line as a macro's, whoever runs it.
  `VimState::from_macro` is gone, and `RemapState::idle` with it (only its reset used it).
- **Kept from trunk.** The single `SHELL_REFUSED` message (no separate `:normal`/macro messages), and the
  `global::running()` and `normal_suspended` checks beside the scope. A macro's `<Cmd>fe:` command still
  has `Source::Replay`. `running_remap_command` (fe-gpui's `run_remap_command`) stays: it tags the command
  the host took last again, whatever was read since; a taken command is matched by its id alone, as the
  host may stamp args.
- **Changed behaviour.** `@:` of the user's own shell line is refused too (it's a replay of a line).
- **Stack.** The minimum stack for `normal_stack.rs`'s recursion and for `normal_inside_itself_stops`
  (`RUST_MIN_STACK`, bisected in 4 KiB steps) is 134 and 135 KiB, the same as on trunk. `Provenances::enter`
  is `#[inline(never)]`. Trunk's filter line state (`state.filter.opened`) is not the reference's
  `Filtered` in `CommandLine`, so its boxing and the `execute_many` split weren't needed.
