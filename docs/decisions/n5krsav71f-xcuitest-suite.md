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
| `testQuitByMenuHotExitAndRelaunch` | `plain` | Tenmoku > Quit Tenmoku with unsaved text: no question, `notes.txt` untouched, a backup holding the text, every backup file mode 0600; a relaunch on the same state restores the tab dirty (`notes.txt ● — Tenmoku`), and cmd-S writes the text | T526 (disk half) |
| `testFirstRunWizardOnce` | `plain`, fresh state | an empty state folder shows the wizard (`Skip setup`) and writes `outcome = "shown"`; Skip setup writes `"skipped"`; a relaunch shows nothing and leaves `setup.toml` byte for byte; Tenmoku > Set Up Tenmoku… shows it again, Escape skips | T1344 (disk half) |
| `testTrustByMenu` | `plain` | a folder launch asks "Do you trust the authors…" (Trust Folder, Don't Trust); Don't Trust, then File > Trust This Folder, Untrust This Folder and Trust again each land in `trust.toml` as the canonical folder's `true`/`false`; a relaunch doesn't ask again and leaves the file | T197 |
| `testOpenRecentAndRecentToml` | `recent` | `recent.toml` lists the folder and the file opened, then after a relaunch on another file lists that one first; File > Open Recent… with the query `quiz` and Return opens `quiz.txt` | T528 |
| `testVimClipboardOff` | `vim`, `settings/off.toml` | `vim.clipboard = ""`: `yyP` then `:w` puts vim's own register (disk `alpha alpha beta`) and leaves the pasteboard as the test set it; `"+yy` writes it | T361 |
| `testVimClipboardUnnamedplus` | `vim`, `settings/unnamedplus.toml` | `vim.clipboard = "unnamedplus"`: `yy` writes the pasteboard; text another app left there goes in with `P`, then `:w` (disk) | T361 |
| `testMarkdownPreviewButtons` | `markdown` | Open Preview and Open Preview to the Side show on the markdown tab only (gone on `notes.txt`); Open Preview swaps the tab to `Preview notes.md`, Show Source back, the source still `●` and the file untouched | T1008 |
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

`waitForTree()` waits for `app.buttons["Split Right"]` with `waitForExistence` (the pane's
tab-row button, there in every pane). Every check of a gpui node (`expectButton`,
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

SHORT-RUN

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

AUTOMATION-DURATION

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

RUNTIME-ENTITLEMENTS

What it unlocks: a test can now change a file behind the app's back, mid-test, which the
spike record put out of reach. For T400 (very large files), the external-edit step: grow
or replace the open file and check the app's reaction on disk and in the title. For T526
(hot exit, recovery, auto-save), the external-change checks: edit a file that has a
backup between quit and relaunch, or while it's open and dirty, and check what the
relaunch restores and what a save writes. Neither is in this card's suite scope; they
are for the cards that take those checks. It does not unlock T526's `kill -9` (the app
is launchd's child, not the runner's) or Finder drops.

## The real run

REAL-RUN
