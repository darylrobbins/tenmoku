# XCUITest spike: driving the bundled Tenmoku.app from outside

Card T1661 (lane `36gk6dhf5d`), 2026-11-09. Harness: `tools/e2e-macos/project.yml`,
`tools/e2e-macos/TenmokuE2E/SmokeTests.swift`, `scripts/e2e-macos.sh`.

## Status

Status: **proposed**. Approver: **the user**. This card files no follow-up card. The suite,
the CI job and the a11y prerequisites in *Recommendation* are drafted only after the user
signs this off.

The spike answers whether XCUITest can drive the shipped app with real OS input, and what
it sees. It changes no product code and adds nothing to nextest, the gate or CI.

**Finding:** XCUITest drives Tenmoku.app. Typing, cmd-S, File > Save through the native
menu bar, and checks on disk all work. The 10-iteration run passed 30 of 30 with no
flakes, at about 36 s an iteration. gpui-pre's accessibility tree **does** appear under
XCUITest, a frame after the first query. Today it holds only kiln's labelled icon
buttons, so the editor itself is checked through the disk and the window title.

## Setup and permissions

Machine: macOS 27 (Darwin 27.0.0), Apple Silicon, Xcode 27.2 (27B52), XcodeGen from
Homebrew (already in the Brewfile), shellcheck.

What `scripts/e2e-macos.sh` does, in order (dry run quoted below):

1. `cargo build --release -p fastedit`, skipped with `--no-build`.
2. `scripts/bundle-macos.sh --binary <target>/release/tenmoku --out <target>/e2e-macos/bundle`.
   This is unchanged and signs ad hoc, with no Sparkle.
3. It re-identifies that bundle as `dev.tenmoku.tenmoku.e2e` with PlistBuddy and re-signs
   it with `scripts/bundle-macos.sh --sign-only`. The tests then launch it with
   **`XCUIApplication(url:)`**, which the 27.2 SDK has (`XCUIApplication.h:42`,
   `initWithURL:`). The bundle is never registered with `lsregister`, because a launch by
   URL doesn't need it. With a different id and a launch by path, XCUITest can't pick up
   either installed `dev.tenmoku.tenmoku` copy. It also doesn't terminate one that's
   already running: XCUITest terminates a running instance of the *same* app before a
   launch.
4. It makes a scratch pool, one set per test run: `project/notes.txt` (empty), `config/`,
   `state/setup.toml` with `outcome = "skipped"`, and `home/`.
5. `xcodegen generate` into `<target>/e2e-macos/`, then `xcodebuild test` with derived
   data and the result bundle under `<target>/e2e-macos/`, test timeouts on, and the
   `TEST_RUNNER_E2E_*` variables. xcodebuild runs in a process group of its own, and its
   output is both shown and saved to `<results>/xcodebuild.log`. A run owns that group,
   the runner and the app, and every descendant of them. testmanagerd and launchd start
   the runner and the app outside the group, so they are matched by their paths in this
   run's build products and bundle. A tracker records each owned process as pid plus
   start time every second, so a child is still known after its parent has gone. When
   xcodebuild ends, times out or the script gets SIGINT or SIGTERM, the main shell, and
   only it, sends TERM to every recorded process that is still the same process. It then
   sends KILL to any left 10 s later. After `--timeout` seconds a watchdog sends TERM,
   then KILL, to xcodebuild's group, so that the wait for it returns. The run then ends
   `FAILED (timed out …)` with status 124, or with 130 and 143 for the signals.
6. `xcrun xcresulttool export attachments`, then `get test-results tests` and
   `get test-results summary` as JSON. The tests' named text attachments are copied to
   the results folder. Each step is checked on its own: a failed export still prints
   the result locations and a failing last line, and it never hides a failing test status.
7. It passes only if xcodebuild exited 0, every export worked, `summary.json` counts all
   the selected tests as passed, and the log shows exactly `tests × iterations` passing
   test runs. `--only` takes only the three test names, so a typo can't select zero
   tests and pass.

The watchdog and the checks were exercised with stand-ins for xcodebuild
(`E2E_XCODEBUILD`, which also skips the screen-lock check because a stand-in drives no
UI). None of these runs launched the app:

| Stand-in | Run | Result |
|---|---|---|
| "family": exits on TERM, and first starts, with `setsid` outside its group, `…/DerivedData/Build/Products/Debug/MockRunner-e2etest` (ignores TERM, child `helper-resist` ignores TERM) and `…/MockQuitter-e2etest` (exits on TERM, leaving `helper-orphan`, which ignores TERM, orphaned) | `--timeout 4` | `timed out after 4s`, then `still running 10s after TERM, sending KILL to: 72969 72975 72976`, `FAILED (timed out after 4s)`, exit 124, nothing left (`pgrep` for every stand-in name finds none) |
| the same, but exits 0 by itself at 3 s, leftovers running | default | `still running 10s after TERM, sending KILL to: …` (4 pids), then `FAILED (xcodebuild exited 0, but these could not be exported: …)`, exit 1, nothing left |
| the same, hanging | SIGINT to the script | `got SIGINT; stopping …`, KILL to 3 pids, `FAILED (SIGINT)`, exit 130, nothing left |
| the same, hanging | SIGTERM to the script | `got SIGTERM; stopping …`, KILL to 4 pids, `FAILED (SIGTERM)`, exit 143, nothing left |
| ignores TERM and hangs, in the group | `--timeout 3` | `FAILED (timed out after 3s)`, exit 124 after 19 s, nothing left |
| exits 0 and runs nothing | default | `FAILED (xcodebuild exited 0, but these could not be exported: attachments, tests.json, summary.json, named attachments)`, exit 1 |
| writes run 2's real result bundle and logs 2 of 3 test runs | default | `test runs: 2 passed of 2 finished, 3 expected; summary: 3 passed of 3 tests`, then `FAILED (expected 3 passing test runs …)`, exit 1 |
| — | `--only testNope` | `--only takes testTypeAndSaveWithCmdS, testMenuFileSave or testDumpAccessibilityTree, got 'testNope'`, exit 1 |

To reproduce, put the stand-ins in a folder of their own (`helper-resist` and
`helper-orphan` are copies of `resister`), and run, for example,
`MODE=hang E2E_XCODEBUILD=<dir>/family scripts/e2e-macos.sh --no-build --timeout 4`. Then check
`pgrep -fl 'e2etest|helper-|family'` and delete the two `Mock*-e2etest` files from the build
products afterwards. The stand-ins stay out of the repo, because tests are outside this
card's scope.

`family`:

```sh
#!/bin/sh
# Stand-in xcodebuild. Starts, outside its own process group (like testmanagerd's runner),
# two "runners" at a path under the build products: one ignores TERM and has a child
# helper-resist that ignores TERM; one exits on TERM and leaves helper-orphan, which
# ignores TERM. MODE=hang: waits until TERM (default action). MODE=exit: exits 0 at 3 s.
M=$(cd "$(dirname "$0")" && pwd)
while [ $# -gt 0 ]; do
	[ "$1" = -derivedDataPath ] && derived=$2
	shift
done
dir="$derived/Build/Products/Debug"
mkdir -p "$dir"
cp "$M/resister" "$dir/MockRunner-e2etest"
cp "$M/quitter" "$dir/MockQuitter-e2etest"
"$M/setsid" "$dir/MockRunner-e2etest" "$M/helper-resist" &
"$M/setsid" "$dir/MockQuitter-e2etest" "$M/helper-orphan" &
echo "stand-in: family started"
if [ "$MODE" = exit ]; then sleep 3; exit 0; fi
while :; do sleep 1; done
```

`resister`:

```sh
#!/bin/sh
# Ignores TERM; starts $1 (if given) as a child first.
trap '' TERM
if [ -n "${1:-}" ]; then "$1" & fi
while :; do sleep 1; done
```

`quitter`:

```sh
#!/bin/sh
# Starts $1 as a child, then exits on TERM, leaving it orphaned.
"$1" &
trap 'exit 0' TERM
while :; do sleep 1; done
```

`setsid`:

```sh
#!/bin/sh
exec python3 -c 'import os,sys; os.setsid(); os.execv(sys.argv[1], sys.argv[1:])' "$@"
```

`hang-ignore-term`:

```sh
#!/bin/sh
echo "mock xcodebuild: ignoring TERM, hanging"
trap '' TERM
while :; do sleep 1; done
```

`exit-zero`:

```sh
#!/bin/sh
echo "mock xcodebuild: exits 0 having run no tests"
exit 0
```

`short-run`:

```sh
#!/bin/sh
# Stand-in: writes a real result bundle (MOCK_XCRESULT) where asked, and logs 2 of 3 test runs.
while [ $# -gt 0 ]; do
	[ "$1" = -resultBundlePath ] && out=$2
	shift
done
cp -R "$MOCK_XCRESULT" "$out"
echo "Test Case '-[TenmokuE2E.SmokeTests testMenuFileSave]' passed (1.000 seconds)."
echo "Test Case '-[TenmokuE2E.SmokeTests testTypeAndSaveWithCmdS]' passed (1.000 seconds)."
```

The pass check's pattern finds 3 passing runs in run 2's log and 30 in the 10-iteration
run's. Those two runs used the script as it was before review (commit
`b5fdc0e22`), whose watchdog sent TERM once and whose pass rested on xcodebuild's status
alone. Both passed, so the stricter checks would have passed them too.

**Open item for the suite card (E2E-1): cleanup misses children of orphans.** Review
round 3 found this gap (finding F1). The user overrode F1 on 2026-11-09, and C1 (rev 2)
moved complete cleanup to the suite card. On each pass `record_owned` walks down from the
*current* roots only: xcodebuild's process group, plus whatever matches the runner's and
the app's paths. It never walks down from processes it has already recorded. Say a
recorded helper is orphaned when its parent exits, and then starts a child of its own.
That child is reachable from no root, so it is never recorded, and cleanup leaves it
running on every exit path (normal end, timeout, SIGINT, SIGTERM). The stand-ins above
don't cover this, because their helpers start before their parents exit. The fix for
E2E-1: use every live, identity-checked recorded process as a discovery root, both while
tracking and in cleanup. Then add a stand-in where an orphan starts a long-lived,
TERM-resistant child after its parent has exited, and check that both go on each exit
path. C1's manual note in the finish evidence quotes this item.

Each test sets `launchEnvironment` to `TENMOKU_CONFIG_DIR` and `TENMOKU_STATE_DIR` (the
scratch set's folders), `TENMOKU_NO_IPC=1`, and **`HOME` = the scratch set's `home/`**.
Overriding HOME moved the app's log to
`<scratch>/<n>/home/Library/Logs/Tenmoku/tenmoku.log`, which the test attaches. The
user's `~/Library/Logs/Tenmoku/tenmoku.log` was last written at 13:03, before the first
UI run at 13:30, and it holds none of the test pids. No other effect of the HOME override
showed up. The app logged `launch: version 0.1.2 pid 20086 kind LaunchServices
displays 1`. The parent was launchd, so ipc.rs treats the launch as a LaunchServices
launch, and `TENMOKU_NO_IPC=1` keeps it alone either way. No folder-trust prompt
appeared for a project holding only `notes.txt`, so **`trust.toml` was not pre-seeded**.

The installed copies were never touched. `/Applications/Tenmoku.app/Contents/Info.plist`
is from Nov 9 08:00 and `~/Applications/Tenmoku.app/Contents/Info.plist` is from Nov 7
12:52, both before the spike. The user's own Tenmoku (`~/Applications/Tenmoku.app`,
pid 99129) kept running through every run, as each launch record shows.

### Dry run and build (C1, C2)

`<worktree>` is `/Users/daryl/code/fastedit/.worktrees/36gk6dhf5d`, and
`CARGO_TARGET_DIR` is unset, so everything lands under `<worktree>/target/e2e-macos/`.

```
$ shellcheck scripts/e2e-macos.sh; echo "exit $?"
exit 0
$ scripts/e2e-macos.sh --dry-run --iterations 10
# dry run: nothing below is run
# would refuse unless 'launchctl managername' is Aqua, /dev/console is daryl and the screen is unlocked
+ env CARGO_TARGET_DIR=<worktree>/target cargo build --release -p fastedit
+ <worktree>/scripts/bundle-macos.sh --binary <worktree>/target/release/tenmoku --out <worktree>/target/e2e-macos/bundle
+ /usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier dev.tenmoku.tenmoku.e2e' <worktree>/target/e2e-macos/bundle/Tenmoku.app/Contents/Info.plist
+ <worktree>/scripts/bundle-macos.sh --sign-only <worktree>/target/e2e-macos/bundle/Tenmoku.app
+ mkdir -p <worktree>/target/e2e-macos/results/20261109-154915
+ for n in 001 .. 030: mkdir -p <worktree>/target/e2e-macos/scratch/20261109-154915/$n/{project,config,state,home}; : >$n/project/notes.txt; echo 'outcome = "skipped"' >$n/state/setup.toml
+ xcodegen generate --spec <worktree>/tools/e2e-macos/project.yml --project <worktree>/target/e2e-macos
# in a process group of its own, output also in <worktree>/target/e2e-macos/results/20261109-154915/xcodebuild.log, with a 2400s
# watchdog that then sends TERM, and KILL 10s later, to it, the runner and the app
+ env TEST_RUNNER_E2E_APP=<worktree>/target/e2e-macos/bundle/Tenmoku.app TEST_RUNNER_E2E_SCRATCH=<worktree>/target/e2e-macos/scratch/20261109-154915 TEST_RUNNER_E2E_POOL=30 xcodebuild test -project <worktree>/target/e2e-macos/TenmokuE2E.xcodeproj -scheme TenmokuE2E -destination platform=macOS -derivedDataPath <worktree>/target/e2e-macos/DerivedData -resultBundlePath <worktree>/target/e2e-macos/results/20261109-154915/TenmokuE2E.xcresult -test-timeouts-enabled YES -default-test-execution-time-allowance 120 -maximum-test-execution-time-allowance 300 -test-iterations 10 -test-repetition-relaunch-enabled YES
+ xcrun xcresulttool export attachments --path <worktree>/target/e2e-macos/results/20261109-154915/TenmokuE2E.xcresult --output-path <worktree>/target/e2e-macos/results/20261109-154915/attachments
+ xcrun xcresulttool get test-results tests --path <worktree>/target/e2e-macos/results/20261109-154915/TenmokuE2E.xcresult ><worktree>/target/e2e-macos/results/20261109-154915/tests.json
+ xcrun xcresulttool get test-results summary --path <worktree>/target/e2e-macos/results/20261109-154915/TenmokuE2E.xcresult ><worktree>/target/e2e-macos/results/20261109-154915/summary.json
+ copy each named text attachment in <worktree>/target/e2e-macos/results/20261109-154915/attachments/manifest.json to <worktree>/target/e2e-macos/results/20261109-154915/<name>.txt
# check: summary.json has 3 tests, all passed; <worktree>/target/e2e-macos/results/20261109-154915/xcodebuild.log has 30 passing test runs
result bundle: <worktree>/target/e2e-macos/results/20261109-154915/TenmokuE2E.xcresult
attachments: <worktree>/target/e2e-macos/results/20261109-154915/attachments
AX dumps and launch records: <worktree>/target/e2e-macos/results/20261109-154915
e2e-macos: dry run, nothing ran
```

`xcodegen generate --spec tools/e2e-macos/project.yml --project target/e2e-macos`, then
`xcodebuild build-for-testing` on the generated project (log `target/e2e-macos/bft.log`):

```
⚙️  Generating plists...
⚙️  Generating project...
⚙️  Writing project...
Created project at /Users/daryl/code/fastedit/.worktrees/36gk6dhf5d/target/e2e-macos/TenmokuE2E.xcodeproj
$ xcodebuild build-for-testing -project target/e2e-macos/TenmokuE2E.xcodeproj -scheme TenmokuE2E -destination platform=macOS -derivedDataPath target/e2e-macos/DerivedData
Ld …/target/e2e-macos/DerivedData/Build/Products/Debug/TenmokuE2E-Runner.app/Contents/PlugIns/TenmokuE2E.xctest/Contents/MacOS/TenmokuE2E normal (in target 'TenmokuE2E' from project 'TenmokuE2E')
CodeSign …/TenmokuE2E-Runner.app/Contents/PlugIns/TenmokuE2E.xctest (in target 'TenmokuE2E' from project 'TenmokuE2E')
    Signing Identity:     "Sign to Run Locally"
CodeSign …/TenmokuE2E-Runner.app (in target 'TenmokuE2E' from project 'TenmokuE2E')
    Signing Identity:     "Sign to Run Locally"
** TEST BUILD SUCCEEDED **
```

The first build took 36 s wall. No stub host app was needed: a UI-testing bundle with no
target application builds and runs. The only other output is xcodebuild's "CoreSimulator
is out of date" notice, which concerns simulators and doesn't affect a macOS destination.
`git status` after generating shows nothing under `tools/e2e-macos/` except the spec and
the Swift file. The project, derived data and `xcuserdata` all live under `target/`,
which is ignored, so `tools/e2e-macos/.gitignore` isn't needed.

**Finding:** the harness builds from the spec alone, with ad-hoc signing and no team.

### The runner is sandboxed (found in run 1)

Run 1 failed in every `setUp` before any launch:

```
error: -[TenmokuE2E.SmokeTests testTypeAndSaveWithCmdS] : failed: caught error: "Error Domain=NSCocoaErrorDomain Code=513 "You don’t have permission to save the file “project” in the folder “testTypeAndSaveWithCmdS-41F9A6E0”." … {Error Domain=NSPOSIXErrorDomain Code=1 "Operation not permitted"}}"
```

Xcode 27 signs `TenmokuE2E-Runner.app` with `com.apple.security.app-sandbox = true` and
`com.apple.security.temporary-exception.files.absolute-path.read-only = [/]`. The target's
own build settings say `ENABLE_APP_SANDBOX = NO`, so the runner template's entitlements
win. The runner can read anything but can write only inside its container. So the script
makes the scratch sets, each test claims the next one with a counter in the runner's own
`UserDefaults`, and every record (AX dumps, launch records, logs) leaves the runner as an
XCTest attachment. **Finding:** a suite can't write fixtures or simulate external edits
from Swift. That work goes in the script, before the run. **Open question (suite card):**
can the runner's entitlements be widened, e.g. a read-write exception for the scratch
root? A widened runner would let tests make external edits mid-test (T400, T526).

### Prompts and grants

| When | What macOS showed | One-time? |
|---|---|---|
| Before the spike | nothing. `automationmodetool` said "Automation Mode is disabled. This device requires user authentication to enable Automation Mode." | — |
| Run 1, at the start of the UI-testing session (13:30) | **a prompt the user saw and answered, whose wording was not captured** (the user was asked and couldn't recall it). It came while Automation Mode switched from disabled to enabled, so it was almost certainly the Automation Mode authorization. That is unconfirmed | no, see below |
| Run 2 (13:34) | none: "No prompt in run 2" (the user, watching) | — |
| 10-iteration run (13:50) | **unknown**: nobody was at the Mac. Nothing blocked on a password, though. xcodebuild started at 13:50:51 and the first test at 13:50:57, the same 6 s as run 2, with nobody there to answer a prompt | — |
| Afterwards | During the 10-iteration run `automationmodetool` said "Automation Mode is ENABLED. This device requires user authentication to enable Automation Mode." After it ended: "Automation Mode is disabled. This device requires user authentication to enable Automation Mode." | — |

No Accessibility (TCC) prompt was reported for Xcode, Xcode Helper or the runner. No Screen
Recording prompt either, although XCTest 27 attached a screen recording to each failed test
in run 1 ("Screen Recording 2026-11-09 at 13.30.57.mp4"). Nothing in System Settings, TCC
or Automation Mode was changed by the agent, and `sudo` was never run. The unified log
(`log show`) returns nothing to this shell, so it couldn't fill the gap.

One-time human steps: install Xcode and XcodeGen. **Automation Mode switches back to
disabled after a test session**, so any run, local or CI, can raise an authorization
prompt each time it enables Automation Mode again. In this spike, run 2 and the 10-iteration
run started without one, and a prompt waiting for a password would have stalled them. So
something (a cached authorization, a window after the first grant) let them through. The
user can remove the prompt entirely with
`sudo automationmodetool enable-automationmode-without-authentication`, a step the agent
may not take.

**C6 is not met for prompts, by the user's decision.** C6 asks for every macOS prompt
word for word. Run 1's prompt was not captured, and nobody watched the 10-iteration run.
Asked, the user (who was away from the Mac) directed on 2026-11-09 that both be recorded
as unknown, with capturing the prompt left as an open item for the suite card. That
exception is also in the spec's Implementation notes. The wording above is not a
substitute for the prompt's text.

**Finding:** the only prompt seen was in run 1, and its wording is unknown. **Open
question (suite card, E2E-1):** capture that prompt word for word on a fresh enablement.
Find out how long an authorization carries over between sessions (minutes, the login
session, until reboot), and whether a run started past that point blocks.

## What XCUITest sees

Dumps from the passing run (`app.debugDescription`, four stages of
`testDumpAccessibilityTree`), in
`target/e2e-macos/results/20261109-133430/ax-tree-*-testDumpAccessibilityTree-001.txt`.
**How these dumps were taken:** in the runs recorded here (commit `b5fdc0e22`), each stage
first queried `windows.firstMatch.exists` and counted the window's descendants by type,
and only then took `app.debugDescription`. So no stage is a first-contact snapshot. Even
stage 1 came after the test's own count queries. After review, `dump()` now takes the
snapshot first and counts after it. That order has not been run yet, because a UI run
needs the user's go-ahead.

| Stage | Below the first window, by type (counted just before the snapshot) |
|---|---|
| 1, right after `launch()` | button=3 group=2 |
| 2, after `windows.firstMatch.waitForExistence` and a count of all 293 descendants | button=3 group=3 |
| 3, after the 1 s settle and `typeText` | **button=8** group=3 |
| 4, after 10 s idle | button=8 group=2 |

Stage 1, the window as AppKit gives it, with nothing from gpui-pre:

```
Window (Main), 0x7617c29200, {{360.0, 155.0}, {1200.0, 800.0}}, title: 'notes.txt — Tenmoku', Keyboard Focused, Disabled
  Group, 0x7617c29380, {{360.0, 155.0}, {1200.0, 800.0}}
  Button, 0x7617c29500, {{371.0, 163.0}, {16.0, 16.0}}, identifier: '_XCUI:CloseWindow'
  Button, 0x7617c29680, {{417.0, 163.0}, {16.0, 16.0}}, identifier: '_XCUI:FullScreenWindow'
    Group, 0x7617c29800, {{418.0, 164.0}, {14.0, 14.0}}, Disabled
  Button, 0x7617c29980, {{394.0, 163.0}, {16.0, 16.0}}, identifier: '_XCUI:MinimizeWindow'
```

Stage 3, after the tree came up: gpui-pre's AccessKit nodes sit under the window's
content group.

```
Window (Main), 0x7617d1fa80, {{360.0, 155.0}, {1200.0, 800.0}}, title: 'notes.txt ● — Tenmoku', Keyboard Focused, Disabled
  Group, 0x7617d1fc00, {{360.0, 155.0}, {1200.0, 800.0}}
    Button, 0x7617d1fd80, {{1210.0, 190.5}, {22.0, 22.0}}, title: 'Split Right'
    Button, 0x7617d20000, {{1234.0, 190.5}, {22.0, 22.0}}, title: 'Split Down'
    Button, 0x7617d20180, {{1258.0, 190.5}, {22.0, 22.0}}, title: 'More Actions'
    Button, 0x7617d20300, {{364.0, 217.5}, {22.0, 22.0}}, title: 'Go Back', Disabled
    Button, 0x7617d20480, {{390.0, 217.5}, {22.0, 22.0}}, title: 'Go Forward', Disabled
  Button, 0x7617d20600, {{371.0, 163.0}, {16.0, 16.0}}, identifier: '_XCUI:CloseWindow'
  …
```

**The activation question:** XCUITest's queries do fire gpui-pre's assistive-technology
activation, and the tree appears one frame later. gpui-pre 0.3.9 answers the first query
with an empty `TreeUpdate` and then forces a frame (`window.rs:1708–1722`: "A11y can be
activated at any time … we return a default empty `TreeUpdate`. So we force a new frame,
which will then send a correct `TreeUpdate`"). In run 2, stages 1 and 2 had no gpui
nodes, despite the queries before them. By stage 3, a second later, the tree was there.
In all ten dumps of the 10-iteration run, the stage 1 snapshot already had the nodes.
Which query activated it, `launch()`'s own waiting or the stage's count queries, these
runs can't tell. The reordered `dump()` will separate the two. Either way the tree stayed
through 10 s idle. What
shows up are kiln's labelled `IconButton`s (T1518 gave them a role and a required
label), with their disabled state. A suite should wait for a known labelled button,
e.g. `app.buttons["Split Right"].waitForExistence`, rather than sleep.

Reachable vs frame-only:

| Element | Reachable? | How |
|---|---|---|
| Window | yes | `app.windows.firstMatch`. The title is the active tab plus a dirty marker: `notes.txt — Tenmoku`, then `notes.txt ● — Tenmoku` after typing. Its frame. XCUITest calls it "Disabled", but typing and the menus still work |
| Title bar, traffic lights | yes | `_XCUI:CloseWindow`, `_XCUI:MinimizeWindow`, `_XCUI:FullScreenWindow` buttons with frames (y = 163, 16 tall, in a window at y = 155) |
| Menus | yes, fully | The native NSMenu: 267 `MenuItem`s in the stage 1 dump. File lists New File, Open Folder…, Open File…, Open Recent…, Trust This Folder, Untrust This Folder, Save, Save As…, Save All, Save without Formatting, Compare Active File With…, Close Tab, Reopen Closed Editor, Close Pane, Close Window |
| Pane toolbar icon buttons | yes, after activation | `Split Right`, `Split Down`, `More Actions`, `Go Back`, `Go Forward` (enabled state included) |
| Tabs | frame only | No node. Clicks by coordinate within the content group |
| Sidebar / project tree | frame only | No node |
| Editor text, caret, selection | **no** | No node. Text is checked through the file on disk after a save, and the dirty state through the window title |
| Status bar | frame only | No node |
| Toasts, find bar, palette, settings, terminal, chat | frame only | No node (kiln controls with roles would appear where they're used: open question per surface) |
| Native panels (Save As, Open) | expected yes | `prompt_for_new_path` / `prompt_for_paths` (`workbench_files.rs:53, 138`) are AppKit panels. Not exercised in the spike |

**Finding:** the tree appears under XCUITest, but today it has only the chrome's icon
buttons. Editor assertions go through the disk and the window title until the editor has
a text node (see *Recommendation*).

## Reliability and run time

One run with all three tests (`scripts/e2e-macos.sh --no-build`, run 2), 13:34:30 to
13:35:26, 55.8 s wall:

```
Test Case '-[TenmokuE2E.SmokeTests testDumpAccessibilityTree]' passed (26.707 seconds).
Test Case '-[TenmokuE2E.SmokeTests testMenuFileSave]' passed (11.922 seconds).
Test Case '-[TenmokuE2E.SmokeTests testTypeAndSaveWithCmdS]' passed (7.177 seconds).
	 Executed 3 tests, with 0 failures (0 unexpected) in 45.807 (45.810) seconds
…
launched (launched-testDumpAccessibilityTree-001.txt): pid 18985 executable /Users/daryl/code/fastedit/.worktrees/36gk6dhf5d/target/e2e-macos/bundle/Tenmoku.app/Contents/MacOS/tenmoku bundle /Users/daryl/code/fastedit/.worktrees/36gk6dhf5d/target/e2e-macos/bundle/Tenmoku.app
result bundle: /Users/daryl/code/fastedit/.worktrees/36gk6dhf5d/target/e2e-macos/results/20261109-133430/TenmokuE2E.xcresult
attachments: /Users/daryl/code/fastedit/.worktrees/36gk6dhf5d/target/e2e-macos/results/20261109-133430/attachments
AX dumps and launch records: /Users/daryl/code/fastedit/.worktrees/36gk6dhf5d/target/e2e-macos/results/20261109-133430
e2e-macos: passed
```

On disk afterwards: set 003 (`testTypeAndSaveWithCmdS`) `notes.txt` = `hello from
xcuitest`, and set 002 (`testMenuFileSave`) = `saved from the File menu`, both with no
trailing newline (`od -c`). Each launch record names the bundle under test:
`pid 20086 executable …/target/e2e-macos/bundle/Tenmoku.app/Contents/MacOS/tenmoku` and
`pid 20357 executable …/target/e2e-macos/bundle/Tenmoku.app/Contents/MacOS/tenmoku`. All
three windows were titled `notes.txt — Tenmoku`. The app's log showed `loaded 0 bytes,
1 rows in 525.8ms` for the empty file. That half-second before the editor holds the
buffer is what the one fixed 1 s settle (`editorSettle`) covers. Nothing in the AX tree
marks the moment, so there is nothing to wait on.

`scripts/e2e-macos.sh --no-build --iterations 10`
(`-test-iterations 10 -test-repetition-relaunch-enabled YES`), 13:50:51 to 13:57:19,
6 min 28 s wall, ending `e2e-macos: passed`. That covers ten iterations of three tests,
each test on its own scratch set (001 to 030) and its own app launch. Results:
`target/e2e-macos/results/20261109-135051/` (result bundle `TenmokuE2E.xcresult`,
attachments `attachments/`, AX dumps `ax-tree-*-testDumpAccessibilityTree-{001,004,…,028}.txt`).

| Iteration | testTypeAndSaveWithCmdS | testMenuFileSave | testDumpAccessibilityTree | Suite wall (s) | Relaunch gap before (s) |
|---|---|---|---|---|---|
| 1 (cold) | passed, 6.5 s | passed, 12.6 s | passed, 18.2 s | 37.3 | 6 (from 13:50:51, xcodebuild start) |
| 2 | passed, 7.5 s | passed, 13.6 s | passed, 17.9 s | 39.0 | 0.9 |
| 3 | passed, 6.4 s | passed, 12.2 s | passed, 18.6 s | 37.2 | 1.0 |
| 4 | passed, 6.2 s | passed, 12.1 s | passed, 17.7 s | 36.0 | 0.7 |
| 5 | passed, 6.2 s | passed, 12.0 s | passed, 17.1 s | 35.3 | 0.7 |
| 6 | passed, 6.2 s | passed, 12.2 s | passed, 18.3 s | 36.6 | 0.9 |
| 7 | passed, 6.4 s | passed, 12.4 s | passed, 17.6 s | 36.4 | 0.8 |
| 8 | passed, 6.3 s | passed, 12.4 s | passed, 17.9 s | 36.6 | 0.7 |
| 9 | passed, 6.4 s | passed, 12.3 s | passed, 17.7 s | 36.4 | 0.8 |
| 10 | passed, 6.3 s | passed, 11.7 s | passed, 17.2 s | 35.2 | 0.8 |

| Test | Cold (iteration 1) | Warm mean (2–10) | Warm min–max |
|---|---|---|---|
| testTypeAndSaveWithCmdS | 6.5 s | 6.4 s | 6.2–7.5 s |
| testMenuFileSave | 12.6 s | 12.3 s | 11.7–13.6 s |
| testDumpAccessibilityTree | 18.2 s | 17.8 s | 17.1–18.6 s |

"Relaunch gap" is the time from one iteration's suite end to the next one's start, which
is when the runner is relaunched. Cold means the first iteration after `xcodebuild test`
started (the app binary had already run in run 2, so the OS caches were warm). Cold and
warm are within a second of each other: the time goes to the app's startup and the
tests' waits, not to first-launch costs. Per test, the fixed waits are 1 s
(`editorSettle`), plus 10 s idle in `testDumpAccessibilityTree`. The rest is launch,
window, title and the XCUITest queries. `testMenuFileSave` is about 6 s over
`testTypeAndSaveWithCmdS`. Its extra steps are opening the menu, waiting for Save, and
reading each of the File menu's items, and each read is an XCUITest query. Those
queries likely account for the difference.

All 30 launch records name
`/Users/daryl/code/fastedit/.worktrees/36gk6dhf5d/target/e2e-macos/bundle/Tenmoku.app/Contents/MacOS/tenmoku`,
and all 30 also note `shipped id running: … /Users/daryl/Applications/Tenmoku.app`, the
user's own instance, left alone. All 30 windows were titled `notes.txt — Tenmoku`. The 20
saving sets held exactly the typed text afterwards.

**Flakes: none.** 30 of 30 passed, with no retries. One nondeterminism showed up that a
suite must handle, though it failed nothing: **when gpui-pre's tree arrives varies.** In
run 2 the stage 1 and 2 dumps had no gpui nodes. In all ten 10-iteration dumps, the stage 1
snapshot already showed `Split Right` and the other four buttons, while that stage's type
count, taken just before the snapshot, said `button=4 group=1`. The tree arrived between
the count and the snapshot. Stage 4 counted `group=2` in six iterations and `group=3` in four,
which is the traffic light's nested group. Cause: AccessKit activation answers the first
query empty and builds on the next frame (see *What XCUITest sees*). Any assertion on a
gpui node must `waitForExistence`.

**Finding:** reliable at 30 of 30, about 6–18 s per test, about 36 s for the three. The
only timing hazard is the tree's arrival, and a wait handles it.

## CI feasibility on GitHub macOS runners

No workflow was edited. From the docs:

- **GUI session.** GitHub's runner reference says nothing about GUI or UI tests
  ([GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)).
  Our own `check` job (`.github/workflows/ci.yml`, `runs-on: macos-15`) already opens
  GPUI windows: the screenshot examples render every scene there, on a 1× display. So a
  window server and a logged-in session exist. Whether testmanagerd can drive them is
  unproven.
- **Automation Mode.** The same page says "The Linux and macOS virtual machines both run
  using passwordless `sudo`." `automationmodetool(1)` sets a device "so that Automation
  Mode for UI testing can be turned on without user authentication", which suits CI
  machines, and it needs an administrator
  ([man page](https://keith.github.io/xcode-man-pages/automationmodetool.1.html)). So a
  job step `sudo automationmodetool enable-automationmode-without-authentication` is
  possible. On a Jenkins Mac that command was the accepted fix for the per-test password
  prompt ([Apple forum 793307](https://developer.apple.com/forums/thread/793307)). No
  source confirms it on a GitHub-hosted image.
- **TCC.** Nothing in the GitHub docs. On this Mac no TCC prompt appeared, only Automation
  Mode. Unverified on the hosted image.
- **Xcode.** "Only one major version of Xcode will be supported per macOS version", and
  betas only on the latest image
  ([runner-images README](https://github.com/actions/runner-images)). `macos-26` (arm64,
  also `macos-latest`) has Xcode 26.0.1 to 26.6, default 26.6, on image 20261102.0473.1
  ([macos-26-arm64 README](https://github.com/actions/runner-images/blob/main/images/macos/macos-26-arm64-Readme.md)).
  `macos-15` stays on Xcode 26.x too. Xcode 27 is only a preview image (`xcode-27`). The
  spike ran Xcode 27.2. Under Xcode 26 the project should generate unchanged (nothing in
  `project.yml` is 27-only). The runner sandbox found above may differ, and the script
  copes either way.
- **Hardware and cost.** The standard arm64 runner has 3 M1 cores, 7 GB RAM and 14 GB SSD
  (the runners reference). The standard macOS runner costs $0.062 a minute
  ([runner pricing](https://docs.github.com/en/billing/reference/actions-runner-pricing)),
  against $0.006 for a standard 2-core Linux runner. Standard hosted runners are free in
  public repositories. For a private repository, as Tenmoku's is today, each job's
  minutes × rate is deducted from the plan's included compute allowance. That allowance
  is "a monetary amount, not a fixed number of minutes" ($18 a month on Pro or Team)
  ([How use of GitHub Actions is measured](https://docs.github.com/en/billing/concepts/product-billing/github-actions)).
  At $0.062 a minute, a 45-minute macOS job is about $2.80, so a run on every push would
  use up Pro's $18 in about six runs.
  The release build took 14 min 08 s here. On 3 cores, with the 14 GB disk that the Linux
  job already overflows (T1655), expect it to be the bulk of the job. The tests
  themselves take about a minute for three.

**Finding:** feasible on paper (passwordless sudo for Automation Mode, a window server
already in use), unproven in practice. **Open question:** does XCUITest get a session on
the hosted image with Automation Mode enabled by `sudo`? Only a probe job answers that,
and it needs a human risk approval (a workflow edit).

## Appium Mac2

Appium's Mac2 driver is "an Appium driver for automating macOS applications using Apple's
XCTest framework" ([appium-mac2-driver](https://github.com/appium/appium-mac2-driver)).
Its server side is an XCTest UI-test bundle (WebDriverAgentMac), so it sees exactly the
tree above and needs the same Automation Mode grant. What it adds is a WebDriver protocol
over HTTP, clients in any language, and a Node and Appium server to install and pin (5.x
needs Appium 4). That buys nothing here. The suite is Mac-only and drives one app, and
Swift XCTest needs no extra runtime.

**Finding:** not worth adopting. Reconsider only if tests must be written outside Swift
or shared with a non-Mac runner.

## Recommendation

### Suite scope

A small, on-demand Mac suite, run with `scripts/e2e-macos.sh` by a person at the Mac (or
by an agent after asking), and never in the gate. Today it can cover what reaches the app
through real OS input and comes back through the disk, the process, the window (title,
dirty marker, frame, traffic lights), the native menus and panels, and kiln's labelled
icon buttons:

- save paths: cmd-S, File > Save, Save All, Save As… through the native panel, saving
  encodings byte-for-byte (T251's disk half);
- the window title as the dirty and active-tab signal, after edits, undo-to-clean and close;
- relaunch and hot exit: quit by menu, relaunch on the same state dir, check the restored
  title and dirty marker, and that the backups are mode 0600 (T526's disk half);
- the first-run wizard's once-only marker and the "Set Up Tenmoku…" menu item (T1344's
  disk half), trust by menu and `trust.toml` (T197), Open Recent and `recent.toml` (T528);
- the vim clipboard against `NSPasteboard` (T361);
- buttons that are already labelled: the markdown preview buttons appear only on markdown
  tabs (T1008), and the pane toolbar buttons' enabled state.

Fixtures are made by the script, because the runner is sandboxed: a scratch set per test,
with a fixture name the test declares. Each test waits for a labelled button to know the
tree is live, and keeps the one named settle delay for the buffer load. Pixel checks stay
with the screenshot goldens. Taste, IME and VoiceOver stay human.

**Finding:** a suite of about ten tests at m size is worth building now. Editor-content
assertions wait for the a11y work below.

### Human check tasks

"Fully" means a suite can run every step of the check. "Partly" means it takes the steps
named and the rest stays human. "Not" means nothing useful can be automated. "After a11y"
names the A11Y card (the T294 design, `docs/decisions/qenw9xxdzk-accessibility-design.md`
section 11) whose nodes the rest would need.

| Task | Verdict | Reason |
|---|---|---|
| T160 title bar | partly | The traffic lights' frames are reachable, so centring in the 32 px bar is arithmetic (they sat 8 px below the window top, 16 tall). Dragging the bar by coordinate and reading the window frame works. Double-click follows a System Setting the suite must not change, so it can only read `AppleActionOnDoubleClick` and assert the matching result |
| T172 fold chevrons vs VS Code | not | Visual comparison and taste |
| T197 folder trust | partly | Menu Trust/Untrust, relaunch, `trust.toml`, and language-server processes are all reachable. The sheet's content needs its controls in the tree (A11Y-3) |
| T208 vim visual mode | partly, after a11y | The results of d/y/c/>/p can be checked on disk after `:w`. Selection and the VISUAL label need A11Y-7 and A11Y-3. Block-cursor drawing stays visual |
| T248 theme and hot reload | not | Colours are pixels, and the screenshot goldens own them. Live config edits need a writable fixture mid-test, which the sandbox forbids |
| T251 file encodings | partly | Open, edit, save and compare bytes against the fixture, and the refused lossy save keeps `●` in the title. The status bar's encoding item needs A11Y-3 |
| T261 proposal demo | not | It's the `proposal_demo` example, not the bundled app, and its bands and blocks are visual |
| T267 in-file find | partly, after a11y | Keys and the disk are reachable. The find bar, "n of m" and the selection need A11Y-6, A11Y-9 and A11Y-7. Highlights stay visual |
| T311 vim search highlighting | not | The check is the highlight drawing |
| T323 go-to-definition, real servers | partly, after a11y | Landing row and column need the caret (A11Y-7), plus real servers installed on the Mac |
| T338 vim command line | partly, after a11y | `:s` results on disk are reachable. The line, the prefill and E492 need A11Y-5 and A11Y-9 |
| T361 vim clipboard | **fully** | `yy` then read `NSPasteboard`, set the pasteboard then `p` then save and check the disk, one launch per `clipboard` value from a pre-seeded `settings.toml`. "Another app" becomes the pasteboard |
| T400 very large files | partly | Typing does nothing (disk unchanged), relaunch restores the tab (title). Memory needs a process probe outside the sandbox, an external edit needs the widened runner (open question above), and the banner needs A11Y-3/9. First-screen time is a perf check |
| T402 LSP diagnostics, hover, completion | partly, after a11y | Completion insert plus a single cmd-z can be checked on disk. Underline, hover and the menu need A11Y-7/9/13. Needs real servers |
| T436 Japanese IME | not | `typeText` sends key events and doesn't drive an input method's candidate window. Typing feel is human |
| T486 git change bar | not | Lane, shape and colour are pixels |
| T503 AI demo host | not | The `ai_demo` example, visual ghost text and bands |
| T524 find and replace in a file | partly | Keys, then save and compare the disk, and one cmd-z restores it. "Replaced N" needs A11Y-9, and the fields need A11Y-5/6 |
| T525 project-wide replace | partly | Closed files changing on disk and `git diff` are reachable. Counts, the prompt and the tabs need A11Y-4/6/9 |
| T526 hot exit, recovery, auto-save | partly | Quit by menu, relaunch, title with `●`, backup modes and auto-save writes on disk are reachable. `kill -9` needs a signal the sandboxed runner can't send. Finder drops are human. Auto-indent needs A11Y-7 or a save |
| T527 project tree file ops | partly | File > New File and the disk and Trash results are reachable. Tree rows and the inline error need A11Y-4 |
| T528 Open Recent | partly | The menu item, relaunch and `recent.toml` are reachable. The picker's rows need A11Y-6 |
| T529 problems panel | partly, after a11y | Counts need the status bar (A11Y-3), the panel needs A11Y-4, and it needs a real rust-analyzer |
| T531 toasts | partly, after a11y | The `chmod 000` setup is the script's. The toast needs A11Y-9's `Alert` group. Fading stays visual |
| T558 git views | partly | Stage, commit and discard results via `git status`/`git log` are reachable. The diff view, blame and the picker need a11y on surfaces the T294 design leaves for later |
| T878 vim remaps, `:w :q`, scrolling | partly | `:w` (disk), `:q` with E37 (window stays), `:q!` (window or app gone) and pre-seeded remaps checked by their effect on disk are reachable. Status-bar problems need A11Y-3. Scrolling is visual |
| T1003 vim g$ and gm | partly, after a11y | Caret position needs A11Y-7. Horizontal scroll is visual. A narrow window is a coordinate drag of the window edge |
| T1008 markdown preview buttons | partly | The eye and panel-right buttons are kiln icon buttons, so they should already be in the tree like "Split Right" (verify). Toggling, `●` persisting and relaunch are reachable. Legibility is taste |
| T1239 settings text caret | partly, after a11y | Caret position needs A11Y-5 on the settings fields (A11Y-12). Caret visibility is visual |
| T1344 first-run wizard | partly | Fresh dirs show the wizard. Its kiln controls carry roles since T1518 (verify in the tree). `settings.toml` holding only the changed keys, once-only across relaunches, and the menu item are reachable. Preview colours and VoiceOver stay human |

**Finding:** one task (T361) can be retired fully, 22 partly (13 of them with steps a
suite can take today, 9 only after a11y work), and 7 not at all. The suite shortens most human checks without ending
them.

### A11y work that unlocks editor assertions

Mapped onto `docs/accessibility-audit.md`'s follow-up cards, and the A11Y cards of the
T294 design that replaced them:

| Assertion a suite wants | Audit card | T294 card |
|---|---|---|
| Wait for "tree live" by a known node, and dump trees from a run | 1 (dump hook): **XCUITest already gives a dump from the running app on the Mac**, so card 1 matters for Linux only | A11Y-1 |
| Status bar items (encoding, counts, vim mode), regions | 2 | A11Y-2, A11Y-3 |
| Tabs by name, active and dirty; tree rows | 3 | A11Y-4 |
| Find bar, palette and picker rows; settings fields | 4 | A11Y-6, A11Y-12 |
| Toasts and messages as nodes | 5 | A11Y-9 |
| Text fields' values (find query, vim line, settings) | 6 | A11Y-5 |
| **Editor text, caret and selection**: the editor assertions | 7 | **A11Y-7** (needs A11Y-5, then A11Y-2) |

**Finding:** the path to editor assertions is A11Y-2, then A11Y-5, then A11Y-7, as the
T294 design already orders it. Nothing new is needed on the gpui-pre side, because
activation under XCUITest works.

### Follow-up cards (proposed, none filed)

1. **E2E-1: Mac smoke suite** (chore, m, Mac-only). It grows `SmokeTests` into the suite
   scope above. It adds fixture-per-test scratch sets to `scripts/e2e-macos.sh`
   (`--fixture` names a folder under `tools/e2e-macos/fixtures/`) and a
   `waitForTree()` helper on a labelled button. It answers the open questions above.
   It captures the Automation Mode prompt word for word on a fresh enablement, finds
   how long an authorization carries over between sessions, and checks whether the
   runner's entitlements can be widened. It also closes the cleanup gap review found
   (F1, the open item under *Setup and permissions*): children that orphaned helpers
   start are never tracked. Scope: `tools/e2e-macos/**`, `scripts/e2e-macos.sh`. Never in the gate.
2. **E2E-2: CI probe job** (chore, s, **needs a human risk approval**: it edits
   `.github/workflows/`). A `workflow_dispatch`-only job on `macos-26` that runs
   `sudo automationmodetool enable-automationmode-without-authentication`, builds,
   runs `scripts/e2e-macos.sh --iterations 3`, and uploads the result bundle. It records
   whether a hosted runner can drive the app and what the job costs. A scheduled job is a
   later decision.
3. **E2E-3: a11y chrome for the suite** (feature, m): A11Y-2 plus A11Y-3 from the T294
   design, taken first, because status-bar items and regions unlock the most partial
   verdicts (T197, T251, T400, T529, T878). The T294 card list is approved separately;
   this only orders it.
4. **E2E-4: suite assertions on editor text** (chore, s, after A11Y-5 and A11Y-7). It
   adds editor value, caret and selection checks to the suite and moves T208, T267,
   T323, T338 and T1003 from "after a11y" to their partial verdicts.

**Open question for the user (sign-off):** approve the suite scope and cards 1 and 2 (card 2 with
its risk approval). The a11y cards follow the T294 design's own approval.
