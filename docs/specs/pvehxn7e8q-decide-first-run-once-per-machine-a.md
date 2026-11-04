---
task_uid: "01m6hxej2cw9nppypvehxn7e8q"
task: "T1340"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test fresh_dirs_show_setup (tests/first_run.rs): empty config and state directories give FirstRun::Show"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test template_settings_still_show: a settings.toml equal to USER_SETTINGS_TEMPLATE (every key commented out) gives Show"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test existing_users_are_not_asked: each alone of a settings.toml with one key, an unparsable settings.toml, keymap.toml, theme.toml, vimrc, \u003cstate\u003e/recent.toml or a non-empty \u003cstate\u003e/layouts gives Existing"
    required: true
    manual: false
  - id: "C4"
    text: "fe-config test marker_wins: a marker with any outcome, or unparsable, gives Done whatever else exists; no state directory gives Done"
    required: true
    manual: false
  - id: "C5"
    text: "fe-config test record_replaces_the_outcome: record(shown) then record(completed) leaves \u003cstate\u003e/setup.toml with outcome = \"completed\" and version = fe-config's CARGO_PKG_VERSION, creating a missing state directory, with no temp file left behind"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Decide first run once per machine: a <state_dir>/setup.toml marker, and existing users (settings, keymap, theme or vimrc file, recent list or saved layouts) never asked

## Problem

The setup wizard (first-run preferences, asked for 2026-11-03) must show once per machine, on first launch
only, and never again once it was shown, finished or skipped. People who already use Tenmoku must never get it:
T1258 moved config and state to `~/.config/tenmoku` and `~/.local/state/tenmoku` with no migration, so "no
marker yet" alone would ask every existing user once.

## Change

New `crates/fe-config/src/first_run.rs`, `pub mod first_run;` in `crates/fe-config/src/lib.rs`. No GPUI.

- The marker is `<state_dir>/setup.toml` (state, not settings: it records this machine, and settings may be
  synced between machines). Contents: `outcome = "shown" | "completed" | "skipped" | "existing"` and
  `version = "<CARGO_PKG_VERSION of fe-config>"`. Written atomically (a temp file in the same directory, then
  rename), creating the state directory if needed.
- `decide(config_dir: Option<&Path>, state_dir: Option<&Path>) -> FirstRun`, reading only, with
  `FirstRun::{Show, Existing, Done}`:
  - No state directory: `Done` (nothing could be recorded, so never ask).
  - The marker exists, whatever it holds, even unparsable: `Done` (an unparsable marker is logged with
    `log::warn!`, never re-asked).
  - Otherwise `Existing` if any of these is there: `<config>/settings.toml` that parses to a non-empty table
    (the template, every key commented out, counts as empty; a file that doesn't parse counts as existing),
    `<config>/keymap.toml`, `<config>/theme.toml`, `<config>/vimrc`, `<state>/recent.toml`, or a non-empty
    `<state>/layouts/`. Use the existing path constants (`SETTINGS_FILE`, `THEME_FILE`, `VIMRC_FILE`, kiln's
    keymap file name, `recent_path`'s file name, `AppPaths::layouts_dir`'s name), not new literals.
  - Else `Show`.
- `record(state_dir: &Path, outcome: Outcome) -> io::Result<()>` writes the marker; a later `record`
  replaces the outcome (shown, then completed or skipped).

Who calls them: the launch card shows the wizard on `Show` and records `shown` then; on `Existing` it records
`existing` and shows nothing; the sheet records `completed` or `skipped`. Nothing here decides about `--wait`
or `--perf` launches; the launch card does.

Tests in new `crates/fe-config/tests/first_run.rs` (integration tests, temp directories only; never the
user's real directories).
