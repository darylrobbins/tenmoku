---
task_uid: "01m7c5fjfwk2wq7keqqnx9nky7"
task: "T1852"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Test tests::debug_panel::expanding_a_variable_fetches_its_children_once_per_stop passes (cargo nextest run -p fe-workbench debug_panel), with children beyond 100 paged"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::debug_panel::selecting_frame_2_opens_its_file_and_bands_the_row_with_the_frame_tint passes, re-scoping Variables and Watch; debug.frame_up and debug.frame_down move the selection"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_panel::watch_expressions_reevaluate_on_each_stop_and_survive_a_restart passes, kept in D5's watches list"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::debug_panel::toggling_an_exception_filter_resends_set_exception_breakpoints passes"
    required: true
    manual: false
  - id: "C5"
    text: "Test tests::debug_panel::the_rail_shows_debug_next_to_runs_after_the_first_session passes, and debug.show_panels toggles the tool window"
    required: true
    manual: false
  - id: "C6"
    text: "Test tests::debug_panel::rows_and_controls_have_accessible_names passes"
    required: true
    manual: false
  - id: "C7"
    text: "Every command this card adds has its rows in typed_command_ids_tests.rs and command_catalog_tests.rs, and cargo nextest run -p fe-workbench typed_command_ids command_catalog passes"
    required: true
    manual: false
  - id: "C8"
    text: "On the Mac, the audit scene crates/fe-workbench/examples/screenshots/audit/debug_panel.rs renders in Yohen and Hakuji and its probes pass (recorded in the decision file); the scene must not skip on SCREENSHOTS_ALLOW_OTHER_SCALE"
    required: true
    manual: false
  - id: "C9"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Debug tool window in the bottom panel: Variables, Watch, Call Stack and Breakpoints, with frame selection and exception filters (debugger D9)

# Debug tool window in the bottom panel: Variables, Watch, Call Stack and Breakpoints, with frame selection and exception filters (debugger D9)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **l**.

## Problem

While stopped the user needs variables, watches, the call stack and the
breakpoint list. §5 put them in a right-dock container; B3 has no right dock (§13).

## What to build (§5 "Panels"; §11 D9; §13 "B3 design first"; D0's section)

- **A Debug tool window in the bottom panel**, registered as Runs is (`dock.rs`;
  T1571's Runs panel lands first), with its icon in the rail's bottom group next to
  Runs, joining the rail on the first session as References and Problems do.
- Its views, laid out as D0 says: **Variables** (a lazily expanded tree per scope on
  `uniform_list` rows, paging beyond 100), **Watch** (expressions in D5's `watches`
  list, evaluated with context `watch` on each stop), **Call Stack** (threads and
  frames, subtle and source-less frames dimmed; selecting one opens it and bands the
  row through D6's seam), **Breakpoints** (grouped by file as Problems groups, plus
  the adapter's exception filters as checkboxes re-sent through
  `setExceptionBreakpoints`).
- Commands `debug.frame_up`, `debug.frame_down`, `debug.show_panels`.
- Starting a session shows the tool window once per session (§5 "Layout").

Reuse what `problems_panel.rs` already exposes for grouping and reveal but don't edit
it (Mac T1587 and T1807 are changing it); copy what you can't reach.

## Not in this card

The console (D10), hover (D11), set value, `sourceReference` sources (later).

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Mac** — acceptance includes a macOS look check of an audit scene (§5, §10), which review fails when made on Linux. The headless criteria can be built on Linux first.

## Implementation notes

- The design calls and limits are in `docs/decisions/eqqnx9nky7-debug-panel.md`.
- Scope respec (worker, before building): `workbench.rs` and
  `workbench_chrome.rs` register the tool window as Runs is; kiln-shell's
  `dock.rs` gains `DockState::add_action_after` so Debug's rail action sits
  after Runs'; `crates/fe-keymap/keymaps/fastedit.toml` and fe-gpui's three
  context pins add `debug_panel` to the dock-leave keys' context;
  `debug_session/**` for the frame and filter hooks.
- C1 is shown by two tests: `expanding_a_variable_fetches_its_children_once_per_stop`
  (once per stop, and 250 children shown 100 at a time from one fetch) and
  `children_past_100_are_asked_for_a_page_at_a_time` (an adapter counting
  `indexedVariables` asked `start`/`count` per page).
