---
task_uid: "01m4smynm4nd58n5zdswmdxpw9"
task: "T76"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-terminal tests/shell_integration.rs: Terminal::cwd() returns the percent-decoded path of the last OSC 7 file:// URL a session printed, BEL- or ST-terminated and split across writes; None before any, and malformed URLs (no file://, relative path, bad percent escape) leave it unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal tests/shell_integration.rs: with scrollback 100 and 40 OSC 133;A prompts each followed by 10 output lines, scroll_to_prompt(false) from the bottom walks back through every prompt still in history, newest first, each time with that prompt's row at the top of the view (its text starts with the prompt), and scroll_to_prompt(true) walks forward to the bottom; it still lands on prompt rows after a column-changing resize that rewraps lines, and after CSI 3 J only the marks on screen remain"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench terminal_shell_integration_tests.rs: terminal.new starts the new session in the active session's OSC 7 directory, and in the panel's directory when the active session reported none or its directory no longer exists"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench terminal_shell_integration_tests.rs: terminal.previous_prompt and terminal.next_prompt are palette commands in the terminal context that scroll the active session's view to its previous and next prompt mark, and do nothing in a session without marks"
    required: true
    manual: false
  - id: "C5"
    text: "docs/terminal-shell-integration.md gives zsh, bash and fish snippets for OSC 7 and OSC 133 guarded by TERM_PROGRAM=fastedit; cargo test and cargo clippy --all-targets -- -D warnings pass for fe-terminal and fe-workbench"
    required: true
    manual: false
---

# Shell integration in the terminal: parse OSC 7 and OSC 133 in fe-terminal, open terminal.new in the active terminal's directory, and scroll between prompts with terminal.previous_prompt and terminal.next_prompt (Mac T613)

Filed on argon from Mac T613 (card ~/coordination/argon-cards/T613.json rev 3). Mac task uid 01m4smra8eq09wt18tgxa75khj. Depends on argon T71 (Mac T607; both edit terminal_panel.rs). Its other dependencies, Mac T578 (argon T62) and Mac T579 (argon T63), are on argon main. The body's "Decided by the lead overnight" section stands: take those calls as made. Title the host commit "... (Mac T613)".

**Needs argon's trunk.** fe-terminal, fe-terminal-view and the terminal panel's tabs, find row and
settings (T573, T574, T578, T579, T585, T586) are on argon's main only; stack 6 is not yet on Mac main.
Every path below is as on argon's main.

## Problem

The terminal knows nothing about the shell running in it. `terminal.new`
(`crates/fe-workbench/src/terminal_panel.rs` `TerminalPanel::add`) always starts in the panel's `cwd` (the
project root), so a second terminal opened while working in `src/deep/dir` starts back at the root, where
VS Code, iTerm2, WezTerm, kitty and Ghostty open it in the active terminal's directory. And there is no
way to jump between commands in a long scrollback; VS Code (`workbench.action.terminal.scrollToPreviousCommand`),
iTerm2 (cmd-shift-up) and kitty (`scroll_to_prompt`) do this with prompt marks.

Shells report both with standard escapes: OSC 7 (`ESC ] 7 ; file://host/path ST`) for the working
directory, and OSC 133 (FinalTerm / VS Code / iTerm2 semantic prompts: `133;A` prompt start, `133;B`
command start, `133;C` output start, `133;D[;exit]` command end). alacritty_terminal 0.26's VT parser
does not handle either: `vte-0.15.0/src/ansi.rs` `osc_dispatch` matches 0, 2, 4, 8, 10-12, 50, 52, 104,
110-112 and logs the rest as unhandled. `crates/fe-terminal/src/pty_loop.rs` `PtyLoop::read` feeds
the pty's bytes straight into `Processor::advance`, so nothing in fastedit sees them either.

## Change

### fe-terminal (GPUI-free)

1. **Scanner.** A new `crates/fe-terminal/src/shell.rs` holds a byte-level OSC scanner that runs over
   every slice `PtyLoop::read` feeds the parser, keeping its state across reads (a sequence may be
   split anywhere). It recognises OSC 7 and OSC 133 terminated by BEL or ST (`ESC \`), at most 4 KiB
   long (longer ones are dropped). `PtyLoop::read` advances the parser up to and including each
   recognised sequence's terminator, then records it with the term still locked, so a mark is at the
   cursor as the shell left it. The parser still receives every byte (it ignores these OSCs).
2. **OSC 7 cwd.** `file://<host>/<percent-encoded path>`: the path is percent-decoded to a `PathBuf`;
   the host is not checked (see decisions). Decoded bytes need not be UTF-8 (`OsStr::from_bytes`).
   Anything else (no `file://`, a relative path, bad percent-encoding) is ignored. `pub fn Terminal::cwd(&self) -> Option<PathBuf>` returns the last one reported.
3. **OSC 133 marks.** `133;A` records a prompt mark at the cursor's row. `B`, `C` and `D` are parsed
   and ignored for now (kept in the scanner so a later card can mark output). Marks are ignored while
   the alternate screen is active. A mark inside an unfinished synchronized update (`ESC[?2026h`, while
   `parser.sync_bytes_count() > 0`) is recorded at the cursor's row once the update is flushed.
4. **Stable rows.** alacritty's grid has no count of rows scrolled into history, and once history is
   full it rotates its oldest rows out silently, so a mark kept as `history_size + line` drifts. fe-terminal
   keeps control of trimming instead: the `Term` is built with `scrolling_history = scrollback + HEADROOM`
   (`HEADROOM` = 1,024 rows), the loop feeds the parser at most 1,024 bytes per `advance` call, and after
   each call trims history back to `scrollback` with `Grid::update_history` (then restores the
   `scrollback + HEADROOM` limit), adding the rows it trimmed to a `u64` `dropped` count. A row's absolute
   number is `dropped + history_size + line`, which never changes while the row exists; marks store it.
   Marks whose rows are trimmed are dropped. Any other fall in `history_size` (CSI 3 J, `clear`, reset)
   drops every mark above the screen. A resize that changes the column count (alacritty reflows history)
   re-anchors marks by logical line (a row plus its `WRAPLINE` continuations) counted before and after
   the resize in `Terminal::resize`. Lock order: the `Term` lock, then the shell state's.
5. **Prompt navigation.** `pub fn Terminal::scroll_to_prompt(&self, forward: bool) -> bool`, VS Code's
   rule: the terminal remembers the mark it last scrolled to, and forgets it when the view returns to the
   bottom (`scroll_to_bottom`, `input`). Backward goes to the mark before the remembered one, or, with
   none, to the last mark above the shell's cursor row (the prompt before the current one); forward goes
   to the mark after the remembered one, or, with none after it, scrolls to the bottom and forgets. The
   target row becomes the view's top row, clamped to the history (a mark already on screen at the bottom
   can't move up; the jump still counts as remembered). Returns whether there was a mark to go to.
   `pub(crate) fn Terminal::prompt_lines(&self) -> Vec<i32>`: the grid lines (negative in history) of the
   marks still in the grid, oldest first, for vi copy mode's `[[` and `]]` (T614). The loop also keeps
   the total rows scrolled into history (`dropped + history_size` growth) readable crate-wide for that
   card's frozen view.

### fe-workbench

6. `terminal.new` (`TerminalPanel::add`) starts the new session in the active session's `Terminal::cwd()`
   when there is one and it is a directory now, else as today (`start_dir(&self.cwd)`). The first
   session (`terminal.toggle`) still starts in the panel's directory.
7. `terminal.previous_prompt` ("Terminal: Previous Prompt") and `terminal.next_prompt` ("Terminal: Next
   Prompt") join the registry in `terminal_panel.rs` (`TerminalOp::PreviousPrompt`, `NextPrompt`, terminal
   key context) and `Workbench::run_terminal_op` (`crates/fe-workbench/src/workbench.rs`): they call
   `scroll_to_prompt` on the active session and notify its view. No default key bindings (keymap files are
   held by helium's T462).

### Docs

8. New `docs/terminal-shell-integration.md`: what fastedit does with OSC 7 and OSC 133, and snippets to
   enable them, each guarded by `[ "$TERM_PROGRAM" = fastedit ]` (fe-terminal already sets it):
   - zsh: `precmd`/`preexec` hooks (`add-zsh-hook`) printing `133;D;$?` then `133;A` before the prompt,
     `133;B` at the end of `PS1` (wrapped in `%{ %}`), `133;C` in `preexec`, and OSC 7 with the
     percent-encoded `$PWD` in `chpwd` and once at startup.
   - bash: the same through `PROMPT_COMMAND` (D, A, OSC 7), `\[ \]`-wrapped `133;B` at the end of `PS1`,
     and `133;C` from `PS0`.
   - fish: fish 4.0+ emits OSC 133 itself; add OSC 7 with a `--on-variable PWD` function (and call it once).
   Note that remote shells over ssh report the remote path, which `terminal.new` ignores when it isn't a
   local directory.

Tests: `crates/fe-terminal/tests/shell_integration.rs` drives a real session whose shell is `/bin/sh -c`
printing the escapes with `printf` (as `tests/session.rs` and `tests/search.rs` do), with a small
scrollback so overflow is cheap; `crates/fe-workbench/src/terminal_shell_integration_tests.rs`, declared
with `#[path]` from `terminal_panel.rs` like `terminal_find_tests.rs`, sets the panel's test shell
(`set_shell`).

## Decided by the lead overnight

- **No automatic shell injection.** VS Code injects its scripts into bash/zsh/fish/pwsh; fastedit doesn't
  (it would mean rewriting `ZDOTDIR` or `--rcfile`, which fights users' dotfiles). The doc gives opt-in
  snippets, as iTerm2's and WezTerm's do. An auto-inject setting can be a later card.
- **OSC 7's host is not checked** against the local hostname: `terminal.new` uses the path only if it is
  a directory here, which covers ssh sessions in practice.
- **Only `133;A` marks for now**; `B`/`C`/`D` are parsed and dropped. Command decorations, exit-status
  gutters and "select command output" need them and are later cards.
- **Navigation follows VS Code's scroll-to-command rule** (remembered mark, forgotten at the bottom),
  with the mark at the top of the view, as VS Code and iTerm2 place it.
- **No default keys** for the prompt commands until T462's keymap files are free; vi users get `[[` and
  `]]` in vi copy mode (T614), which builds on this one's `prompt_lines`.
- **History headroom** of 1,024 rows (feeding at most 1,024 bytes per parser call) makes the row count
  exact for anything but a program scrolling more than 1,024 rows from 1 KiB of output (only `CSI n S`
  can); that drift is accepted.

## Implementation notes

- The loop's parser is a `Processor` whose synchronized-update timeout never
  pends, so vte never holds output back. The loop holds a synchronized
  update itself, from `CSI ? 2026 h` to exactly `CSI ? 2026 l` (vte's rule),
  for at most 150 ms or 2 MiB (vte's limits). A repeated exact
  `CSI ? 2026 h` while it is held renews the 150 ms, as vte does, and
  `CSI ? 1049 ; 2026 h` freezes the marks, switches screens, then starts
  the update. A matcher looks at each held byte once, so adjacent short
  updates are fed in one pass. The deadline is checked after every poll,
  not only on a quiet one, so an update times out while output keeps
  coming. When the update ends it
  replays the held bytes through the same 1,024-byte calls, scanning and
  history bookkeeping as any other output, then wakes the view once. A
  prompt in an update is therefore marked where the shell printed it, on
  the screen that was active then. Clears and output longer than the
  headroom inside an update are counted exactly.
- The scanner also stops at the final byte of a sequence that clears
  history (`CSI 3 J`, `ESC c`), and the loop feeds the parser that byte in a
  call of its own. `clear` sends `CSI 2 J` (which alacritty scrolls into
  history) and `CSI 3 J` in one write. In a single call the rows scrolled
  in just before the clear went uncounted.
- It also stops before the final byte of `CSI ? 1049 h`, where the shell
  state notes the marks' logical lines and the view's offset while the
  primary grid is still visible (`ShellState::freeze`). A resize on the
  alternate screen then reflows the hidden primary grid, and the marks are
  placed again from those logical lines as soon as it shows. Only a resize
  there with no frozen anchors (the screen entered some other way) drops
  the marks.
- `Terminal::resize` changes rows, then columns, one `Term::resize` each,
  re-anchoring the marks by logical line after each. Before each step it
  lifts the history's limit (`grid_mut().update_history`, or
  `Term::set_options` for the hidden primary grid, the only way to its
  limit, which also resends the title), so alacritty's reflow never
  truncates rows at the top. The trim that follows, at once or when the
  primary grid shows again, drops the excess rows and counts them.
- The remembered prompt is forgotten whenever the view moves to the bottom:
  `scroll_to_bottom` and `input` as specified, and also `scroll`, `find`
  and `resize` when they take the view from history to the bottom, and a
  history clear. A hidden resize forgets it when the primary view, scrolled
  back as it was hidden, shows at the bottom. Forgetting also clears the
  frozen copy, so typing on the alternate screen sticks. A prompt jump that
  is clamped at the bottom is still remembered.
- `terminal.next_prompt` with no remembered mark does nothing (the view is
  where the user left it), so in a session without marks both commands
  leave the view alone (C4).
- The bash and zsh snippets in docs/terminal-shell-integration.md split the
  hooks. The first, run before any other, sends `133;D` and OSC 7 and
  returns the status it saw, so later hooks still see the command's status.
  The last sends `133;A`, after any other hook's output. In bash both go
  around an array `PROMPT_COMMAND` (bash 5.1) as well as a string one,
  joined by newlines so a string ending in `;` still parses. They were checked
  under `script` with zsh 5.9, bash 5.3 and fish 4.2, beside an existing
  hook that reports `$?` (fish sends `133;A;click_events=1`, which the
  scanner accepts).
