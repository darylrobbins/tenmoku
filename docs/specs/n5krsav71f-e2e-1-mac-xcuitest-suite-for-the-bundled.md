---
task_uid: "01m71sdh6sp5q22gn5krsav71f"
task: "T1682"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "docs/decisions/36gk6dhf5d-xcuitest-spike.md's Status line reads 'Status: **approved** (2026-11-09, approved by the user)' with the Approver unchanged and the 'files no follow-up card' sentence updated to name E2E-1 and E2E-2; no other part of that record changes except a pointer to the suite's decision file"
    required: true
    manual: false
  - id: "C2"
    text: "tools/e2e-macos holds about ten tests covering every bullet of the record's Suite scope (save paths incl. Save As and encodings byte-for-byte; window-title dirty/active signal; quit-by-menu relaunch and hot exit with 0600 backups; wizard once-only marker, trust by menu and trust.toml, Open Recent and recent.toml; vim clipboard vs NSPasteboard per clipboard value; markdown preview buttons and pane toolbar enabled state); finish evidence lists each test name and the human check task it shortens (T251, T526, T1344, T197, T528, T361, T1008)"
    required: true
    manual: false
  - id: "C3"
    text: "scripts/e2e-macos.sh accepts --fixture \u003cname\u003e naming a folder under tools/e2e-macos/fixtures/, builds one scratch set per test from the fixture the test declares (and pre-seeded settings.toml where declared), keeps the runner sandboxed, and its --dry-run output shows the fixture step; shellcheck scripts/e2e-macos.sh exits 0; T1664's temp state-dir behaviour is preserved"
    required: true
    manual: false
  - id: "C4"
    text: "A waitForTree() helper waits on a known labelled button via waitForExistence; every assertion on a gpui node goes through it, and the only fixed delay left is the named editorSettle (rg for sleep/Thread.sleep/usleep in tools/e2e-macos/TenmokuE2E finds none besides it)"
    required: true
    manual: false
  - id: "C5"
    text: "F1 fixed: record_owned and cleanup use every live, identity-checked (pid + start time) recorded process as a discovery root, while tracking and in cleanup. A new stand-in under tools/e2e-macos/standins/ where an orphan starts a long-lived TERM-resistant child after its parent has exited is run with MODE=hang under --timeout, MODE=exit, SIGINT and SIGTERM, and on each path the orphan and its child are gone (pgrep finds none) and the exit status is 124, 1, 130 and 143 as the record's table; the record's four earlier stand-in runs give the same results as before; the stand-in outputs are quoted in the finish evidence (C1's manual note quotes the record's F1 item)"
    required: true
    manual: false
  - id: "C6"
    text: "docs/decisions/\u003clane\u003e-xcuitest-suite.md answers the three open questions: the Automation Mode prompt quoted word for word from a fresh enablement done by the user at the Mac (or stated as still unknown, with the card's finish then waiting on that human step: the agent runs no sudo and changes no Automation Mode or System Setting), how long an authorization carries between sessions and whether a run past that blocks, and whether the runner's entitlements can be widened (what it unlocks for T400/T526 external edits, or why not)"
    required: true
    manual: false
  - id: "C7"
    text: "A real run on the Mac, started only after the user's go-ahead: 'scripts/e2e-macos.sh --iterations 3' ends 'e2e-macos: passed' with every test passing in all 3 iterations (no retries), quoting per-test times and the ordering of any flake; the user's running Tenmoku and both installed copies are untouched (launch records name only the e2e bundle)"
    required: true
    manual: false
  - id: "C8"
    text: "Nothing outside the scope changes: the suite is not wired into nextest, the gate or .github/workflows (git diff --stat shows no such path), and no editor-text assertion is added (those wait for E2E-4)"
    required: true
    manual: false
---

# E2E-1: Mac XCUITest suite for the bundled Tenmoku.app, with complete cleanup of launched processes (F1)

# E2E-1: the Mac XCUITest suite, and complete cleanup of launched processes

Source: docs/decisions/36gk6dhf5d-xcuitest-spike.md (T1661), signed off by the user on 2026-11-09.
Read its *Suite scope*, *Setup and permissions*, *Reliability and run time* and *Follow-up cards*
sections first. This card builds what the record scopes as E2E-1. Mac only, run on demand with
`scripts/e2e-macos.sh` by a person at the Mac (or an agent after asking). It is never in the gate,
never in nextest, and adds nothing to CI (E2E-2 is the CI probe).

## Build

1. Grow `SmokeTests` into the suite scope in the record, about ten tests, each on its own scratch
   set and its own app launch. Cover what reaches the app through real OS input and comes back
   through the disk, the process, the window (title, dirty marker, frame, traffic lights), the native
   menus and panels, and kiln's labelled icon buttons:
   - save paths: cmd-S, File > Save, Save All, Save As... through the native panel, and saving
     encodings byte-for-byte (T251's disk half);
   - the window title as the dirty and active-tab signal after edits, undo-to-clean and close;
   - relaunch and hot exit: quit by menu, relaunch on the same state dir, the restored title and
     dirty marker, backups mode 0600 (T526's disk half);
   - the first-run wizard's once-only marker and the "Set Up Tenmoku..." menu item (T1344's disk
     half), trust by menu and `trust.toml` (T197), Open Recent and `recent.toml` (T528);
   - the vim clipboard against `NSPasteboard`, one launch per `clipboard` value from a pre-seeded
     `settings.toml` (T361, the one task the record retires fully);
   - already-labelled buttons: the markdown preview buttons appear only on markdown tabs (T1008),
     and the pane toolbar buttons' enabled state.
2. Fixtures: `scripts/e2e-macos.sh` makes a scratch set per test with a fixture the test declares;
   `--fixture` names a folder under `tools/e2e-macos/fixtures/`. The runner is sandboxed (it writes
   only inside its container), so fixtures and pre-seeded config come from the script, and every
   record (AX dumps, launch records, logs) leaves the runner as an XCTest attachment.
3. A `waitForTree()` helper that waits on a known labelled button (e.g. `Split Right`) with
   `waitForExistence`; every assertion on a gpui node goes through it, because the tree arrives a
   frame after the first query and when it arrives varies. The one named settle delay
   (`editorSettle`) stays for the buffer load; nothing else sleeps.
4. Editor-text assertions stay out: they wait for A11Y-5/A11Y-7 (E2E-4). Pixel checks stay with the
   screenshot goldens. Do not change any System Setting, and do not touch the installed
   `/Applications` or `~/Applications` copies or the user's running Tenmoku (the bundle keeps the
   `dev.tenmoku.tenmoku.e2e` identity and `TENMOKU_NO_IPC=1`, `HOME` and the state/config dirs point at
   the scratch set).

## F1: complete descendant and watchdog cleanup (open item in the record)

`record_owned` walks down from the current roots only (xcodebuild's process group plus the processes
matching the runner's and the app's paths), never from processes already recorded. A recorded helper
orphaned when its parent exits, which then starts a child of its own, leaves that child running on
every exit path. Fix: use every live, identity-checked (pid plus start time) recorded process as a
discovery root, both while tracking and in cleanup. Add a stand-in (kept in the repo under
`tools/e2e-macos/standins/`, a stand-in `xcodebuild` selected with `E2E_XCODEBUILD`) where an orphan
starts a long-lived, TERM-resistant child after its parent has exited, and check that both go on each
exit path (normal end, `--timeout` watchdog, SIGINT, SIGTERM). The record's earlier stand-ins
(family, hang-ignore-term, exit-zero, short-run) still pass.

## Open questions the record assigns to this card

- Capture the Automation Mode authorization prompt word for word on a fresh enablement (run 1's
  wording was never captured; the user decided on 2026-11-09 to leave it as an open item).
  A person at the Mac does this: the agent may not run `sudo` or change Automation Mode, so
  ask the user and block that part if they are away. Do not claim it unless the text is quoted.
- How long an authorization carries over between sessions (minutes, the login session, until
  reboot), and whether a run started past that point blocks. Note that
  `sudo automationmodetool enable-automationmode-without-authentication` removes the prompt and is the
  user's step, not the agent's.
- Can the runner's entitlements be widened (e.g. a read-write exception for the scratch root)? If so,
  say what T400 and T526 external-edit checks it unlocks; widen only if it works without changing
  the machine, otherwise record why not.

Record the answers in a decision file `docs/decisions/<lane>-xcuitest-suite.md`.

## Scope notes

Depends on T1664, which also edits `scripts/e2e-macos.sh` (it makes the script launch the app with a
temp state dir); this card rebases onto that change and keeps its temp-dir behaviour.
Uses a human-run UI session: a UI run needs the user's go-ahead before it starts, as in the spike.

## Implementation notes

- Done without a UI session (2026-11-10): the spike record's Status (C1); 13 tests in
  `SmokeTests` over `Support.swift` helpers, each declaring its fixture in
  `Fixtures.swift` (C2, C4: `waitForTree()` on `Split Right`, `editorSettle` the only
  fixed delay); fixture sets per test and iteration, `--fixture`, declarations checked by
  the script (C3, shellcheck clean); F1 fixed (recorded live processes are roots) with
  the `late-orphan` stand-in, all stand-ins in `tools/e2e-macos/standins/` and run on
  every exit path (C5, results in `docs/decisions/n5krsav71f-xcuitest-suite.md`).
- `tools/e2e-macos/fixtures/.gitattributes` (`* -text`) keeps `crlf.txt` CRLF against the
  repo's `* text=auto eol=lf`.
- C6 entitlements: Xcode merges the target's `CODE_SIGN_ENTITLEMENTS` into the sandboxed
  runner's signature (checked with `build-for-testing` and `codesign -d`), so the script
  signs the runner with one read-write exception for the run's scratch folder, and
  `testRunnerWritesOnlyItsScratch` checks it at runtime. A standalone sandboxed probe
  could not run from the agent's own sandboxed shell (SIGTRAP at sandbox init), so the
  runtime answer comes from the first UI run.
- UI runs, 2026-11-16, after the user's go-ahead, with the user at the Mac: the
  Automation Mode prompt captured word for word on the probe run, and never again within
  the session (duration recorded as at least 28 minutes, longer gaps untested, by the
  user's decision). The first suite runs found that `.firstMatch` never finds gpui-pre's
  nodes, so gpui buttons resolve with `element(boundBy: 0)` by title. Four expectations
  were corrected to what the app does (file launches record files only and restore no
  layout; the preview keeps the source tab's title; the Save As caret). Then
  `--no-build --iterations 3` passed 39 of 39. A possible app bug went to the record:
  focus after the trust alert on a restored window.
