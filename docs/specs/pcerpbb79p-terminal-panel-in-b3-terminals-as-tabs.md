---
task_uid: "01m75b0mydznzw62pcerpbb79p"
task: "T349"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::terminal_b3 (cargo nextest run -p fe-workbench) pass: each session is a tab in the bottom panel's strip (no session list); OSC 133 C without D shows the spinner, a D with a non-zero exit (or a failed task) the error mark, a zero exit neither; the active tab has the accent top line"
    required: true
    manual: false
  - id: "C2"
    text: "The terminal's default font is Code small (12/18) when terminal.font_size is unset"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Terminal panel in B3: terminals as tabs in the panel strip, a spinner while a command runs, a mark for a bad exit, Code small text (Mac T1552)

# Terminal panel in B3: terminals as tabs, running and failed marks

Spec: `docs/design/design-system.md#terminal`. Uses the theme's ANSI table.

## Change (`crates/fe-workbench/src/terminal_panel.rs`, `terminal_find.rs`)

- Terminals are tabs in the bottom panel's own strip (today a session list on
  the right): the active tab with an accent top line, a spinner on a tab whose
  foreground command is still running (OSC 133 C seen without D, or a task
  still running), a shinsha mark on one whose last command or task exited
  non-zero. `␣\`` new terminal and `␣1`–`␣9` switch, as the design's keys.
- Code small (12/18) on the editor surface.
- Paths in output are links (exists): check the link style is the accent
  underline on hover.
- An audit scene `audit/terminal.rs` on the audit card's fake session (two
  finished commands, one running), both themes, with probe points on the tabs.
- Command blocks, Send to an agent, agent terminals and history suggestions
  are separate work.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `terminal_panel.rs` and `terminal_find.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/terminal.ftl` (keys `workbench-terminal-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Linux-OK**.

## Implementation notes

- **The strip.** The tabs are the terminal panel's own strip, the row that
  held its `+` and kill buttons, under the dock's strip (Problems, Terminal,
  Git log): "own header, own tabs where it has them" in the design's
  anatomy table. Each session is a kiln `Tab` in a `TabBar` (the editor's
  tabs), so the active tab has the 2 px accent line along its top while the
  panel has focus, as every strip's active tab has; the close button (or a
  middle click) kills its session, a double click renames it, and the strip
  scrolls sideways. `+` and kill stay at its right. One session shows its
  tab too (the old list showed from two). Putting the sessions in the
  dock's strip itself would need a kiln-shell `DockItem` API for a panel's
  sub-tabs; not done here.
- **Marks.** `fe-terminal` tracks the foreground command from OSC 133:
  `C` makes it running, `D;<code>` finished with that code
  (`Terminal::command_status`, `CommandStatus`). `TerminalView` reads it on
  each wakeup and emits `TerminalViewEvent::CommandStatus` when it changes,
  so a tab that isn't drawn (behind the active one, or with the dock
  hidden) follows its command too. A tab shows `LoaderCircle` while its
  command or task runs (the run widget's static spinner glyph), `CircleX`
  in `status.error` once its last command or its task exited non-zero, else
  the terminal icon. A passed task keeps its check mark.
- **Code small.** The terminal's rows are Code small's proportion, 18 px at
  the default 12 px (were 15 px, T1515's 1.25 line height); a size set in
  `terminal.font_size` keeps the same proportion. Box drawing no longer
  meets row to row at that height.
- **Links.** `fe_terminal::link_at` has no caller in the terminal view on
  this tree, so there's no link hover style to check: a follow-up.
- **Keys.** `␣\`` and `␣1`–`␣9` are not bound: they need a switch-to-N
  command and lazyvim-preset maps (`crates/fastedit/presets/lazyvim.vimrc`,
  the command registry and `commands.ftl`), as the design's other `␣` keys
  do: a follow-up.
- **Audit scene.** `audit/terminal.rs` opens three fake sessions (running,
  failed, passed) and probes the active tab, the two inactive tabs and the
  strip's ground. The audit builds only on macOS, so its probe points are
  derived from the layout (250 px panel, 34 px strips, 42 + 248 px before the
  centre, ~145 px tabs as the Linux tests measure them), unrun: the Mac's
  audit run checks them.
- **Files outside the scope:** `crates/fe-terminal/src/{shell,session,lib,pty_loop}.rs`
  and `crates/fe-terminal-view/src/{view,lib}.rs` (the command status),
  `crates/fe-workbench/src/task_run.rs` (matches the event enum's new
  variant), `src/l10n.rs` and `l10n-allow.toml` (the catalogue), the
  terminal tests under `src/terminal_*_tests.rs`, `task_run_tests.rs` and
  `src/tests/mod.rs` (the list became the strip, the 18 px rows),
  `examples/screenshots/audit_scenes.rs` (the scene's `mod` line) and
  `docs/terminal-shell-integration.md` (C and D are used now).
