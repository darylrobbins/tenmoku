---
task_uid: "01m4ewtqdrrqsvf0xhaa5n44w0"
task: "T217"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test tests::ex_host::host_commands_are_handed_over: :w, :w!, :q, :q!, :wq, :x, :exi, :e src/main.rs, :e!, :qa, :qa! and :wa (and their abbreviations from :help) each leave the matching HostCommand in take_host_command(), which a second call returns as None, and change no text, cursor or register; :5w, :q foo and :w path are errors with a message in VimState::message() and leave no HostCommand"
    required: true
    manual: false
  - id: "C2"
    text: "Every earlier corpus and unit test passes with its files unchanged; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs describe the HostCommand contract"
    required: true
    manual: false
---

# fe-vim: :w :q :wq :x :e :qa :wa parsed and handed to the host as a HostCommand

# fe-vim: :w :q :wq :x :e :qa :wa parsed and handed to the host as a HostCommand

Milestone M7 "Vim", second batch (resized), card 05b: the host-command part of the old card 5.
Read 05a (the `:` line and `src/ex.rs`). Design: `docs/decisions/vim-engine.md` §1 and §9 item 12.

**Clean room** as every M7 card: `:help :w`, `:help :q`, `:help :x`, `:help :e`, `:help :qa`,
`:help :wa` for names, abbreviations and what each accepts; never source, hjkl-ex, Zed or Helix.

## Change (`src/ex.rs`, `src/state.rs`, `src/commands.rs`, `src/lib.rs`)

- fe-vim parses but doesn't run file and window commands: `:w[rite]`, `:w!`, `:q[uit]`, `:q!`,
  `:wq`, `:wq!`, `:x[it]`, `:exi[t]`, `:e[dit] {path}`, `:e!`, `:qa[ll]`, `:qa!`, `:wa[ll]`.
  Submitting one leaves `VimState::take_host_command() -> Option<HostCommand>` for the host and
  changes nothing else (text, cursor, mode back to normal as after any ex line).
- `pub enum HostCommand { Write { force }, Quit { force }, WriteQuit { force, only_if_modified },
  Edit { path: Option<String>, force }, QuitAll { force }, WriteAll }`, exported from `fe_vim`.
- A range or argument a command doesn't take (`:5w`, `:q foo`) is an error with nvim's message in
  `VimState::message()`. `:w {path}` (save as) is an error for now ("not supported yet").
- No corpus: nvim would write or quit. Unit tests only.

## Tests (new files)

`src/tests/ex_host.rs`, declared in `src/tests/mod.rs`.

## Size

About 150 non-test lines.

## Out of scope

Running them (06c); `:w {path}`, `:sav`, `:up`, `:wn`, `:bd`, tabs and windows.

## Decided (lead defaults; the user may revisit)

- **fe-vim parses every ex line but runs only editing commands.** File and window commands come out
  as a `HostCommand` the host maps to fastedit commands (06c), so fe-vim stays GPUI-free and doesn't
  learn about tabs or files.
