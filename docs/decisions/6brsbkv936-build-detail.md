# T1580: Build detail, the crate waterfall, Timeline or Output

Spec: `docs/specs/6brsbkv936-build-detail-the-crate-waterfall-fresh.md`;
design: `docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`
("a crate waterfall instead of a wall of log: fresh crates as thin lines,
rebuilt ones as bars, the crate you edited in rust; messages beside it;
Timeline or Output").

## What changed

- **`crates/fe-workbench/src/build_timeline_view.rs`** (new): the detail of
  the build run selected in the Runs list, hosted by the Runs panel as the
  test run's detail is (beside the list from 600 px, else under it). A
  title row (title in code small, summary muted) with a Timeline | Output
  `SegmentedControl` (T1518's, small).
  - **Timeline.** An 18 px axis, then one 18 px row per crate in start order
    (a kiln-ui `List`: arrows, Enter, screen-reader options). Each row has
    the name in code small (120 px column), the track, the warning and error
    counts (56 px column, only when the build has messages: ▲ `Triangle` in
    `status.warning`, ✕ `X` in `status.error`, each with its number), and
    the duration right-aligned (48 px; `412ms`, `2.3s`). On the track a
    fresh crate is a 2 px wide, 8 px tall line at the time cargo reported
    it; a rebuilt one is an 8 px bar from start to end, at least 4 px wide
    so a quick rebuild never reads as fresh. Marks stay on the track. Local
    crates are `accent.base`; the rest are `text.muted` at 50%.
  - **`layout(timeline, elapsed, width)`** is the pure function behind it.
    The track stands for the build's length: the last crate's end, or the
    time it's been running if that's later (1 s if both are zero). It gives
    each crate's mark and tone, its duration, the axis's step and its ticks.
    The step is 1, 5, 10 or 30 s: the smallest that puts no more than 10
    steps in the length (`tick_step`). On a narrow track only every k-th
    tick is labelled, so labels are at least 32 px apart. The axis shows the
    build's length over the durations column. The track's width is measured
    by a canvas in the axis, which has the rows' columns.
  - **Counts and Problems.** `build_runner::message_crate` gives a message
    its crate: the package cargo named, else the local crate whose folder
    holds its file (the innermost one; a relative file is tried in each
    manifest folder). A click on a crate's row, or Enter, shows the Problems
    panel with the first problem it lists of that crate selected
    (`Workbench::show_crate_problems`). The panel switches to Project and
    turns that severity's filter on if they hide it
    (`ProblemsPanel::select_source_problem`). A crate with no listed
    problems does nothing beyond the list's selection.
  - **Output.** The build's stderr (cargo's `Compiling` lines and the
    diagnostics it rendered, a line read in parts made whole) in code small,
    in a `uniform_list` that scrolls. It's read again only while it shows.
  - **Live.** A running build's view observes its `RunProcess` and reads the
    build again each batch. It also redraws every 250 ms, so the crates
    being compiled grow to the time it's been running and the scale
    shrinks with them.
- **`crates/fe-workbench/src/build_runner.rs`.** After each batch with new
  lines the sink stores `CargoParser::snapshot()`, the build so far
  (`Workbench::build_shown` reads the final timeline, else that). Also
  `build_output`, `show_crate_problems`, `message_crate` and
  `record_build_run`: a finished build with a given timeline and output for
  the screenshots, its problems listed as a real build's are. A build's
  problems remember which message each one is. `Plan::crate_dirs` adds each
  manifest folder as the file system resolves it, computed in `plan` off
  the main thread: cargo gives crate folders resolved (`/private/var/…` for
  a `/var/…` temp root on macOS).
- **`crates/fe-runlog/src/cargo.rs`.** `CrateSpan` gains `dir` (a local
  crate's folder, from its `path+file://` id or its `Compiling` line) and
  `running` (only in a snapshot: `Compiling` seen, no artifact yet).
  `CargoParser::snapshot(&self)` is `finish` without consuming the parser.
  A crate whose `Compiling` line names a folder is local even if it never
  got an artifact. Before this, a local crate that failed to compile read
  as a registry crate.
- **`runs_panel.rs`** shows the build detail for a selected Build run;
  `lib.rs`, `tests/mod.rs` and `audit_scenes.rs` declare the new modules.
- **Catalogue.** `l10n/en-US/build_runs.ftl` holds the switch, the segments,
  the list's name, tick labels (`5s`, `1m 30s`, `2m`), durations, counts,
  the empty states and each row's accessible name: "app, built in 4s,
  local, 2 warnings, 1 error", "itoa, fresh", "app, building". fe-workbench's
  `l10n-allow.toml` is `all = true` since W8, so the new files are checked
  with no list entry.

## Choices

- A fresh crate's "thin 2 px line" is a vertical mark 2 px wide. Its span is
  zero-length, so a horizontal line would have no length to draw.
- Live growth reads `Run::started` for the elapsed time, not the parser's
  clock. They differ by the program's start-up, well under a frame.
- A message without a package or a file (a linker error, say) belongs to no
  crate. It counts in the run's summary but beside no row.
- The snapshot clones the parser's state once per batch that brought lines
  (at most one each 16 ms). That's spans plus messages: small for real
  builds.

## Evidence

- C1: `cargo nextest run -p fe-workbench -E 'test(/build_timeline_view/)'`.
  Eight tests pass: the six named, plus `a_crate_row_selects_its_messages_in_problems`
  and `a_running_build_grows_its_bars`. The latter runs a gated fake
  `cargo`, sees `app` running, then sees its bar reach the track's end as
  time passes. The `build_runner`, `runs_panel`, `test_run_view` and
  `problems` tests still pass (200 tests). fe-runlog's `cargo_json` adds
  `a_snapshot_marks_the_crates_being_compiled` and
  `a_local_crate_has_its_folder`.
- C2: `cargo nextest run -p fe-workbench --test l10n`: 3 passed.
- C3, on the Mac (2026-11-17): `cargo run -p fe-workbench --example
  screenshots -- audit <dir>` wrote 83 captures, all probes on, including
  `ok runs_build_dark probes (4)` and `ok runs_build_light probes (4)`. The
  scene (`audit/runs_build.rs`) is a recorded build of the demo project:
  four fresh registry crates, three rebuilt, and `demo` last in the accent
  with two warnings. Its probes: `demo`'s bar is `accent.base`, its track
  before the bar is `surface.panel`, the chosen Timeline segment is
  `state.selected_inactive`, and the panel below the rows is
  `surface.panel`. Checked by eye in both themes too: ticks every second,
  labelled every 2 s on the 158 px track; the 9.8s length; `▲ 2` beside
  `demo`; and the status bar counting the recorded build's two warnings.
