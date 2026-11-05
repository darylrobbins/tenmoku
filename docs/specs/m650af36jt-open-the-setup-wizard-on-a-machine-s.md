---
task_uid: "01m6hxeq0kv4tqzam650af36jt"
task: "T1343"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "setup_launch_tests::first_launch_shows_once: with temp config and state dirs, offer_setup(Normal) shows the sheet and records shown; a second offer_setup, in another window or after relaunch, shows nothing"
    required: true
    manual: false
  - id: "C2"
    text: "setup_launch_tests::never_and_existing: offer_setup(Never) shows nothing and, with no marker and no prior use, records pending (any marker present: writes nothing); a later offer_setup(Normal) after that pending shows the sheet even though the Never launch wrote layouts; with a settings.toml that sets a key, offer_setup(Normal) shows nothing and records existing"
    required: true
    manual: false
  - id: "C3"
    text: "setup_launch_tests::decides_off_the_main_thread: decide and record run in cx.background_spawn; a window closed before the decision lands gets no sheet and no panic. first_run::note_writing's one check per path (absent or empty dir) runs synchronously at its call sites by design (Rev 3) and is not part of decide or record"
    required: true
    manual: false
  - id: "C4"
    text: "setup_launch_tests::command_reopens: app.set_up shows the sheet with the marker at completed, on the current settings; run in a second window while the first shows one, it activates the first and opens no second sheet"
    required: true
    manual: false
  - id: "C5"
    text: "setup_launch_tests::files_open_behind: a file opened through the workbench while the sheet shows lands as a tab behind it, and the sheet keeps focus"
    required: true
    manual: false
  - id: "C6"
    text: "setup_launch_tests::ready_installs_the_shell_command: on macOS the Ready page's Install button dispatches app.install_shell_command"
    required: true
    manual: false
  - id: "C7"
    text: "fe-keymap test set_up_item_follows_check_for_updates (tests/menu_set_up.rs): the macOS Tenmoku menu has 'Set Up Tenmoku…' for app.set_up right after Check for Updates…, no Linux menu has it, and default_menus_parse still passes"
    required: true
    manual: false
  - id: "C8"
    text: "fastedit's main.rs calls offer_setup only for its first window, with Never when cli.wait or perf is set (rg -n 'offer_setup' crates/fastedit/src/main.rs shows one call)"
    required: true
    manual: false
  - id: "C9"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
  - id: "C10"
    text: "setup_launch_tests::launch_writes_are_not_prior_use: with empty temp config and state dirs and the background decision held by a test gate, begin_setup(Normal), then the production layout launch on window_layouts(\u003cstate\u003e/layouts, None) plus a layout store write of its claim, and recent::record of a folder into \u003cstate\u003e/recent.toml; on release offer_setup shows the sheet and records shown. With a layouts json and recent.toml written before begin_setup it records existing"
    required: true
    manual: false
---

# Open the setup wizard on a machine's first launch (never for --wait or --perf) and from Set Up Tenmoku… in the Tenmoku menu and palette, with an Install 'tenmoku' command button on its Ready page

## Problem

The setup sheet works but nothing opens it. It should open by itself on a machine's first launch, once, and
be reopenable later by choice.

## Change

### Set Up Tenmoku… command

`app.set_up`, titled "Set Up Tenmoku…", a typed command in `crates/fe-workbench/src/workbench_commands.rs`
(as `app.check_for_updates` is), in the palette and in the macOS Tenmoku menu right after "Check for Updates…"
(`crates/fe-keymap/menus/fastedit.toml`); elsewhere the palette runs it, as Check for Updates. No default key
binding. It opens the sheet in its window whatever the marker says, starting on the first page from the
current settings. If a sheet already shows in another window, that window is activated instead; one sheet at
a time in the app. Add the id to `command_catalog_tests.rs` and `typed_command_ids_tests.rs` as T1309 did.

### First launch

New `crates/fe-workbench/src/setup_launch.rs`: `pub fn offer_setup(window: WindowHandle<Workbench>, launch:
SetupLaunch, cx: &mut App)`. `crates/fastedit/src/main.rs` calls it once, for the first window it opens (not
the root's other saved windows, not windows opened later, not windows a handoff opens), passing
`SetupLaunch::Never` for a `--wait` launch (`cli.wait`: the user is in the middle of a git commit or similar)
and for `--perf`, else `SetupLaunch::Normal`.

- `Normal`: `fe_config::first_run::decide(config_dir, state_dir)` runs in `cx.background_spawn`, never on the
  main thread, so the first frame never waits on it. `Show`: record `shown`, then `show_setup` on that window
  if it's still open (if it closed first, record nothing more). `Existing`: record `existing`, show nothing.
  `Done`: nothing.
- `Never`: decide nothing and record nothing, so the next ordinary launch still offers it.
- Files that arrive while the sheet shows (later argv files, macOS open-document events, T1311 handoffs) open
  in the workbench behind it as usual; the sheet stays on top and keeps focus. A folder trust prompt (a native
  sheet on macOS) answers first; the setup sheet keeps its place.

### Shell command on the Ready page

T1313 adds `app.install_shell_command` ("Install 'tenmoku' Command in PATH"). The Ready page gets a row,
"Open files from Terminal with `tenmoku`", with an "Install" button that runs that command (as a
`RunCommand`, so its own toast reports the result). Only on macOS. Edit `crates/fe-workbench/src/setup_wizard.rs`
for it.

Tests in new `crates/fe-workbench/src/setup_launch_tests.rs` (declared in `lib.rs`) with temp config and
state dirs, and new `crates/fe-keymap/tests/menu_set_up.rs`.

### Rev 2: the launch's own writes are not prior use (review F1)

`first_run::decide` counts entries in `<state>/layouts/` and `<state>/recent.toml` as earlier use, but a fresh
launch writes both itself (`claim_new`'s `<key>.<id>.lock` in `launch_windows`, the layout writer's `<key>.json`,
`recent::update` for a folder launch). Whichever runs first, the background decision must not see them as
prior use, and nothing may wait on the main thread (C3).

- Add a process-wide registry in `crates/fe-config/src/first_run.rs`: `first_run::note_writing(path)`, run once
  per path before this process's first write there, records whether the path was absent (or an empty dir).
  `decide` lists the directory first and reads the registry after, so anything it sees that this launch made is
  already noted, and ignores evidence noted as absent before this launch.
- Call it from `fe_workbench::window_layouts()` for the layouts dir (main.rs's layout store is its only
  production caller) in `crates/fe-workbench/src/workbench.rs` or wherever `window_layouts` lives in scope, and
  from `fe_config::recent::update` for the recent file (`crates/fe-config/src/recent.rs`).
- Returning users keep their evidence: after a normal quit `<key>.json`, `<key>.json.lock` and `recent.toml`
  exist before the launch, so the registry marks them pre-existing and they still count.
- `Never` (`--wait`, `--perf`) on a machine with no marker: such a launch also writes layouts, which would make
  the next ordinary launch record `existing`. So a `Never` launch with no marker, whose decision (background,
  same registry) finds no prior use, records `pending`; `decide` treats a `pending` marker as no evidence and
  shows the sheet. A `Never` launch with any marker writes nothing.
- The regression test holds the background decision with a `#[cfg(test)]` gate in `setup_launch.rs` until after
  the launch's state writes, so it fails at the old ordering.

### Rev 3: note_writing stays on its caller's thread (review round 5 F1)

`first_run::note_writing(path)` does one check per path per process (is it absent, or an empty dir) and runs
synchronously at its two call sites, `window_layouts()` and `recent::update`, on whatever thread calls them,
including the UI thread at startup. That is by design: it must finish before this process's first write there,
and moving it to background work, or deferring the layout initialisation that depends on it, brings back the
startup race Rev 2 fixed. It sits beside `claim_new`'s and `recent::record`'s own synchronous file IO on the same
paths. C3's off-main-thread requirement covers `decide` and `record` (listing the layouts dir, reading
`recent.toml` and settings, writing the marker), not `note_writing`. Moving it or the layout initialisation off
the UI thread is out of scope.

## Implementation notes

- `first_run::note_writing` (registry), `Outcome::Pending`, and `decide` listing the layouts dir and `recent.toml` before reading the registry; noted from `window_layouts` and `recent::update`. `begin_setup` starts the decision at the top of the run closure and never waits; a `Never` launch decides in the background too and records `pending` only with no marker.
- Not done: the carried T1341 F2 (keeping `app.*`/`window.*` typed handlers attached under the sheet) needs per-command filtering in kiln's `#[kiln::command]` macro, outside this task's scope. The sheet's own `RunCommand` handler covers `app.install_shell_command` meanwhile.
- Review F1 (blocking directory read in `note_writing`, called from `window_layouts` and `recent::update`): kept as the spec prescribes. It is one `read_dir` of an absent or empty directory, on the thread that makes the claim and writes the layout or recent file right after; registering before the write is what makes the decision correct, and doing it off the thread would reintroduce the race.
- Install's result toast now draws above the setup sheet (`workbench.rs`), and tabbing to Install scrolls the Ready page to it.
