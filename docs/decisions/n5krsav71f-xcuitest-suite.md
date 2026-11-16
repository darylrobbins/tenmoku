# XCUITest suite (E2E-1): the Mac suite for the bundled Tenmoku.app

Card T1682 (lane `n5krsav71f`), 2026-11-10. It builds what the approved spike record,
`docs/decisions/36gk6dhf5d-xcuitest-spike.md` (T1661), scopes as E2E-1, closes that
record's open cleanup item (F1), and answers the three questions it left to this card.
Harness: `scripts/e2e-macos.sh`, `tools/e2e-macos/project.yml`,
`tools/e2e-macos/TenmokuE2E/{SmokeTests,Support,Fixtures}.swift`,
`tools/e2e-macos/fixtures/`, `tools/e2e-macos/standins/`.

Mac only, run on demand by a person at the Mac (or an agent after asking). It is not in
nextest, the gate or `.github/workflows`, and it checks no editor text: that waits for
A11Y-5 and A11Y-7 (E2E-4).

## The suite

Thirteen tests in `SmokeTests`, each on a scratch set of its own and its own app launch
(the relaunch tests launch twice on the same set). What the editor holds is checked on
disk after a save, its dirty state through the window title, and the rest through the
native menus, panels and alerts, `NSPasteboard`, the state files and kiln's labelled icon
buttons.

| Test | Fixture | Checks | Human check it shortens |
|---|---|---|---|
| `testSaveWithCmdSAndFileMenu` | `plain` | cmd-S, then File > Save, each on disk; the File menu's save and recent items; title dirty, then clean | T251 (save half) |
| `testSaveAllAndSaveAs` | `two-files` | File > Save All writes both dirty tabs; File > Save As… through `NSSavePanel` writes `a-copy.txt`, leaves `a.txt`, retitles the tab | T251 (save half) |
| `testSaveEncodingsByteForByte` | `encodings` | windows-1252 (guessed, no BOM), UTF-16LE with BOM, UTF-8 with BOM, UTF-8 CRLF: each edited at its start and saved is the original bytes plus the typed text in that encoding, BOM kept, Return written as CRLF | T251 (disk half) |
| `testWindowTitleDirtyAndActive` | `two-files` | `●` after an edit, gone after undo back to the saved text, follows the active tab across cmd-shift-[ and ], gone after cmd-S; closing a dirty tab asks (Cancel, Discard), Cancel keeps it; closing the last tab leaves no file in the title | T526 (title), T251 (dirty marker) |
| `testQuitByMenuHotExitAndRelaunch` | `plain` | Tenmoku > Quit Tenmoku with unsaved text: no question, `notes.txt` untouched, a backup holding the text, every backup file mode 0600; a folder relaunch on the same state (answering its trust alert) restores the tab dirty (`notes.txt ● — Tenmoku`), and File > Save All writes the text | T526 (disk half) |
| `testFirstRunWizardOnce` | `plain`, fresh state | an empty state folder shows the wizard (`Skip setup`) and writes `outcome = "shown"`; Skip setup writes `"skipped"`; a relaunch shows nothing and leaves `setup.toml` byte for byte; Tenmoku > Set Up Tenmoku… shows it again, Escape skips | T1344 (disk half) |
| `testTrustByMenu` | `plain` | a folder launch asks "Do you trust the authors…" (Trust Folder, Don't Trust); Don't Trust, then File > Trust This Folder, Untrust This Folder and Trust again each land in `trust.toml` as the canonical folder's `true`/`false`; a relaunch doesn't ask again and leaves the file | T197 |
| `testOpenRecentAndRecentToml` | `recent` | `recent.toml` lists the file opened, then after a relaunch on another file lists that one first (a file launch records no folder); File > Open Recent… with the query `quiz` and Return opens `quiz.txt` | T528 |
| `testVimClipboardOff` | `vim`, `settings/off.toml` | `vim.clipboard = ""`: `yyP` then `:w` puts vim's own register (disk `alpha alpha beta`) and leaves the pasteboard as the test set it; `"+yy` writes it | T361 |
| `testVimClipboardUnnamedplus` | `vim`, `settings/unnamedplus.toml` | `vim.clipboard = "unnamedplus"`: `yy` writes the pasteboard; text another app left there goes in with `P`, then `:w` (disk) | T361 |
| `testMarkdownPreviewButtons` | `markdown` | Open Preview and Open Preview to the Side show on the markdown tab only (gone on `notes.txt`); Open Preview swaps the tab to the preview (Show Source; the window keeps `notes.md ● — Tenmoku`), Show Source back, the file untouched | T1008 |
| `testPaneToolbarEnabledState` | `lines` | Go Back and Go Forward disabled with no history, Split Right, Split Down and More Actions enabled; a go-to-line jump enables Back, Back and Forward swap their states; Split Right makes a second pane with its own buttons | (pane toolbar, record's last bullet) |
| `testRunnerWritesOnlyItsScratch` | `plain` | the runner may write its run's scratch folder and nowhere beside it (see *Widening the runner's entitlements*) | T400, T526 (external edits, later) |

T361 is the one check the spike record retires fully: both `vim.clipboard` values, each
on its own launch from a pre-seeded `settings.toml`, against `NSPasteboard` ("another app"
is the pasteboard). Each test that writes the pasteboard saves its items first and puts
them back when it ends, so a run leaves the person's clipboard as it was.

### Fixtures

The runner is sandboxed, so `scripts/e2e-macos.sh` makes every scratch set before the
run. Each test declares its fixture in `Fixtures.swift`, one line per test, which the
script reads with `sed` and checks: every test method has exactly one entry, and every
fixture folder and settings file named exists, or the script stops before building.
A set is `<scratch>/<test>/<iteration>/`: `project/` copied from
`tools/e2e-macos/fixtures/<fixture>/project/`, `config/` with the declared
`settings/<name>.toml` as `settings.toml`, `state/` with `setup.toml`
(`outcome = "skipped"`) unless the entry says `fresh: true`, and `home/`. Each test checks
its set against the fixture byte for byte before it launches, so a reused or stale set
fails at once. `--fixture <name>` runs the tests that declare `<name>` (with `--only`,
that test must), and `--only` takes only declared names. The fixtures folder has a
`.gitattributes` of its own (`* -text`), because the repository's `* text=auto eol=lf`
would turn `crlf.txt` into LF.

T1664's behaviour stays: the script exports a temporary `TENMOKU_STATE_DIR` and
`TENMOKU_CONFIG_DIR` of its own, removed on exit, and each test's app gets the set's.

### Waiting

`waitForTree()` waits for the button titled `Split Right` (the pane's tab-row button, there
in every pane) with `waitForExistence`, after an `exists` check. The button is resolved
with `element(boundBy: 0)`, because `.firstMatch` never finds gpui-pre's nodes (see *The
real run*). Every check of a gpui node (`expectButton`,
`expectNoButton`, `expectButtonCount`, `clickButton`) goes through it first. Titles,
menus, native alerts and panels wait with `waitForExistence` too, and the disk and the
pasteboard are polled every 50 ms up to a timeout. The one fixed delay is `editorSettle`
(1 s), after the title and tree are up, for the buffer load and editor focus that
nothing in the tree reports. `rg 'sleep|usleep' tools/e2e-macos/TenmokuE2E` finds only
`Thread.sleep(forTimeInterval: Self.editorSettle)` in `settle()`.

Absence after a relaunch (no wizard, no trust question) is checked once the relaunch has
settled, together with the file the offer would have rewritten first (`setup.toml`
becomes `shown` before the wizard shows; a trust answer is written to `trust.toml`). The
launch's decisions are a file read or two, made at its start, so by then they're done.

## F1: complete cleanup of launched processes

The spike record's open item: `record_owned` walked down only from the current roots
(xcodebuild's process group, and the processes matching the runner's and the app's
paths), so a recorded helper orphaned when its parent exited, which then started a
child, left that child running on every exit path.

Fixed: `roots()` now also returns every recorded process that is still the same process
(`alive_owned`: alive, not a zombie, with the start time recorded), so each tracker pass,
every second while the run lasts, and each pass in `cleanup_run` walk down from them too.
Cleanup sends TERM to every owned process, KILL to any left 10 s later, then checks again
up to three times and sends KILL to anything a killed process started meanwhile.

### The stand-ins

The spike record's stand-ins are now in the repo, `tools/e2e-macos/standins/` (with a
README), with a new one for F1, `late-orphan`. It starts, outside its process group and
at a path under the build products (as testmanagerd starts the runner),
`MockLateParent-e2etest`, which starts `late-helper` and exits 3 s later. `late-helper`,
now an orphan that matches no root, ignores TERM, waits until its parent has gone, then
starts `late-child`, which ignores TERM and runs until killed. Every run below used
`E2E_XCODEBUILD=tools/e2e-macos/standins/<name> scripts/e2e-macos.sh --no-build`, launched
no app, and was followed by `pgrep -fl 'e2etest|standins/|helper-resist|helper-orphan'`.
Signals were sent 7 s (`late-orphan`) or 5 s (`family`) after the start, by a harness
that runs the script with SIGINT at its default disposition.

**Before the fix**, the script as of `d20f563` with `late-orphan`, `MODE=hang`,
`--timeout 8`: the orphan's late child was left running.

```
stand-in: late-parent exits, leaving pid 42523 orphaned
stand-in: late-helper (pid 42523, parent gone) started late-child pid 42799
e2e-macos: timed out after 8s; stopping xcodebuild, the runner and the app
e2e-macos: still running 10s after TERM, sending KILL to: 42523
e2e-macos: FAILED (timed out after 8s)
exit 124; pgrep afterwards: 42799 /bin/sh …/tools/e2e-macos/standins/late-child
```

**After the fix** (KILL lists include the stand-ins' own `sleep` children):

| Stand-in | Run | Result |
|---|---|---|
| `late-orphan`, `MODE=hang` | `--timeout 8` | `late-helper (pid 55781, parent gone) started late-child pid 56201`, `timed out after 8s`, `still running 10s after TERM, sending KILL to: 55781 56201 59306`, `FAILED (timed out after 8s)`, **exit 124**, nothing left |
| `late-orphan`, `MODE=exit` (exits 0 at 8 s) | default | `started late-child pid 60856`, `sending KILL to: 60305 60856 63726`, `FAILED (xcodebuild exited 0, but these could not be exported: attachments, tests.json, summary.json, named attachments)`, **exit 1**, nothing left |
| `late-orphan`, `MODE=hang` | SIGINT | `started late-child pid 65663`, `got SIGINT; stopping …`, `sending KILL to: 64826 65663 68119 68172`, `FAILED (SIGINT)`, **exit 130**, nothing left |
| `late-orphan`, `MODE=hang` | SIGTERM | `started late-child pid 69575`, `got SIGTERM; stopping …`, `sending KILL to: 69095 69575 71338 71426`, `FAILED (SIGTERM)`, **exit 143**, nothing left |
| `family`, `MODE=hang` | `--timeout 4` | `timed out after 4s`, `sending KILL to: 72181 72194 72195 74877 74984`, `FAILED (timed out after 4s)`, exit 124, nothing left (as in the spike record) |
| `family`, `MODE=exit` | default | `sending KILL to: 75903 75909 75911 78875 78876`, `FAILED (xcodebuild exited 0, but these could not be exported: …)`, exit 1, nothing left (as before) |
| `family`, `MODE=hang` | SIGINT | `got SIGINT; …`, `sending KILL to:` 6 pids, `FAILED (SIGINT)`, exit 130, nothing left (as before) |
| `family`, `MODE=hang` | SIGTERM | `got SIGTERM; …`, `sending KILL to:` 6 pids, `FAILED (SIGTERM)`, exit 143, nothing left (as before) |
| `hang-ignore-term` | `--timeout 3` | `FAILED (timed out after 3s)`, exit 124 after 18 s, nothing left (as before) |
| `exit-zero` | default | `FAILED (xcodebuild exited 0, but these could not be exported: attachments, tests.json, summary.json, named attachments)`, exit 1 (as before) |

`short-run`, with `MOCK_XCRESULT` set to the C7 run's result bundle (13 tests, all
passed): `test runs: 2 passed of 2 finished, 13 expected; summary: 13 passed of 13 tests`,
then `FAILED (expected 13 passing test runs of 13 tests; the log shows 2 of 2 and the
summary 13 of 13)`, exit 1, as before.

## Open questions from the spike record

### The Automation Mode prompt, word for word

Captured 2026-11-16 on a fresh enablement. Before the run, `automationmodetool` said
"Automation Mode is disabled. This device requires user authentication to enable
Automation Mode." At 05:03:26, as `xcodebuild test` started the probe run
(`scripts/e2e-macos.sh --no-build --only testRunnerWritesOnlyItsScratch`), macOS showed
a system dialog with the Touch ID fingerprint icon badged "exec". The user, at the Mac,
copied it word for word:

> **XCTest is trying to Enable UI Automation.**
>
> Touch ID or enter your password to continue with XCTest.
>
> Buttons: **Use Password…** and **Cancel**

The user authorized it (with Touch ID or the password; which was not noted), and the run
went on and passed. This is the prompt the spike's run 1 saw but didn't capture. It is
macOS's authentication dialog for Automation Mode, raised for XCTest, not a TCC
(Accessibility) prompt, and nothing in System Settings was changed. Cancel leaves
Automation Mode disabled, so the UI tests can't drive the app. After the run,
`automationmodetool` again said "Automation Mode is disabled."

### How long an authorization lasts

**At least 28 minutes within one login session; longer gaps untested.** The user
authorized the prompt once, at about 05:03:26 on 2026-11-16. Ten more UI runs followed,
with `xcodebuild` starting about 10 s after each of 05:04:55, 05:10:25, 05:11:42, 05:12:57,
05:14:16, 05:15:31, 05:16:50, 05:18:42, 05:25:34, 05:30:14 and 05:31:05 (the C7 run, which
ended 05:41:57). The user, at the Mac throughout, reports the prompt never appeared again.
No run stalled: each reached its first test within seconds. Yet `automationmodetool`
said "Automation Mode is disabled" before and after every run. So the authorization is
cached apart from the mode itself: each session re-enables Automation Mode without asking
while the cache lasts.

Not measured, by the user's decision (2026-11-16): whether it lasts the whole login
session, or until a logout, a reboot or some timeout, and so whether a run started past
that point blocks. A run that prompts with nobody at the Mac would presumably wait on the
dialog (untested). If so, the script's `--timeout` watchdog ends it, with exit 124 and
the cleanup the stand-ins exercise. For an
unattended Mac or CI, `sudo automationmodetool enable-automationmode-without-authentication`
removes the prompt. That is the user's step (E2E-2 for CI), never the agent's.

### Widening the runner's entitlements

Yes, without changing the machine. Xcode 27 signs `TenmokuE2E-Runner.app` with the
runner template's entitlements merged with the test target's `CODE_SIGN_ENTITLEMENTS`.
A `build-for-testing` with `CODE_SIGN_ENTITLEMENTS=<file>` naming one
`com.apple.security.temporary-exception.files.absolute-path.read-write` entry gave a
runner signed with both (`codesign -d --entitlements - --xml TenmokuE2E-Runner.app`):

```
"com.apple.security.app-sandbox" => true
"com.apple.security.temporary-exception.files.absolute-path.read-only" => [ "/", "/" ]
"com.apple.security.temporary-exception.files.absolute-path.read-write" => [
  "/Users/daryl/code/fastedit/.worktrees/n5krsav71f/target/e2e-macos/scratch/" ]
```

So the script now writes `<target>/e2e-macos/TenmokuE2E.entitlements` with a single
read-write exception for **this run's scratch folder** (`<target>/e2e-macos/scratch/<time>/`)
and passes it to `xcodebuild test`. The runner stays sandboxed: it reads anywhere and
writes only its container and that folder, never the repository or the fixtures.
`testRunnerWritesOnlyItsScratch` checks both sides in every run: a write in the test's
set works, and one in the scratch root beside this run's folder is refused.

At runtime it holds. In the first UI run (`--only testRunnerWritesOnlyItsScratch`,
2026-11-16 05:03) the test's record reads:

```
inside …/target/e2e-macos/scratch/20261116-050317/testRunnerWritesOnlyItsScratch/001/home/runner-write-probe.txt: written
outside …/target/e2e-macos/scratch/runner-write-probe-001.txt: refused, Error Domain=NSCocoaErrorDomain Code=513 "You don’t have permission to save the file “runner-write-probe-001.txt” in the folder “scratch”." … {Error Domain=NSPOSIXErrorDomain Code=1 "Operation not permitted"}
```

and it passed in every run since, all three C7 iterations included.

What it unlocks: a test can now change a file behind the app's back, mid-test, which the
spike record put out of reach. For T400 (very large files), the external-edit step: grow
or replace the open file and check the app's reaction on disk and in the title. For T526
(hot exit, recovery, auto-save), the external-change checks: edit a file that has a
backup between quit and relaunch, or while it's open and dirty, and check what the
relaunch restores and what a save writes. Neither is in this card's suite scope; they
are for the cards that take those checks. It does not unlock T526's `kill -9` (the app
is launchd's child, not the runner's) or Finder drops.

## The real run

All on 2026-11-16, with the user at the Mac and the screen unlocked, after their go-ahead.
Every run used `--no-build`, because the release binary in the lane's target was built
from the card's code (only the Swift tests changed afterwards, and `xcodebuild test`
rebuilds those).

### Fix-up runs

| Time | Run | Result | What it showed |
|---|---|---|---|
| 05:03 | `--only testRunnerWritesOnlyItsScratch` | passed | The Automation Mode prompt (above); the widened runner works |
| 05:05 | the suite, 1 iteration | 2 of 13 | Every check of a gpui node failed: `waitForTree()` never saw `Split Right` in 15 s, while the snapshot taken right after held it |
| 05:10–05:16 | `--only testPaneToolbarEnabledState`, 5 times | 4 failed, 1 passed | Probes of the lookup (below); the fix |
| 05:19 | the suite, 1 iteration | 9 of 13 | Four expectations wrong about the app (below) |
| 05:26 | the suite, 1 iteration | 12 of 13 | cmd-S after the trust alert saved nothing |
| 05:30 | `--only testQuitByMenuHotExitAndRelaunch` | passed | |
| 05:31 | **`--iterations 3`** | **39 of 39** | C7, below |

**`.firstMatch` never finds gpui-pre's nodes.** The spike recommended
`app.buttons["Split Right"].waitForExistence`, but its runs only dumped the tree and never
queried it. A probe at the moment of failure gave:

```
app.buttons[m].exists = true
buttons(m).firstMatch.exists = false
buttons(m).count = 1
app.buttons.matching(title).firstMatch.exists = false
app.buttons.element(matching: title).exists = true
window.buttons.matching(title).count = 1
app.buttons.matching(title).firstMatch.waitForExistence(1) = false
```

So `Support.swift` resolves every gpui button with `element(boundBy: 0)` of a query
matching its title (gpui-pre's nodes have the label as the AX title and no
identifier), and `.firstMatch` is kept for AppKit's own elements (windows, menus, alerts,
the save panel), where it works. A `waitForExistence` also costs about 1.1 s even when the
element is already there, so each wait is preceded by an `exists` check.

**What the app does that the tests first assumed otherwise** (the tests now check what
it does):

- A file launch (`tenmoku <folder> <file>`) records the file in `recent.toml` but not its
  folder ("A file's folder is only the root it opens in, not one opened",
  `workbench.rs`); folders are recorded by folder launches.
- A file launch opens only the file; a folder launch (or one with no argument) reopens
  the root's saved windows, hot-exit backups included (`main.rs`). So the hot-exit
  relaunch is a folder launch, which on an undecided folder first raises the trust
  alert. The test answers Don't Trust.
- Open Preview shows the preview in the tab's place, and the window keeps the source
  tab's title, `●` included (`notes.md ● — Tenmoku`), rather than the preview item's
  `Preview notes.md — Tenmoku`.
- After the trust alert on that relaunch, cmd-S saved nothing: the restored editor
  didn't have keyboard focus. The test saves with File > Save All instead. **Possible
  bug, for a follow-up:** a window restored behind the launch's trust alert may not give
  its editor focus once the alert closes.

### C7: `scripts/e2e-macos.sh --no-build --iterations 3`

05:31:05 to 05:41:57, 10 min 52 s wall, ending:

```
test runs: 39 passed of 39 finished, 39 expected; summary: 13 passed of 13 tests
launch records: 36, of which 0 name a dev.tenmoku.tenmoku.e2e process other than <worktree>/target/e2e-macos/bundle/Tenmoku.app
result bundle: <worktree>/target/e2e-macos/results/20261116-053105/TenmokuE2E.xcresult
e2e-macos: passed
```

Every test passed in all three iterations, with no retries (the script passes no retry
option, and the log has exactly 39 finished runs). **Flakes: none.** The suite took 208.3,
207.2 and 203.4 s an iteration (`Executed 13 tests, with 0 failures`).

| Test | Iteration 1 | 2 | 3 |
|---|---|---|---|
| testFirstRunWizardOnce | 15.3 s | 14.3 s | 14.2 s |
| testMarkdownPreviewButtons | 15.9 s | 16.1 s | 15.7 s |
| testOpenRecentAndRecentToml | 15.5 s | 15.2 s | 15.0 s |
| testPaneToolbarEnabledState | 12.4 s | 11.6 s | 11.6 s |
| testQuitByMenuHotExitAndRelaunch | 22.6 s | 22.7 s | 22.4 s |
| testRunnerWritesOnlyItsScratch | 0.8 s | 0.6 s | 0.7 s |
| testSaveAllAndSaveAs | 24.9 s | 23.9 s | 24.0 s |
| testSaveEncodingsByteForByte | 22.7 s | 22.1 s | 22.0 s |
| testSaveWithCmdSAndFileMenu | 19.4 s | 18.8 s | 19.0 s |
| testTrustByMenu | 20.9 s | 20.7 s | 20.8 s |
| testVimClipboardOff | 7.5 s | 9.9 s | 7.2 s |
| testVimClipboardUnnamedplus | 7.1 s | 7.0 s | 7.1 s |
| testWindowTitleDirtyAndActive | 23.4 s | 24.4 s | 23.6 s |

**The user's Tenmoku and the installed copies were untouched.** All 36 launch records
(the 12 tests that launch the app, three times each) name only
`<worktree>/target/e2e-macos/bundle/Tenmoku.app/Contents/MacOS/tenmoku`. None notes a
`dev.tenmoku.tenmoku` process: the user's own Tenmoku wasn't running during the run.
`/Applications/Tenmoku.app/Contents/Info.plist` and
`~/Applications/Tenmoku.app/Contents/Info.plist` are both from Nov 9 15:45, before the
card. The vim tests put the pasteboard back as they found it.
