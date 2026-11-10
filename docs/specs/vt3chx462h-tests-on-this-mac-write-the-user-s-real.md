---
task_uid: "01m711jmw0ysnbzmvt3chx462h"
task: "T1664"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Running the workspace tests under nextest leaves $XDG_STATE_HOME/tenmoku (recent.toml, layouts, backups, history) byte-for-byte unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/e2e-macos.sh launches the app with a temp TENMOKU_STATE_DIR"
    required: true
    manual: false
  - id: "C3"
    text: "New test crates/fe-config/tests/test_state_isolation.rs fails when a test process's state dir resolves to the user's real state dir"
    required: true
    manual: false
---

# Tests on this Mac write the user's real Tenmoku state dir (recent.toml holds only test temp dirs), so the app forgets the user's projects: isolate the state dir for every test run and the e2e script

# Tests on this Mac write the user's real Tenmoku state dir, so the app forgets the user's projects

## Evidence (user report 2026-11-09: "it doesn't reopen the project that I had previously")

`~/.local/state/tenmoku/recent.toml` (modified 15:51 local, while finish gates ran) lists ONLY test temp dirs:
20 folders like `/var/folders/1q/.../T/.tmpQ2Kaqf` and `/private/var/folders/.../.tmp9eJJkx`, and files like
`.../.tmpQ2Kaqf/a.txt` and `.../.tmpcW8Pki/tenmoku/settings.toml`. None of the user's real projects is left: the
tests' entries evicted them. `layouts/` holds 39 files; the newest, `cbf29ce484222325.json` (the rootless key,
13:27), has `/Users/daryl/code/fastedit/Cargo.lock` plus an untitled backup, which matches the T1661 XCUITest
spike's launch of the bundled app around then, not the user's own session.

So some tests (and possibly scripts/e2e-macos.sh) resolve `fe_config::paths::state_dir()` to the real
`$XDG_STATE_HOME/tenmoku` (XDG_STATE_HOME=/Users/daryl/.local/state is set in the user's shell, and every lane and
gate inherits it) instead of a temp dir.

## Fix

1. Find every test that writes recents, layouts, backups or history through the real state dir (start with
   crates/fe-workbench/tests/start_page_selection_and_recents.rs and anything writing `a.txt` into a tempdir and then
   opening it through the workbench's recents).
2. Isolate centrally rather than test by test: give every nextest run its own `TENMOKU_STATE_DIR` (and
   `TENMOKU_CONFIG_DIR`/cache equivalents if they exist in crates/fe-config/src/paths.rs), for example with a
   nextest setup script in .config/nextest.toml that exports temp dirs. Make scripts/e2e-macos.sh launch the app
   with its own temp TENMOKU_STATE_DIR too.
3. Add a guard test (new file) that fails if a test process's state dir resolves under the user's real home state.
Do not touch the user's existing ~/.local/state/tenmoku files.
