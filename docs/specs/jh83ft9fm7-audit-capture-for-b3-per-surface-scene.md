---
task_uid: "01m6vvs6762zswz1jh83ft9fm7"
task: "T1519"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "On the Mac, cargo run -p fe-workbench --example screenshots -- audit target/ui-audit writes every scene the T1429 audit wrote plus problems_rows, terminal_session and git_log_card, each _dark (Yohen) and _light (Hakuji), with no real settings, keymap or recent.toml read or written"
    required: true
    manual: false
  - id: "C2"
    text: "The token probe runs over the audit scenes and passes; an injected wrong colour at a probe point fails it (shown in the decision file), with SCREENSHOTS_ALLOW_OTHER_SCALE set too"
    required: true
    manual: false
  - id: "C3"
    text: "The golden run no longer writes the user's recent.toml (a test or a check in main.rs that it refuses to run outside its temp dirs); cargo run -p fe-workbench --example screenshots still matches its goldens"
    required: true
    manual: false
---

# Audit capture for B3: per-surface scene files, hooks for Problems rows, a fake terminal session and the Git log card, token probes over the audit scenes

# Audit capture for B3: per-surface scenes, the left-out hooks, and probes

Reuses the audit capture of `docs/decisions/25w38k498b-ui-polish-audit.md`
(`cargo run -p fe-workbench --example screenshots -- audit target/ui-audit`)
and folds in the held audit cards P15 (hooks for the left-out surfaces) and P26
(token probe over the audit scenes). The surface cards use it for their
screenshot criteria.

## Change

- Split `crates/fe-workbench/examples/screenshots/audit_scenes.rs` into
  `crates/fe-workbench/examples/screenshots/audit/mod.rs` (the runner, the
  `SURFACES` list, isolation) and one file per surface
  (`audit/chrome.rs`, `audit/problems.rs`, …), so each surface card adds one
  file and one line in `mod.rs`. Dark and light are now Yohen and Hakuji.
- Hooks (each `#[doc(hidden)]`), so these surfaces can be captured:
  - `Workbench::set_diagnostics_for_audit(path, diagnostics)`
    (`crates/fe-workbench/src/problems_panel.rs`): Problems with rows;
  - `TerminalPanel::open_with_session(...)` fed fixed bytes, including two
    finished commands (exit 0 and exit 1) and one running
    (`crates/fe-workbench/src/terminal_panel.rs`);
  - `Workbench::git_log_panel()` and `GitLogPanel::show_card(ix, ...)`
    (`crates/fe-workbench/src/git_log.rs`).
- The golden run's scenes isolated from the user's state as the audit is
  (`scenes.rs` and `main.rs`: they write the real `recent.toml` today).
- `token_probe.rs` probes the audit scenes too: for each scene, a list of
  (point, token) pairs and a check that the pixel equals the token's colour in
  that theme. Each surface card adds its probes in its own scene file.
- New scenes: `problems_rows`, `terminal_session`, `git_log_card`.

Platform: **Mac-only** (Metal captures).

## Implementation notes

- The runner stays in `examples/screenshots/audit_scenes.rs` (the scope names it, not
  `audit/mod.rs`), declaring the surface files with `#[path = "audit/<name>.rs"]`. Every
  surface has a file of its own in `audit/`, T1429's 15 (the trust prompt included) and the
  three new ones; a surface card adds its file, its `mod` line and its `SURFACES` entry. The
  14 files beyond the scope's four are that split (review round 1, F3).
- Every surface has probes, and the run refuses one without; a step that can't do its part
  fails the run.
- Isolation moved to `main.rs` and covers every mode, the golden run included; the child
  refuses to start outside its temporary directories.
- Evidence, the hooks and the probe output (passing, and failing on an injected colour, a
  surface without probes and a terminal that never writes, all with
  `SCREENSHOTS_ALLOW_OTHER_SCALE` set) are in `docs/decisions/jh83ft9fm7-audit-capture-b3.md`.
