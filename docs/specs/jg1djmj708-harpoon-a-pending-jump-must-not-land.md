---
task_uid: "01m6s6wek9xsw59njg1djmj708"
task: "T1422"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "an_open_while_a_jump_is_pending_beats_the_jump passes: a file opened, or a tab left and returned to, while a harpoon jump is pending is not overridden by the jump"
    required: true
    manual: false
  - id: "C2"
    text: "a_dismissed_list_picker_does_not_open_its_row passes: after Escape or toggling the list off following Enter, no tab opens, and after Escape focus does not move (the toggle-off focus check is follow-up T1488)"
    required: true
    manual: false
  - id: "C3"
    text: "the existing harpoon tests (only_the_latest_jump_lands_and_only_if_nothing_moved, the_list_picker_checks_the_disk_on_enter) still pass"
    required: true
    manual: false
---

# Harpoon: a pending jump must not land over a newer open or navigation, and a dismissed list picker must not open its row

Harpoon's open guard lets a stale open land (two findings carried from the review of 5ef6043e, Mac T1397 / T474).

Defects, both in crates/fe-workbench/src/harpoon.rs:

1. `Asked::holds` (~line 461) compares only the root, the active pane id and the active tab id at ask time against now, plus harpoon's own per-window `Opens` counter. So while a harpoon jump (`open_mark`, ~line 298) is pending: (a) opening a file that isn't open yet (its tab doesn't exist until the read lands, so the active tab is unchanged) and (b) switching tab away and back (A -> B -> A, same ids) both leave `holds` true, and the older jump then lands over the newer open or navigation.

2. `HarpoonPicker::confirm` (~line 761) starts a workbench-owned `open_mark` that nothing cancels: `dismissed` (~line 797) is a no-op. Escape, toggling the list off (`harpoon_toggle_list` -> `dismiss_modal`), or another modal replacing the picker while the read is in flight does not stop it; with pane and tab unchanged the dismissed picker's result still opens the file and takes focus.

Fix (reuse the existing pattern; do not invent a new counter family):
- config_problems.rs (`ConfigProblems::open`, ~line 170) already supersedes a pending open by capturing `(pane.open_serial(), pane.navigations())` at ask time and comparing both at landing. Make `Asked` capture the same pair and have `holds` compare them, in addition to the existing checks. Harpoon's own landing bumps them only after `holds` has passed, so two jumps still resolve by the `Opens` counter as today.
- For the picker: a dismissal (Escape, toggle off, modal replaced or picker released; find whichever hook actually fires on all three, `dismissed` if it does) bumps the window's `Opens` entry so a pending open from that picker no longer holds. A successful open that then emits DismissEvent must still land first (bump only after, or only for dismissals not caused by a landed open).
- Missing-file and empty-slot toasts of a superseded open stay silent as today (holds is false, returns None).

Tests: a NEW integration test file crates/fe-workbench/tests/harpoon_supersede.rs (the tests dir is flat; there is no tests mod.rs, so a top-level file is its own binary, as the existing ones are). Src-declared test modules (harpoon_tests.rs) are absent at base, so the regression tests cannot live there. Use only APIs public at base, in the style of tests/hot_exit_rootless.rs: fe_gpui::init + fe_workbench::init, Launch { root, file }, Workbench::new, commands by RunCommand(Invocation::of("workbench.harpoon_jump2")...). `harpoon::set_state_dir` is cfg(test) and invisible there, so point the marks at a tempdir with the TENMOKU_STATE_DIR env var (fe-config paths::state_dir; set under a serial lock before gpui starts, as hot_exit_rootless's `serially` does) and write the list with fe_config::harpoon::MarkList::save(&marks_file(state, root)).
Tests (both must fail at base):
- an_open_while_a_jump_is_pending_beats_the_jump: marks a,b,c; tabs a and b open, a active; dispatch harpoon_jump2 (not drawn), then open c (not yet open) through the pane before the read lands, or switch to b and back to a; after parking, the active tab is the newer choice (c, or a), not b.
- a_dismissed_list_picker_does_not_open_its_row: toggle the list (workbench.harpoon_toggle_list), Enter on a row, then Escape (and, in a second case, toggle off) before parking; after parking no tab for that file exists and focus is unchanged. Keep both cases in one test or two small ones; no soak runs.
Keep the existing harpoon_tests.rs tests passing (only_the_latest_jump_lands_and_only_if_nothing_moved, the_list_picker_checks_the_disk_on_enter).

Scope: crates/fe-workbench/src/harpoon.rs, crates/fe-workbench/tests/harpoon_supersede.rs (and src/harpoon_tests.rs only if an existing test needs adjusting; pane.rs accessors are already pub(crate), no change expected).
