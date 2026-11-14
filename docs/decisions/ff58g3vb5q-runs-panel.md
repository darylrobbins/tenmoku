# T1571: Runs panel, the list of runs at the bottom

Spec: `docs/specs/ff58g3vb5q-runs-panel-a-bottom-tool-window-listing.md`;
design: `docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`
and the Workbench anatomy rows (Rail bottom: Problems, Runs, Git log,
Terminal; Bottom panel: Problems, Runs, Terminal, Git log; key `␣r`).

## What changed

- **`crates/fe-workbench/src/runs_panel.rs`** (new). A bottom tool window
  over the workbench's `Runs` (T1570), drawn with kiln-ui's `List`, so the
  arrow keys, Home/End/Page keys and Enter work, and each row is a
  `ListBoxOption` in a `ListBox` named "Runs". A row is 24 px (`ROW_HEIGHT`).
  It shows a status icon, the title in code small (mono, at most half the
  row, truncated), the summary in UI small, muted, and the age at the right.
  The icons are a spinner (`LoaderCircle`, the terminal tab's), ✓ (`Check`)
  in `status.success`, ✕ (`X`) in `status.error`, and a dash (`Minus`) in
  `text.muted` once cancelled. A running row draws a 2 px `accent.base` line
  along its bottom edge. It fills to `progress` when that's known; otherwise
  it runs the whole width at 40% alpha.
- **Ages.** `now` under 5 s, then whole `12s`, `3m`, `2h`, `4d`, each
  rounded down. A finished run's age counts from when it ended, a running
  run's from when it began. The panel redraws once a second, a single
  timer, while it draws and a row is under a minute old. It draws only while
  the bottom panel shows it, so hidden or older lists cost nothing. A
  screen reader hears the age spelled out ("12 seconds ago"), since "12s"
  reads badly. The accessible name is "<title>, <status>, <age>". A running
  row with known progress says "Running, 60% done".
- **Summaries** come from `RunSummary`, through the catalogue with plurals:
  `Exited with code 1`, `3 passed, 0 failed`, `40 passed, 2 failed of 42`,
  `12 crates, 1 warning, 0 errors`. With no summary, the line follows the
  status: `Running…`, `Done`, `Stopped without an exit code`, `Cancelled`.
- **Opening a run.** A click on a row, or Enter on the selected one, shows
  the terminal tab of the task that made the run, focused
  (`Workbench::open_run`). The open is deferred out of the click's or key's
  update. `task_run.rs` keeps each task tab's earlier runs with its current
  one (`TaskTab::earlier`, capped at `MAX_FINISHED`, since `Runs` keeps no
  more). An older run's row therefore opens the tab its rerun took over.
  When the tab is closed, a run opens nothing and the list stays.
- **Empty state.** A centred icon, "No runs yet", and a hint naming the
  command that starts one by its palette title (`tasks.run`, "Tasks: Run
  Task…"), resolved through `command_label` so it follows the locale.
- **Registration.** `dock.rs` implements `DockItem` for the panel (id
  `runs`, icon `ListVideo`, the play triangle with list lines from T1516)
  and adds the rail action after Problems'. The rail foot is now Problems,
  Runs, Git log, Terminal. The workbench adds the panel to the bottom dock
  after Problems, so the tabs read Problems, Runs, Terminal, Git log.
  `runs.show` (`DockOp::ShowRuns`, "Show Runs") shows it focused. Its button
  is lit while it shows, and clicking the lit button hides the bottom
  panel. A layout saved showing Runs shows it again on restore, as Problems
  and the Git log do: an empty list until something runs.
- **`␣r`.** `space r` in `crates/fe-keymap/keymaps/fastedit.toml`, context
  `editor && vim.mode == normal && !vim.pending`, the same as `]d`. The
  keymap matcher holds space as a chord prefix. Space then any other key
  comes back `Unmatched` with both keys, which go to vim as before. The
  which-key popup lists `r` after space, from `Keymap::continuations`,
  merged with any vim `<leader>` maps when the leader is space (LazyVim's
  preset maps no `<leader>r`).

## Review round 1 (z9b6ne)

- **F1, keys out of the panel.** The panel's root carries the key context
  `runs_panel`, with the `vim` flag while vim is on, as Problems' does. It
  joins the fifteen dock-leave bindings in `fastedit.toml`: `escape`,
  `<C-w>h/j/k/l/p/w` (with ctrl on the second key too) and
  `<C-h>/<C-j>/<C-k>/<C-l>`. The context sits on the panel's root, so it
  holds over the list and over the empty state, which tracks the same focus
  handle. Tests: `runs_bindings::dock_keys_leave_the_runs_panel` (the
  keymap, vim on and off) and
  `tests::runs_panel::vim_keys_leave_the_panel_empty_or_not` (the
  workbench, back to the editor from both states).
- **F2, a spinner that spins.** A running row's `LoaderCircle` turns one
  revolution a second (`spin_angle`). The angle counts from the list's own
  epoch, so all the spinners turn together. Each time a spinner draws, it
  asks for the next frame (`Window::request_animation_frame`, which
  notifies the list alone). It turns while drawn, whatever the row's age.
  It stops once the run ends, as the row draws no spinner, and once the
  panel hides, as nothing draws. With reduced motion (`App::reduce_motion`,
  which `workbench.reduce_motion` sets), it stands still and asks for no
  frames. Test:
  `tests::runs_panel::the_spinner_turns_while_drawn_and_stops_when_done_or_hidden`.
- **F3, no reveal on a background update.** kiln-ui's `List` gains
  `keep_selected`, which moves the selection without scrolling or emitting.
  The panel calls it only when a row came above the selected run. A
  progress update or a finish keeps the index and calls nothing. Test:
  `tests::runs_panel::a_background_update_never_scrolls_to_the_selection`.
  It scrolls ten rows away from the selected row, then updates one run,
  finishes another and adds a new one.

## Scope added by respec

The panel couldn't be wired from the listed files alone. It needed:
`workbench.rs` (the bottom dock's registration, the panel's field and its
focus-out hook), `workbench_chrome.rs` (foot-button highlight, focusing,
`run_dock_op` and restore), `task_run.rs` (run → task tab), and
`examples/screenshots/audit_scenes.rs` (the scene's `mod` and `SURFACES`
lines), and `kiln/crates/kiln-ui/src/list.rs` for review round 1's F3.
Six tests pinned the rail foot's three buttons and their adjacency:
`chat_rail_tests.rs`, `git_panel_tests.rs`, `tests/activity_rail.rs`,
`tests/rail_b3.rs`, `tests/references_panel.rs` and
`tests/bottom_panel_b3.rs`. They now expect Runs between Problems and the
Git log.

## Audit scene (C4)

`examples/screenshots/audit/runs.rs` makes five runs through the
workbench's public API (a test run at 60%, a release build with unknown
progress, a cancelled script, a failed test run and a passed build), then
runs `runs.show`. Its probes check the accent line at the first row's foot,
the row's ground past the fill, and the panel below the rows.

Recorded on the Mac, 2026-11-14, at commit 3b0eee4d6 plus this file:
`cargo run -p fe-workbench --example screenshots -- audit target/ui-audit`
wrote `runs_dark.png` and `runs_light.png` (Tenmoku Dark and Light) and
printed `ok runs_dark probes (3)` and `ok runs_light probes (3)`. Both show
the five rows newest first, `cargo test` with its accent line to three
fifths, `cargo build --release` with the faint whole-width line, the
dash, ✕ and ✓ in their colours, mono titles, muted summaries and `now`.

The same run failed 22 probes in other scenes: chrome, settings,
shortcuts_editor, changes, git_log, git_log_card, problems_panel,
problems, terminal_session and terminal. None is at the rail, the one
thing this card changes in them. They look like geometry drift on main:
the terminal scene's probes, for one, assume the bottom panel starts at
x = 290, and it starts at 282. They are not this card's to fix.

`UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots`
regenerated `golden/workbench.png` for the rail's new Runs button. The
golden on main was already stale: it lacked the Project header's `+`
(New File), which the new one also picks up. The run rewrote the
setup-wizard and file-icon goldens too; this card can't change those
scenes, so they were left as they were on main.
