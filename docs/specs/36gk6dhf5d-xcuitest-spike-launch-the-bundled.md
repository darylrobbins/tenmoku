---
task_uid: "01m70kex5n3thbvc36gk6dhf5d"
task: "T1661"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/e2e-macos.sh exists (POSIX sh, macOS only, refuses without a GUI session); shellcheck scripts/e2e-macos.sh is clean; --dry-run prints the cargo build, bundle-macos.sh, bundle re-id/sign or URL launch, xcodegen, xcodebuild test (derived data and result bundle under ${CARGO_TARGET_DIR:-target}/e2e-macos/, test timeouts on, -test-iterations with relaunch when N \u003e 1) and xcresulttool export commands and runs none; output quoted in the decision file. Complete cleanup of every descendant process on every exit path (review finding F1, which the user overrode on 2026-11-09) is not required here: it belongs to the e2e suite card. The decision file lists it as an open item for the suite card, naming the gap F1 found."
    required: true
    manual: false
  - id: "C2"
    text: "tools/e2e-macos/project.yml generates with xcodegen into ${CARGO_TARGET_DIR:-target}/e2e-macos/ and xcodebuild build-for-testing succeeds on it (transcript in the decision file); no generated .xcodeproj or xcuserdata is committed"
    required: true
    manual: false
  - id: "C3"
    text: "The app under test is launched unambiguously (XCUIApplication(url:) or a copied bundle with CFBundleIdentifier dev.tenmoku.tenmoku.e2e), with TENMOKU_CONFIG_DIR and TENMOKU_STATE_DIR in scratch dirs, TENMOKU_NO_IPC=1 and a pre-seeded setup.toml; the decision file quotes the launched executable's path, and /Applications/Tenmoku.app and ~/Applications/Tenmoku.app are never launched or modified"
    required: true
    manual: false
  - id: "C4"
    text: "Evidence: one run of scripts/e2e-macos.sh passes SmokeTests testTypeAndSaveWithCmdS (notes.txt on disk equals the typed text after cmd-S), testMenuFileSave (File \u003e Save clicked via menuBars changes the file; File lists Save and New File) and testDumpAccessibilityTree; the result bundle, attachments dir and ax-tree dump paths are quoted in the decision file"
    required: true
    manual: false
  - id: "C5"
    text: "Evidence: scripts/e2e-macos.sh --no-build --iterations 10 ran, and the decision file has a table of pass/fail per test and wall time per iteration and per test (cold vs warm), naming every flake with its cause"
    required: true
    manual: false
  - id: "C6"
    text: "docs/decisions/\u003clane\u003e-xcuitest-spike.md has sections Status, Setup and permissions, What XCUITest sees, Reliability and run time, CI feasibility on GitHub macOS runners, Appium Mac2, Recommendation; each ends in a finding or a named open question; What XCUITest sees says whether gpui-pre's a11y tree appeared under XCUITest, with dump excerpts, and lists reachable vs frame-only elements; Setup and permissions quotes every macOS prompt word for word; CI feasibility cites GitHub or Apple docs"
    required: true
    manual: false
  - id: "C7"
    text: "Recommendation gives the suite scope, a verdict (fully, partly or not retirable, with a reason) for each of T160 T172 T197 T208 T248 T251 T261 T267 T311 T323 T338 T361 T400 T402 T436 T486 T503 T524 T525 T526 T527 T528 T529 T531 T558 T878 T1003 T1008 T1239 T1344, the a11y work that would unlock editor assertions mapped to docs/accessibility-audit.md's follow-up cards, and follow-up cards of size s or m (suite, CI job needing human risk approval, a11y prerequisites); none is filed by this card"
    required: true
    manual: false
  - id: "C8"
    text: "git diff main --stat lists only the scoped files: nothing under crates/, .github/, Cargo.toml, Cargo.lock, regatta.toml, TESTING.md or scripts/bundle-macos.sh; nothing is added to nextest, the gate or CI"
    required: true
    manual: false
  - id: "C9"
    text: "The decision file's status line says proposed and names the user as the approver; the finish notes ask the USER to sign off before any suite card is drafted"
    required: true
    manual: false
---

# XCUITest spike: launch the bundled Tenmoku.app on a scratch project, type, save with cmd-S and File > Save, dump the accessibility tree XCUITest sees, and record flakiness, permissions and CI feasibility in a decision record the user signs off

XCUITest spike (user-approved 2026-11-09). The user asked whether we have end-to-end tests. We don't: nothing launches the shipped app and drives it with real OS input. They then asked about Swift end-to-end tests and said "Sounds good" to this proposal: "A spike card first: a minimal XCUITest target that launches the bundled app on a temp project, types, saves with cmd-S and checks the file on disk, clicks one menu item, and dumps the accessibility tree XCUITest sees. That answers the tree question and flakiness before we commit to a suite. macOS-only, you as approver."

This card answers questions and leaves a small harness behind. It builds no suite, changes no product code, and adds nothing to the gate or CI.

## Ask first (the Mac is the user's own working machine, one lane)

An XCUITest run takes over the screen, mouse and keyboard. Stop and ask the lead, who asks the user, **before**:
- any `cargo build` (the release build the bundle needs);
- **each** `xcodebuild test` / `scripts/e2e-macos.sh` run that launches the app. The user may need to step away, or to grant a permission in a dialog;
- `rgt finish`.

You may run `--dry-run`, `xcodegen generate`, `xcodebuild build-for-testing`, shellcheck and file edits without asking. Never run `sudo`. Never change System Settings, TCC or Automation Mode yourself (`automationmodetool enable-automationmode-without-authentication` is the user's step). Write down every prompt macOS shows, word for word. If the screen locks, a UI test hangs (see the GUI perf note in memory), so every run has a timeout, and after one you kill the whole process group (runner, app, `testmanagerd` children).

## Facts checked while drafting (2026-11-09)

- Xcode 27.2 (27B52) is selected at /Applications/Xcode.app. `/opt/homebrew/bin/xcodegen` is installed and already in the Brewfile; so is shellcheck. `automationmodetool` says "Automation Mode is disabled. This device requires user authentication to enable Automation Mode."
- `scripts/bundle-macos.sh` builds `Tenmoku.app` from an already-built `target/release/tenmoku`, under `CARGO_TARGET_DIR` when that is set. It takes `--out DIR`, `--version` and `--sign-only APP`, signs ad hoc by default, and has no Sparkle unless `--sparkle` is passed. **Do not edit it.**
- The bundle id is `dev.tenmoku.tenmoku` (crates/fastedit/macos/Info.plist). **Two installed copies have that id: /Applications/Tenmoku.app and ~/Applications/Tenmoku.app.** So `XCUIApplication(bundleIdentifier: "dev.tenmoku.tenmoku")` can launch the wrong app. Launch the bundle under test unambiguously. Use `XCUIApplication(url:)` if the 27.2 SDK has it. Otherwise the script copies the bundle, sets `CFBundleIdentifier` to `dev.tenmoku.tenmoku.e2e` with PlistBuddy, re-signs with `scripts/bundle-macos.sh --sign-only`, and registers it with `lsregister -f`. Either way the evidence must show the executable path of the process that was launched. Never launch, move or modify the installed copies.
- Isolation. Set these in `launchEnvironment`:
  - `TENMOKU_CONFIG_DIR` and `TENMOKU_STATE_DIR` (fe-config/src/paths.rs), both scratch dirs.
  - `TENMOKU_NO_IPC=1`, so the launch never hands off to the user's running Tenmoku (crates/fastedit/src/ipc.rs).

  On macOS the log goes to `$HOME/Library/Logs/Tenmoku/tenmoku.log` (crates/fastedit/src/log_file.rs). Say in the decision file whether you overrode HOME, and what that did.
- First run. An empty state dir shows the setup wizard (fe-config/src/first_run.rs). Before launch, pre-seed `<state>/setup.toml` with `outcome = "skipped"`. If a folder trust prompt appears, pre-seed `trust.toml` as crates/fastedit/tests/extension_disabled.rs does, or use a project with nothing that needs trust. Record which.
- CLI: `tenmoku [options] [--] [path[:line[:column]] ...]`. The first folder is the project root and the last path is the active tab. So launch with `<project> <project>/notes.txt`.
- The menu bar is a real NSMenu, built from crates/fe-keymap/menus/fastedit.toml (checked by crates/fastedit/tests/menus.rs). File > Save is `editor.save`.
- What AX sees. docs/accessibility-audit.md says fastedit exposes nothing of its own to assistive technology: EditorElement paints directly, and settings controls have no roles. crates/fe-workbench/tests/a11y_tree_dump.rs found that gpui-pre builds its a11y tree only after the platform's assistive-technology activation fires. Nobody knows whether an XCUITest session fires it. **That is the main question.**
- xcodebuild passes `TEST_RUNNER_<NAME>` variables to the test runner as `<NAME>`. Use that to hand the scratch paths from the script to the Swift test.
- Gate cost. A change outside the crates other than docs/** makes `scripts/ci-test-affected.sh` run the whole workspace. That is expected here; nothing in this card adds to nextest.

## Change

1. **`tools/e2e-macos/project.yml`** (XcodeGen spec).
   - One macOS UI-testing bundle target, `TenmokuE2E`, and a scheme for it. Set `GENERATE_INFOPLIST_FILE` so there is no plist to commit.
   - If xcodebuild refuses a UI-test bundle without a target application, add a stub host app: `tools/e2e-macos/Host/HostApp.swift`, nothing else.
   - Don't commit a generated `.xcodeproj`. The script generates it under `${CARGO_TARGET_DIR:-target}/e2e-macos/`.
   - Add `tools/e2e-macos/.gitignore` only if Xcode writes `xcuserdata` or build output next to the spec.

2. **`tools/e2e-macos/TenmokuE2E/SmokeTests.swift`**: one `XCTestCase`, `SmokeTests`. Each test:
   - launches the app on a fresh scratch project (from the `TEST_RUNNER_` paths);
   - waits for the window with `waitForExistence` (no sleeps, except one fixed settle delay you name and justify);
   - terminates the app in `tearDown`, and attaches a screenshot on failure.

   The tests:
   - `testTypeAndSaveWithCmdS`: `typeText` a known line, `typeKey("s", modifierFlags: .command)`, then poll `notes.txt` on disk, for up to 5 s, until it equals the expected text.
   - `testMenuFileSave`: type more text, then click File, then Save through `app.menuBars`, and check the disk the same way. Also assert that the File menu's items include Save and New File.
   - `testDumpAccessibilityTree`: write `app.debugDescription` to an attachment and to `<results>/ax-tree-*.txt` four times:
     - right after launch;
     - after the first element query;
     - after typing;
     - after 10 s idle.

     Count the elements below the window by type. The test passes whatever it finds: it records, it does not judge.

3. **`scripts/e2e-macos.sh`** (new, POSIX sh, shellcheck-clean, macOS only).
   - Usage: `[--no-build] [--iterations N] [--only TEST] [--results DIR] [--dry-run]`.
   - Unless `--no-build` is given, it runs `cargo build --release -p fastedit`. Then it bundles with `scripts/bundle-macos.sh --out <work>/bundle` and makes the bundle unambiguous (see Facts).
   - It runs `xcodegen generate`, then `xcodebuild test` with:
     - `-derivedDataPath` and `-resultBundlePath` under `${CARGO_TARGET_DIR:-target}/e2e-macos/`;
     - `-test-iterations N -test-repetition-relaunch-enabled YES` when N > 1;
     - test timeouts on, with a per-test allowance;
     - the `TEST_RUNNER_` variables.
   - It exports attachments with `xcrun xcresulttool`, and prints the result bundle, the attachments dir and a pass/fail line last.
   - `--dry-run` prints every command and runs none.
   - It refuses to run with no GUI session and on anything but macOS.
   - It is never called from regatta.toml, nextest, ci.yml or another script.

4. **`docs/decisions/<lane>-xcuitest-spike.md`** (your lane's id; the decision-file convention). Sections, each ending in a finding or a named open question:
   - **Status**: proposed, with the user as approver.
   - **Setup and permissions**: every prompt and grant, word for word: Accessibility for Xcode Helper or the runner, Automation Mode authentication on each run versus once, and anything else. Say which steps are one-time human steps.
   - **What XCUITest sees**: whether gpui-pre's a11y tree appears (the activation question), with excerpts from the dumps. Which elements are reachable: window, title bar, traffic lights, tabs, sidebar, editor text, status bar, menus. Which are only frame-addressable.
   - **Reliability and run time**: a table of the 10-iteration run, with pass or fail per test and wall time per iteration and per test, cold versus warm. Name every flake with its cause.
   - **CI feasibility on GitHub macOS runners**: from GitHub's and Apple's docs, cited. Cover the GUI session, Automation Mode and TCC on hosted runners, Xcode versions on macos-15 and later images, and minutes cost. No workflow edit; a CI job is a follow-up.
   - **Appium Mac2**: a short note on it as the XCTest-based alternative. Don't build it.
   - **Recommendation**:
     - Suite scope.
     - For each human check task, whether a suite could retire it fully, partly or not at all, with the reason: T160, T172, T197, T208, T248, T251, T261, T267, T311, T323, T338, T361, T400, T402, T436, T486, T503, T524, T525, T526, T527, T528, T529, T531, T558, T878, T1003, T1008, T1239, T1344.
     - The a11y work that would unlock editor assertions, mapped onto the follow-up cards in docs/accessibility-audit.md.
     - The follow-up cards, size s or m each: the suite, a CI job (needs a human risk approval), and the a11y prerequisites.

## Evidence (quote transcripts in the decision file and in the finish evidence)

1. `scripts/e2e-macos.sh --dry-run` output, and `shellcheck scripts/e2e-macos.sh` clean.
2. `xcodebuild build-for-testing` succeeding on the generated project.
3. After asking, one `scripts/e2e-macos.sh` run with all three tests passing. Include the result bundle path, the attachments dir, the AX dumps and the launched executable's path.
4. After asking, `scripts/e2e-macos.sh --no-build --iterations 10`, with the results table.

If XCUITest can't drive the app at all (no window found, input refused, permission impossible without sudo), don't work around it in product code. Block the card with the exact error and what the user must do.

## Scope notes

Mac-only: don't lend to Linux. Don't touch crates/**, Cargo.toml, Cargo.lock, scripts/bundle-macos.sh, .github/**, regatta.toml, TESTING.md or the installed Tenmoku.app copies. No product change for a11y here: that work goes into the recommendation.

## Implementation notes

- **The USER must sign off** `docs/decisions/36gk6dhf5d-xcuitest-spike.md` (status
  proposed, approver the user) before any suite card (E2E-1 to E2E-4 in its
  Recommendation) is drafted. This card files none of them.
- Xcode 27's UI-test runner is sandboxed (read-only `/`), so `scripts/e2e-macos.sh` makes
  the scratch sets and the tests' records come out as attachments.
- Runs: run 1 failed in setUp (the sandbox), run 2 passed 3 of 3, and
  `--no-build --iterations 10` passed 30 of 30. The run-1 macOS prompt's wording was not
  captured. Capturing it is an open item for the suite card.
- **C6 exception, user-authorized 2026-11-09.** C6 asks for every macOS prompt word for
  word. Run 1's prompt (almost certainly the Automation Mode authorization) was seen by
  the user but not captured, and nobody watched the 10-iteration run. Asked, the user
  directed: "Record both as unknown, note that Automation Mode resets to disabled after a
  session so a CI or local run may raise an authorization prompt each time, and list
  capturing that prompt as an open item for the suite card." The decision record says
  so, and C6's verbatim-prompt part is not claimed as met.
- Review round 1: the watchdog now sends TERM then KILL to xcodebuild's own process group
  and to the runner and the app, exiting 124/130/143. A pass needs every export to work,
  every selected test to pass, and `tests × iterations` passing runs in the log. `--only`
  takes only the three test names, and `dump()` snapshots before it counts. Each path was
  exercised with stand-ins for xcodebuild (see the decision record), and the reordered
  dump has not been run in the UI yet.
- Review round 2: one owner (the main shell) now cleans up. A tracker records every
  run-owned process (pid plus start time) each second: xcodebuild's group, the runner and
  the app by path, and all their descendants. Cleanup sends TERM to each that is still
  alive, then KILL after 10 s, however xcodebuild exits. Stand-ins that resist TERM,
  outside the group and orphaned, all end killed.
