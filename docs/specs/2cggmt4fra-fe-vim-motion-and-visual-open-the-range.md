---
task_uid: "01m5f6t920fzd2aj2cggmt4fra"
task: "T255"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test filter_keys_prefill_the_range (top level, in crates/fe-vim/tests/filter.rs) passes: !ip, !j, 3!!, !! and visual ! open the command line with nvim 0.12.x's probed prefills (in the test's comments)."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test filter_line_becomes_a_host_command (top level, same file) passes: :.,+2!sort and :%!tr a b leave HostCommand::Filter with the rows, the command's parts and Actor::User; :r !date and :0r !date leave HostCommand::ReadCommand below the line; :{range}! repeats the last command, or with none leaves E34; :!ls with no range is refused with a message and no host command."
    required: true
    manual: false
  - id: "C3"
    text: "The integration test apply_filter_replaces_rows_as_nvim_does (top level, same file) passes: VimState::apply_filter with fewer, more and no output rows (and output with no final line break) replaces the rows in one undo step with nvim's cursor and message, u restores them, and . after a ! filter asks the host again with the same command; VimState::apply_read_command puts the output's rows below the line as :r puts a file's."
    required: true
    manual: false
  - id: "C4"
    text: "The integration test shell_command_parts (top level, same file) passes: :%!echo % \\% # \\# gives the parts CurrentFile, literal %, AlternateFile and literal #, in order with the text between; :%!echo %:p and #:h are refused with a message and no host command."
    required: true
    manual: false
  - id: "C5"
    text: "The integration test filters_run_only_for_the_user (top level, same file) passes: :%!sort, :r !date and . after a user's filter, each invoked with Actor::Host(\"acme.x\") or Actor::Ai, leave the message 'Shell commands run only from your own keys' and no host command; :g/a/.!sort and :normal !!sort\u003cCR\u003e are refused with a message for Actor::User too; and a macro never reaches the shell on any route (C6); and cargo test -p fe-vim passes with any changed existing test named in the finish notes."
    required: true
    manual: false
  - id: "C6"
    text: "The integration test macros_never_reach_the_shell_through_remaps (top level, crates/fe-vim/tests/filter.rs) and the fe-gpui test vim::macro_tests::filters_do_not_run_from_remapped_macro_keys pass: after a user's filter, a macro whose key is mapped to . (R), a mapped filter line and a mapped :r ! line replayed from a macro, and a deferred \u003cCmd\u003efe:vim.repeat\u003cCR\u003e from a macro each leave the message 'Shell commands run only from your own keys' and no host command, while the same keys typed directly by the user (including through their own remap) still produce the host command"
    required: true
    manual: false
---

# fe-vim: !{motion}, !! and visual ! open the range's filter line, and :{range}!{cmd} and :r !{cmd} hand the command to the host, only for the user's own keys (Mac T908)

## Problem

At Mac main c7b6a04a fe-vim has no filter: `!{motion}`, `!!` and visual `!` cancel, `:{range}!{cmd}` is E492,
and T833 keeps `:r !cmd` refused. nvim, VSCodeVim and IdeaVim filter rows through an external command
(`!ip sort`, `:%!jq .`) and read a command's output (`:r !date`). The user approved `!` in the "Vim parity"
track on 2026-10-17 and, the same day, set its guardrails: trusted folders only and the user's own shell
(the workbench card, T909); only from the user's own keystrokes; `%` and `#` expanded to shell-escaped file
names; off the UI thread with a timeout and a cancel, one undo step, and a failure leaving the text as it
was (T909). fe-vim never spawns processes: like `:w` and T833's `:r`, it hands the work to the host as a
`HostCommand` (`crates/fe-vim/src/ex.rs`).

Who asked is already known where an ex line runs: `ex::run(target, actor, line)` gets the `Actor` of the
invocation that submitted it (`vim.command_line_submit`, `crates/fe-workbench/src/vim_line.rs`), and the
plugin decision draft (T839) has every plugin action invoke commands with `actor = Host("<extension id>")`
and AI features with `Actor::Ai { .. }` (`kiln/crates/kiln-command/src/invocation.rs`). The user's own
keymap.toml bindings, `[[vim.remap]]` (T873), the vimrc (T875) and `<Cmd>` remaps (T876) all run as
`Actor::User`: they count as the user. Workspace-provided config can't produce one: T909 drops a project
file's `vim.remap` and `vim.shell`, keymaps load only from the app and the user's keymap.toml, and T839 bars
workspace extensions from keymaps.

nvim's own `%` is not escaped (`:.!echo %` on `x y.txt` runs `echo x y.txt`), and with no alternate file `#`
fails `E194: No alternate file name to substitute for '#'` (probed with nvim 0.12.6).

## Change

fe-vim only; the workbench runs it in T909.

- **Keys.** A new feature module `crates/fe-vim/src/feature/filter.rs`, declared in
  `crates/fe-vim/src/feature.rs` and listed in `FEATURES`: `!{motion or object}`, `{count}!!` and visual `!`
  open the command line prefilled with the rows' range as nvim's do (`:.,.+2!`, `:.!`, `:'<,'>!`; probe each),
  carrying nothing else.
- **Ex.** A new `crates/fe-vim/src/ex/filter.rs`, wired as `crates/fe-vim/src/ex/table.rs` says (the `!`
  after a range is not a name, so `ex.rs` hands it over before the name lookup):
  - `:{range}!{cmd}` becomes `HostCommand::Filter { rows, command, actor }`; `:{range}!` with an empty
    command repeats the last filter command, or fails E34 with none (probe). A `!` inside `{cmd}` stays
    literal (nvim puts the previous command there; deliberately not).
  - `:[line]r[ead] !{cmd}` (in T833's `:r` code in `ex.rs`) becomes `HostCommand::ReadCommand { below,
    command, actor }`: the output goes below the line as T833's `:r {file}` puts a file's rows.
  - `:!{cmd}` with no range stays refused with a message pointing to the terminal panel.
  - `command` is a `ShellCommand`, a list of parts: literal text, `CurrentFile` for `%` and `AlternateFile`
    for `#`, with `\%` and `\#` literal. A `%` or `#` followed by a filename modifier (`%:p`) is refused with a
    message (not in v1). fe-vim knows no file names: the host expands the parts (T909), shell-escaping each.
- **Who may run it.** The filter and `:r !` run only for `Actor::User`. Any other actor (`Host(..)`, `Ai
  {..}`, `Lsp`, `Format`) gets the message `Shell commands run only from your own keys` and no host command;
  refused for v1, not confirmed. So does a filter line run by `:g` or `:normal` (`crates/fe-vim/src/ex/global.rs`,
  `crates/fe-vim/src/ex/normal.rs`), whatever the actor, and `.` repeating a filter (`crates/fe-vim/src/repeat.rs`)
  passes the actor of the `.` invocation, refused the same way. A macro replay already runs no host command
  (`crates/fe-gpui/src/vim.rs`, "a macro replay doesn't run"): unchanged, a stated divergence from nvim.
  `HostCommand::Filter` and `ReadCommand` carry the actor so the host checks again.
- **Applying.** `VimState::apply_filter(model, rows, output)` replaces the rows with the output's rows in one
  undo step (`u` restores them), leaving the cursor and marks where nvim leaves them (probe with `sort` and
  `tr`) and nvim's message (`3 lines filtered`); `VimState::apply_read_command(model, below, output)` puts
  the output below the row as `:r` does. `VimState::set_host_message` carries a refusal or failure.

Tests in a new integration file `crates/fe-vim/tests/filter.rs`: the prefills, the `HostCommand` each line
gives, the parts of `%`, `#`, `\%` and `\#`, the actor refusals, `apply_filter` with fixed output (fewer, more
and no rows; output without a final line break), undo, `.`, the empty-command repeat and E34.

Order: feature.rs is changed by the vim-parity chain and now the ReplaceWithRegister card; ex.rs,
ex/table.rs, ex/global.rs, ex/normal.rs and repeat.rs by T823–T833, T865–T866, T870 (remaps under `.` and
`:normal`), T893 and T906–T907. This card depends on the ReplaceWithRegister fe-vim card (after T905), T870,
T893 and T907 (after T905, T906, T825, T866), and on T833 for `:r`.
## Widened after review round 2 (Mac y34): a macro never reaches the shell

Review F1 (high): a macro's provenance is lost through remaps, so a macro key that triggers a map (e.g. `R` mapped to `.`), a mapped filter or `:r !` line, and a deferred `<Cmd>fe:vim.repeat<CR>` still yield `HostCommand::Filter`/`ReadCommand` with `Actor::User`. Fix it properly, in scope now (`crates/fe-vim/src/remap.rs` joins the scope next to `state.rs` and `crates/fe-gpui/src/vim.rs`):

- Carry a "from a macro replay" bit through `remap::Origin::Expansion`/`Tag` and the feed/record_key origin reads in `state.rs`, so every key a macro replays, and every key its maps expand to, is marked replayed.
- Mark deferred `RemapCommand`s (e.g. `Source::Replay`) so handlers can see it, and make `replay_from`/`run_remap_command_later` in `crates/fe-gpui/src/vim.rs` pass it on.
- Suppress `HostCommand::Filter` and `ReadCommand` (with the 'Shell commands run only from your own keys' message) for every replayed route, while a user's own typed remap still runs its filter.
- Regression tests (top level, in `crates/fe-vim/tests/filter.rs`, and an fe-gpui test in `crates/fe-gpui/src/vim/macro_tests.rs`): a macro with `R` mapped to `.` after a user filter; a mapped filter line and a mapped `:r !` line replayed from a macro; a deferred `<Cmd>fe:vim.repeat<CR>` from a macro; and the user's directly typed remap still reaching the host.

## Known limits (review round 2, y16 process)

- F4 (explicit change marks `'[` `']` at buffer boundaries after empty filters/reads: nvim leaves them past EOF or unset) and F7 (`g;` / `:changes` record of the logical filter/read position, including unchanged filters and empty reads) are not done here; they are follow-up cards. This card keeps the clamped marks and the physical-batch changelist positions as its documented limits.

## Implementation notes

- **Scope widened** (respec, revision 2): `!` is an operator (`Operator::Filter` in `args.rs`, dispatched
  from `operator.rs`, with `unreachable!` arms in `visual/operate.rs` and `visual/block.rs`); the line `!`
  opened and the last shell command live on `VimState` (`state.rs`); `lib.rs` exports `ShellCommand`,
  `ShellPart`, `FilterArgs` and `SHELL_REFUSED`; the new `HostCommand` variants need an arm in the
  workbench's exhaustive matches (`vim_host.rs`, `git_diff.rs`: "Not supported yet: shell commands" until
  T909; `git_commit.rs`: the commit box's "Not supported in the commit box"); and
  `crates/fe-vim/tests/ex_read_path.rs` changed (below).
- **Changed existing test:** `ex_read_path::read_parses_to_a_host_command` expected `:r !ls` refused; it
  now expects `HostCommand::ReadCommand { below: 2, .. }`.
- **Probed with nvim 0.12.4** (the 0.12.x on helium): prefills `.,.+2!` (`!ip`), `.,.+1!` (`!j`, `!k`),
  `.,.+2!` (`3!!`), `.!` (`!!`, `!w`), `.,$!` (to the last row), `'<,'>!` (visual, charwise too); the
  caret goes to the start of what the operator covers: where the motion lands if before the caret (`!!`
  and `!_` at the first non-blank, `!k` and `!gg` where they land; `!j`, `3!!` and `!G` keep the caret),
  an object's start (`!ip` at the first row's start, `!iw` at the word's); `5!!` past the end fails;
  `!G` sets `''`. `'[` and `'.` go to the range's first row, `']` to the last row put in (with none, the
  row above), whatever text changed; `:r !`'s to the first and last rows read (with none, the line below
  and the line).
  After a filter the caret is at the first row's first non-blank (or the row that took the range's
  place), the message `{n} lines filtered` only past 2 rows ('report'), and `u` puts the caret at the
  start of the range's last row; marks on rows kept stay, on rows dropped go. `:r !cmd` leaves the
  caret on the last row read (with none, on line `below`), and `u` back where it was.
- **The empty command:** C2 has `:{range}!` repeat the last command (E34 with none). nvim's `:.!` does
  nothing and its `:.!!` repeats; fe-vim takes both as the repeat (`!` alone is the one place a `!` in
  the command isn't literal), and `:r !` (empty) too.
- **`.`** is set only when the line `!{motion}` opened is submitted still starting with its prefill on the
  row it opened on (`Change::Filter`, repeated through `vim.filter`); a line typed by hand isn't
  repeatable, as in nvim. After a visual `!`, `.` takes as many rows.
- **T220 (T166 review F2/F3/F4, ported onto this card's filter):** a kept row the output rewrites (an
  emptied one too) keeps its marks when rows are put in after it (`marks.rs`'s `refilled`, set anew
  between the rewrite batch and the put); `.` makes its motion ready as the `!` did (`vim.filter` runs
  in the display rows of `gj`, `gk` and `g$`, and `zj` and `zk` among the host's and closed folds, as
  `vim.operate`); `!gc` selects the comment run as `comment::operate_object` does for any operator
  (`'<,'>!`, as nvim's object is a visual selection), `operator.rs` handing it to
  `feature::filter::operate_object`, and its `.` finds the run around the caret again the same way (the
  selection's rows noted, not a line opened), doing nothing off a comment.
- **Refused for v1** with a message, not run with something else in their place: `%`/`#` with a filename
  modifier (`%:p`, `#:h`), `#2`, `#<2`, `##`, and the `<cword>`-style words nvim expands.
- **Review round 1:** a macro replay's `.` after a filter leaves no shell command (`fe-gpui`'s
  `replay_fed` drops it, as its command line's: `crates/fe-gpui/src/vim/macro_tests.rs`); a map's
  command run while a `:normal` is suspended on it is refused (`ex/filter.rs` checks
  `normal_suspended`); `apply_filter` takes dropped rows out whole in a batch of their own, so a mark on
  the last row kept stays; `'[ '] '.` are set explicitly (`marks::filtered`); any command line opened
  or cancelled drops the line `!` opened (`cmdline.rs`), so `.` can't pick up a later hand-typed line.
  `!!` goes to the first non-blank as nvim's does (the review suggested column 0; nvim 0.12.4 gives the
  first non-blank). Scope widened again for `cmdline.rs`, `marks.rs`, `fe-gpui/src/vim.rs` and its
  `macro_tests.rs`.
- **Known limits:** redo puts the caret where undo does (fe-vim's default), not where nvim's redo of a
  filter goes. (`!gc` once failed; T220 has it filter the comment run, below.)
- **Review round 2 (rev 4):** a macro's provenance now goes through the maps it runs:
  `remap::Origin::Expansion { replayed }` carries whether a key of the map's left side was a replay's,
  `VimState` notes `from_macro` for each key read (`feed_key`, `record_key`), clears it once the replay
  and its maps are over (`take_replay` with nothing left), and a map's `<Cmd>fe:…<CR>` a macro ran is
  `Source::Replay`, which `take_remap_command` and `running_remap_command` (fe-gpui's
  `run_remap_command`) turn back into `from_macro`. `ex/filter.rs` refuses any shell command while it's
  set. Known limit: a command dispatched with no key between it and a macro's end (a palette command
  right after `@a` ends) is refused too, until the next key; the refusal fails closed. F8: `vim.filter`
  and `.` check and set a search target as `vim.operate` does (`!*` then `.` makes the new word the last
  search). F4 and F7 are T265.
- **Carried T238 F2 (`remap.rs`):** a `c` after `gc` and a count (`gc2c`) is matched against the
  operator-pending maps (`onoremap c j` comments three rows, as nvim). The unmapped `gc2c`, which nvim
  cancels and fe-vim still runs as `2gcc`, is `feature/comment.rs`'s, outside this card's scope.
