---
task_uid: "01m7vb50v4xkkc0tw7d151bhk3"
task: "T705"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "In new crates/fe-workbench/tests/coverage_report_watch.rs (cfg(unix), tempdir canonicalized for /var -\u003e /private/var), default_discovery_drops_a_report_when_a_symlinked_folders_target_is_removed_or_replaced: a report folder that is a symlink to a target folder loads its report (first check an assert_eq! on report_path()); removing the target, sending FsChange::Path(\u003ccanonical target\u003e) and polling until !is_loading() leaves no report (assert_eq! None); replacing it with a different target that holds a different report loads the new one."
    required: true
    manual: false
  - id: "C2"
    text: "In the same file, glob_discovery_drops_a_report_when_a_wildcard_folder_is_removed_or_replaced: with coverage.report = runs/*/lcov.info, removing a matched folder drops its report, and separately replacing it with a new folder holding a newer report loads that one, by the same event and assert_eq! pattern; the symlinked wildcard-folder case is cfg(unix)."
    required: true
    manual: false
  - id: "C3"
    text: "The new file compiles against base APIs only and is the only new test file; existing coverage_tests.rs passes unedited; cargo clippy -p fe-workbench --all-targets is clean with -D warnings; public API unchanged."
    required: true
    manual: false
---

# Watch a symlinked report folder's target and the folders a coverage.report glob reaches, so a removed or replaced report is dropped (review follow-up of T1982 F7) (Mac T2027)

Review F7 (medium, carried at T1982 approval), crates/fe-workbench/src/coverage.rs, CoverageSource::fs_changed and watch_places (~line 1650-1700). watch_places adds a symlinked folder's canonical target only if it canonicalizes at that moment, and never resolves folders a wildcard reaches (runs/*/lcov.info). A removal or rename of a symlinked folder's target, or of a wildcard-matched folder, is then not could_be_report, so no check or rediscovery is owed. The old report stays loaded under a live watcher, because set_watching(true) stops the timed polls.

Fix: match canonical target directories across removal events (remember each searched folder's last canonical target so a path equal to or under it still counts after it is gone), and treat directories the glob matched, with their canonical targets, as watch places that owe a rediscovery. Public API unchanged.

Test notes: CoverageSource, fs_changed, poll and report_path() are pub; wait_for_load is pub(crate), so loop poll until !is_loading(). In the regression tests, make the first check that fails at base an assert_eq!/assert_ne!.

## Helium filing note

- Lent from the Mac as y152: the review follow-up of helium T700 F7 (Mac T1982). The fix it builds on, helium T704 (Mac T1982 gate), has landed on main.
- **Never touch crates/fe-workbench/src/lsp_consent*.rs** (Mac T2024 and T1701 own it).
- linux-ok: headless tests. The macOS gate runs when the batch lands on the Mac, so canonicalize the tempdir as C1 says.
- The regression check applies the new tests/coverage_report_watch.rs to the base and runs it there. It must compile against base APIs only (C3). If a test needs an API the fix adds, ask the lead before the first finish.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if anything in the criteria can't be met as written.
