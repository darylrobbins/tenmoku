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
| `testPaneToolbarEnabledState` | `lines` | Go Back and Go Forward (in the title bar since B3) disabled with no history; the tab row's Tab List, Split Right and More Actions enabled; a go-to-line jump enables Back, and Back and Forward swap their states; Split Right makes a second pane with its own Split Right and More Actions | (pane toolbar, record's last bullet) |
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
(`outcome = "skipped"`) and `whats_new.toml` (`seen = "<workspace version>"`, so the What's
New tab T1946 added stays away; the seed came with main's T1946 change and is kept here)
unless the entry says `fresh: true`, and `home/`. A fresh state is a new install, for
which the app records What's New as seen itself. Each test checks
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

Fixed, in two rounds.

1. Every recorded process that is still the same process (alive, not a zombie, with
   the start time recorded) is itself a root, so each tracker pass and each cleanup pass
   walk down from it too.
2. Review round 1 (finding F1 of T1682's review) found two shutdown cases the first
   round missed. A recorded process may handle TERM by starting a child and exiting
   before the next scan, so no root leads to the child. And a child started just before
   the KILL, whose parent is then killed, is never seen. The script now:
   - owns every **process group led by an owned process**: a child stays in its parent's
     group when the parent exits, so the TERM-time child is still found;
   - **freezes before signalling**: before TERM, and before every KILL (the watchdog's
     included), it sends SIGSTOP to every owned process and scans again until a pass
     finds nothing new. A stopped process starts nothing and orphans nothing, so what
     gets TERM or KILL is the whole tree as it stands. TERM is followed by CONT;
   - scans with **one `ps -A` snapshot**, worked through in awk, instead of a `pgrep`
     and a `ps` per process. That made the 0.2 s scans during the grace period
     affordable. The first `spawn-loop` run took 149 s on the per-process scans. The
     grace period is measured by the clock, not by passes, because one stand-in run on
     the loaded Mac (load average 17, other lanes building) took 451 s with
     pass-counted waits.

3. Review round 2 (finding F4) showed that a recorded group was trusted by its bare id.
   Once the group emptied and the id was reused, an unrelated group would have been
   adopted and signalled. Each group is now recorded as "pgid leader-start-time",
   xcodebuild's own included (recorded as it starts, in place of the old bare
   `$run_pid` match). It stays owned only while it has members and no live process
   holding its id started at another time. A group that fails either test is retired
   for good, so a reused id is never adopted. A child left in the original group after
   its leader exits is still found. `tools/e2e-macos/standins/scan-cases` runs the
   script's own scan on synthetic `ps` snapshots for each of these cases (results
   below). The watchdog still signals xcodebuild's group by id, but only while
   xcodebuild is unreaped, when its id can't be reused.

4. Review round 3 found two more gaps, and the card was respecced (C5, rev 2) to anchor
   the groups:
   - **F4:** round 2's check only rejected a reused id while the new leader was still
     alive. If that leader had been reaped, or was a zombie (whose identity the scan
     dropped), the unrelated group's members were adopted.
   - **F6:** xcodebuild's group was registered by reading its start time after it
     started. A command that leaves a child and exits before that read was never
     registered, and the empty result was silently skipped.

   The script now runs xcodebuild under an **anchor**, a shell started in a group of its
   own. Before xcodebuild starts:
   - `run_watched` reads the anchor's pid and start time and checks that it leads its
     group. An empty result is an error: the run fails with "can't read the identity of
     the run's anchor" and xcodebuild never starts.
   - `run_watched` records the anchor's group, then acknowledges it. The anchor polls for
     the acknowledgement and only then runs the command in the foreground, in its own
     group.

   When the command ends, the anchor writes its exit status and `exec`s a `sleep`. That
   keeps it the same process (pid and start time), holding the group's id until
   `cleanup_run` kills it last.

   Scans, cleanup and the watchdog now treat groups this way:
   - **While the anchor lives**, every member of xcodebuild's group is owned, its parent
     gone or not, because the group's id can't be reused.
   - **Any other group** an owned process leads is owned only while a process with its
     id, alive or a zombie, has the recorded start time. Zombies are never signalled,
     but their identity counts, so a reused id is rejected whether the new leader is
     alive, a zombie or reaped.
   - **The anchor** is never in a scan's output. The watchdog signals the group's
     members but not the anchor, and waits on the command's status rather than on the
     group.

   Unanchored groups (the runner's and the app's, started by testmanagerd and launchd)
   have no such guarantee. A child made on TERM in one of them would be lost once its
   leader was reaped, so cleanup sends TERM (with the 10 s grace) only to xcodebuild's
   group. Every other owned process gets KILL while still frozen, and a frozen process
   killed outright starts nothing. A recorded member of an unanchored group whose leader
   is reaped stays owned by its own identity.

5. Review round 4 (F7) found that the anchor published the exit status with an external
   `mv`. That `mv` ran inside the anchored group, so the watchdog's TERM, or a freeze and
   KILL, could catch it as the command finished near the timeout. Without the status,
   the anchor still became its sleep, and the wait never ended. Now:
   - From the command's end to its `exec sleep`, the anchor runs only builtins. It writes
     the status with one `printf`, which nothing the script signals can interrupt, since
     the anchor itself is never signalled.
   - The wait ends when the status is published (a non-empty file), when the anchor has
     gone or become its sleep without one ("the run's anchor wrote no exit status",
     exit 1), or when the watchdog has finished its TERM, grace and KILL (exit 124).
     So no failed or paused publication can hold the run.

   `tools/e2e-macos/standins/status-cases` tests this. A test-only `E2E_ANCHOR_HOLD`
   pauses the status write while the watchdog fires, and `bad-status` makes the write
   fail (results below).

Cleanup: freeze; KILL the owned processes outside xcodebuild's group; TERM and CONT its
members; scan every 0.2 s for up to 10 s; then freeze and KILL what is left; then up to
three more scans, each freezing and killing anything a killed process left; then KILL
the anchor.

**What it still can't follow:** macOS has no subreaper. Two cases are out of reach:
- a process that, between two scans, starts a child in a new group or session of its
  own and then exits, leaving that child neither its descendant nor in an owned group;
- a child left in an unanchored group by a leader that exits and is reaped between two
  scans. During cleanup this can't arise, because those processes are killed frozen.

No stand-in covers these, because the script can't catch them. Neither the runner, the
app nor xcodebuild's helpers do either.

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

**Before review round 1's fix** (the script at `b611b40d6`), `term-spawner`, `MODE=hang`,
`--timeout 4`: the TERM-time child was left running.

```
e2e-macos: timed out after 4s; stopping xcodebuild, the runner and the app
stand-in: spawn-on-term got TERM, started pid 16727 and exits
e2e-macos: FAILED (timed out after 4s)
exit 124; pgrep afterwards: 16727 /bin/sh …/tools/e2e-macos/standins/late-child
```

**With the round-3 script** (2026-11-18 00:00: the anchor, and KILL outside
xcodebuild's group). Every run ended with the expected status, and nothing was
left on any of them: `pgrep -fl 'e2etest|standins/|helper-resist|helper-orphan'` found
none each time. Four runs are quoted from a rerun:
- two whose pgrep in the first pass also listed my own shell commands running alongside,
  whose text contained `standins/`;
- `late-orphan`'s SIGINT and SIGTERM runs, signalled at 10 s, because at load average
  40 to 80 late-child hadn't started by 7 s.

`term-spawner` no longer prints its TERM-time line: its runner is outside xcodebuild's
group, so it gets KILL while frozen and never runs its TERM handler.

| Stand-in | `--timeout` | `MODE=exit` | SIGINT | SIGTERM |
|---|---|---|---|---|
| `quick-exit` (r3 F6: leaves a child, exits at once) | `MODE=hang`: `a sub-command left quick-child pid 89262 and exits`, `sending KILL to: 89262 …`, `FAILED (timed out after 4s)`, **124** | `quick-exit left quick-child pid 93891 and exits`, KILL to it, `FAILED (xcodebuild exited 0, …)`, **1** | `MODE=exit`, at 3 s, during cleanup: pid 96434 killed, `FAILED (SIGINT)`, **130** | `MODE=exit`, at 3 s: pid 37275 killed, `FAILED (SIGTERM)`, **143** |
| `late-orphan` (F1: orphan's late child) | `started late-child pid 1987`, **124** | `started late-child pid 40214`, **1** | at 10 s: `started late-child pid 42288`, `FAILED (SIGINT)`, **130** | at 10 s: `started late-child pid 44282`, `FAILED (SIGTERM)`, **143** |
| `term-spawner` (a runner that spawns on TERM) | `FAILED (timed out after 4s)`, **124** | **1** | `FAILED (SIGINT)`, **130** | `FAILED (SIGTERM)`, **143** |
| `spawn-loop` (new-session children up to the KILL) | **124** | **1** | **130** | **143** |
| `family` (the spike record's four runs) | `--timeout 4`, **124** | **1** | **130** | **143** |

The spike record's other stand-ins:
- `hang-ignore-term`, `--timeout 3`: `FAILED (timed out after 3s)`, exit 124 after 20 s.
- `exit-zero`: `FAILED (xcodebuild exited 0, but these could not be exported: …)`, exit 1.
- `spawn-loop` signalled at 1 s: `FAILED (SIGTERM, before the run)`, exit 143, and
  `FAILED (SIGINT, before the run)`, exit 130.

**Review round 3, F4 with real processes.** `group-reuse` runs the script's own
`cleanup_run` after forging a record of a group whose id now belongs to another group.
With the round-2 script, both reuse cases killed the unrelated process:

```
FAIL the id reused, the new leader reaped: leader 99486 [], bystander 99487 in group 99486; recorded "99486 Mon Jan 1 00:00:00 2024"; bystander killed, expected survives
FAIL the id reused, the new leader a zombie: leader 99891 [Z], bystander 99892 in group 99891; recorded "99891 Mon Jan 1 00:00:00 2024"; bystander killed, expected survives
ok   control: the recorded leader a zombie, the group still ours: leader 400 [Z], bystander 402 in group 400; recorded "400 Tue Nov 17 23:58:57 2026"; bystander killed
group-reuse: 2 failed
```

With the final script:

```
ok   the id reused, the new leader reaped: leader 97173 [], bystander 97175 in group 97173; recorded "97173 Mon Jan 1 00:00:00 2024"; bystander survives
ok   the id reused, the new leader a zombie: leader 97344 [Z], bystander 97345 in group 97344; recorded "97344 Mon Jan 1 00:00:00 2024"; bystander survives
ok   control: the recorded leader a zombie, the group still ours: leader 97527 [Z], bystander 97528 in group 97527; recorded "97527 Tue Nov 17 23:58:41 2026"; bystander killed
group-reuse: all passed
```

**Review round 3, F6.** Codex reproduced the race with a synthetic snapshot. I couldn't
reproduce it on demand from a shell stand-in: in both `quick-exit` runs against the
round-2 script, the script read xcodebuild's start time before xcodebuild exited, and
cleaned up. With the anchor the race can't occur, because the identity is read while the
anchor waits for its acknowledgement. An unreadable identity is now an error. With
`tools/e2e-macos/standins/no-identity` first on `PATH` (a `ps` that returns no start
time), a `quick-exit` run printed the lines below. There was no `stand-in:` line, because
the stand-in never started, and pgrep found nothing.

```
e2e-macos: can't read the identity of the run's anchor (pid 5540, start '', group '5540'); not starting xcodebuild
e2e-macos: FAILED (xcodebuild test exited 1)
exit 1
```

A signal that arrives before xcodebuild starts, during the build, bundle or xcodegen
steps, has nothing to stop. One harness run sent SIGTERM at 3 s, while the loaded Mac was
still bundling, and the shell died of it outright (exit -15, nothing launched). The
script now traps INT and TERM from its first lines: `spawn-loop` signalled at 1 s ended
`FAILED (SIGTERM, before the run)` with exit 143, and `FAILED (SIGINT, before the run)`
with exit 130. Nothing was left, and its temporary state and config folders were removed.

**Review round 4, F7, with the final script** (2026-11-18 01:20). `status-cases`:

```
     stand-in: quick-exit left quick-child pid 72255 and exits
     e2e-macos: timed out after 4s; stopping xcodebuild, the runner and the app
     e2e-macos: FAILED (timed out after 4s)
ok   status write paused while the watchdog fires: exit 124; nothing left, nothing stopped
     stand-in: bad-status blocked the status file and exits
     e2e-macos: the run's anchor wrote no exit status
     e2e-macos: FAILED (xcodebuild test exited 1)
ok   status write fails: exit 1; nothing left, nothing stopped
status-cases: all passed
```

Against the round-3 script, the paused case can't pause, because that script has no
`E2E_ANCHOR_HOLD`: it exited 1, not 124. The failed-write case ended with exit 1 there
too, because that anchor exited on the failed write instead of becoming its sleep.
Neither case reproduces codex's exact race (the watchdog catching the `mv`). Codex's
read-only probes are the before-fix evidence. Both cases cover the new paths: the wait
released by the watchdog, and an anchor that became its sleep without a status.

The whole matrix above was rerun on the final script, and every run ended as in the
table: 124, 1, 130 and 143 for each four-path stand-in, 124 for `hang-ignore-term`, 1 for
`exit-zero`, and 143 and 130 for signals before the run. pgrep found nothing each time.
At load average 30 to 45 the script's setup took up to 8 s, so three signal runs were
repeated with later signals, to land after the stand-in had started:
- `quick-exit` SIGINT at 10 s: `left quick-child pid 87410`, `sending KILL to: 87410 …`,
  `FAILED (SIGINT)`, 130;
- `quick-exit` SIGTERM at 7 s: pid 77454 killed, 143;
- `late-orphan` SIGINT and SIGTERM at 14 s, after `started late-child pid 80136` (and
  `82252`): 130 and 143.

`scan-cases` and `group-reuse` passed again. `group-reuse` now fails, rather than ending
silently, if a case stops early. It had stopped early once, because `cleanup_run` named a
variable the stand-in didn't set.

`scan-cases`, the script's own scan on synthetic snapshots (review F4, rounds 2 and 3).
With the round-2 script, the reaped and zombie reuse cases adopted the unrelated pid 601,
and the anchor was listed:

```
ok   a matched leader and its group: owned [500 501]
     recorded groups: 500 Mon Nov 17 10:00:00 2026
ok   the leader a zombie, its group kept: owned [501 502]
FAIL the leader reaped, only recorded members kept: owned [501 503], expected [501]
     retired: 
FAIL the id reused, the new leader reaped: owned [601], expected []
     retired: 
FAIL the id reused, the new leader a zombie: owned [601], expected []
ok   the id reused, the new leader alive: owned []
FAIL the anchored group, a member's parent gone: owned [400 401 402], expected [401 402]
ok   pid identity by start time: owned [701 702]
ok   the app by its path, taken literally: owned [800]
scan-cases: 4 failed
```

With the final script:

```
ok   a matched leader and its group: owned [500 501]
     recorded groups: 500 Mon Nov 17 10:00:00 2026
ok   the leader a zombie, its group kept: owned [501 502]
ok   the leader reaped, only recorded members kept: owned [501]
     retired: 500 Mon Nov 17 10:00:00 2026
ok   the id reused, the new leader reaped: owned []
     retired: 600 Mon Nov 17 10:00:00 2026
ok   the id reused, the new leader a zombie: owned []
ok   the id reused, the new leader alive: owned []
ok   the anchored group, a member's parent gone: owned [401 402]
ok   pid identity by start time: owned [701 702]
ok   the app by its path, taken literally: owned [800]
scan-cases: all passed
```

`short-run`, with `MOCK_XCRESULT` set to the C7 run's result bundle (13 tests, all
passed): `test runs: 2 passed of 2 finished, 13 expected; summary: 13 passed of 13 tests`,
then `FAILED (expected 13 passing test runs of 13 tests; the log shows 2 of 2 and the
summary 13 of 13)`, exit 1, as before. With the final script it also reports `two
attachments named window-title-testFirstRunWizardOnce-001`, because that bundle predates
the record numbering (F2). The copy now refuses names that clash, and the run fails with
`could not be exported: named attachments`, exit 1.

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

A later run on the final script started at 11:52:43, 6 h 49 min after that
authorization. It reached its first test 30 s after the script started, no slower than an
unprompted start, so it didn't wait on a password. Whether the prompt appeared and was
answered at once is **unanswered so far** (asked, not yet known).

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

The path is XML-escaped (`&`, `<`, `>`) before it goes into the plist (review finding
F3), and the script checks the file with `plutil -lint`, then that PlistBuddy reads back
exactly the scratch path, or it stops. The generation step, given a scratch path holding
`a&b <c>`, gives a valid plist granting that path. The `build-only` stand-in ran the
script with `CARGO_TARGET_DIR=<tmp>/t&x` (a `release/tenmoku` link to the lane's binary).
xcodegen, a real `xcodebuild build-for-testing` with the script's
`CODE_SIGN_ENTITLEMENTS` and the signing all worked, and the runner came out signed with
the exception `<tmp>/t&amp;x/e2e-macos/scratch/20261116-075924/` (`&` as XML gives it).

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

### After review round 1: one flake under load

The script changed in review round 1 (cleanup, record numbering, the launch audit). So
`scripts/e2e-macos.sh --no-build --iterations 3` was run again on the final code at 11:52:43,
with the user's go-ahead and the user at the Mac. It ended `FAILED (xcodebuild test exited
65)`:

```
test runs: 38 passed of 39 finished, 39 expected; summary: 12 passed of 13 tests
launch records: 48, of which 0 name a dev.tenmoku.tenmoku.e2e process other than <worktree>/target/e2e-macos/bundle/Tenmoku.app, or none
```

- **The flake, in order:** iterations 1 and 2 passed all 13. In iteration 3, 12 passed and
  `testWindowTitleDirtyAndActive` failed (`no window titled "b.txt ● — Tenmoku": ["b.txt —
  Tenmoku"]`, SmokeTests.swift:140). Its `typeText("y")`, just after a cmd-Z, spent 11.1 s in
  XCTest's "Synthesize event" (12:04:02.0 to 12:04:13.1), against a few hundredths of a second
  elsewhere, and the key never reached the app: `b.txt` was unchanged on disk and the title
  stayed clean. The app's log shows nothing unusual. The Mac was at load average 26 to 35
  (other lanes building), against 11 at 08:17, and every test ran slower than at 05:31
  (`testWindowTitleDirtyAndActive` 23.6, 27.0 and 31.3 s). So the likely cause is load starving
  event synthesis, which is not proven. The test doesn't retype on a failure: a retry would
  hide flakiness, and C7 allows none.
- **What it showed of the final script:** the numbered records exported without a clash.
  The audit counted 48 launch records, the four relaunch tests' second launches included,
  all naming only the e2e bundle. Cleanup and the exports worked.
- A clean three-iteration run on the final script waited for the user's next session at the
  Mac (the lead's decision, 2026-11-16).

### 2026-11-17: on main as of v0.2.2, rebuilt

The branch was rebased onto main (296 commits past the binary the earlier runs used,
the B3 chrome included), and the release binary was rebuilt from it (17:40 to 17:55).

- **17:55, `--iterations 3`: 24 of 26 finished** (the third iteration didn't run after
  a failed one). `testPaneToolbarEnabledState` failed in both iterations, the same way:
  `no button Split Down in the tree`. The B3 tab strip's row is now Tab List, Split
  Right and More Actions. Split Down moved to the pane's menu, and Go Back and Go
  Forward moved to the title bar, where their enabled-state checks passed. The test now
  checks the new row (commit `50ed563cb`). The other 12 tests passed both times.
- **18:15, `--only testPaneToolbarEnabledState`: 1 of 1 passed** (13.1 s).
- **18:16, `--iterations 3`: 38 of 39.** Iterations 1 and 2 passed all 13 (210.6 and
  213.5 s). In iteration 3, `testSaveEncodingsByteForByte` failed on its last file:
  `crlf.txt is 6c 69 6e 65 … (unchanged), expected 65 32 65 0d 0a …`. The typing had
  landed (the title read `crlf.txt ● — Tenmoku`), but XCTest's "Synthesize event" for the
  cmd-S that followed took 10.25 s (t = 19.67 to 29.92 s, against about 0.03 s for every
  other key), and the save never came. The app's log shows nothing. The load average was
  15 to 21, and `testSaveAllAndSaveAs` also ran slow in that iteration (34.5 s against
  24). The 48 launch records all name the e2e bundle.

### C7: the clean run on the final code

`scripts/e2e-macos.sh --no-build --iterations 3`, 2026-11-17 19:04:25 to 19:15:07, with
the user at the Mac and the other lanes' finishes held (load average 8.0 at the start,
13.3 at the end). The binary was the 17:55 release build of the branch rebased on main
(v0.2.2). Only the Swift tests and these docs changed after it. The run ended:

```
test runs: 39 passed of 39 finished, 39 expected; summary: 13 passed of 13 tests
launch records: 48, of which 0 name a dev.tenmoku.tenmoku.e2e process other than <worktree>/target/e2e-macos/bundle/Tenmoku.app, or none
result bundle: <worktree>/target/e2e-macos/results/20261117-190425/TenmokuE2E.xcresult
e2e-macos: passed
```

Every test passed in all three iterations, with no retries and no flakes. The suite took
207.5, 208.7 and 208.0 s an iteration.

| Test | Iteration 1 | 2 | 3 |
|---|---|---|---|
| testFirstRunWizardOnce | 16.0 s | 15.3 s | 15.3 s |
| testMarkdownPreviewButtons | 16.2 s | 15.9 s | 16.0 s |
| testOpenRecentAndRecentToml | 15.3 s | 15.6 s | 15.4 s |
| testPaneToolbarEnabledState | 11.7 s | 12.0 s | 11.9 s |
| testQuitByMenuHotExitAndRelaunch | 23.1 s | 24.5 s | 23.7 s |
| testRunnerWritesOnlyItsScratch | 0.5 s | 0.6 s | 0.7 s |
| testSaveAllAndSaveAs | 24.0 s | 23.9 s | 24.9 s |
| testSaveEncodingsByteForByte | 22.2 s | 22.2 s | 22.1 s |
| testSaveWithCmdSAndFileMenu | 18.9 s | 19.2 s | 19.1 s |
| testTrustByMenu | 21.0 s | 21.1 s | 20.7 s |
| testVimClipboardOff | 7.2 s | 7.2 s | 7.2 s |
| testVimClipboardUnnamedplus | 7.4 s | 7.7 s | 7.3 s |
| testWindowTitleDirtyAndActive | 23.8 s | 23.5 s | 23.7 s |

**The user's Tenmoku and the installed copies were untouched.** All 48 launch records name
only the e2e bundle's executable. That is 36 first launches plus the 12 second launches of
the wizard, trust, Open Recent and hot-exit tests (`launched-<test>-<iteration>-2`), and the
script's audit checked every one. None notes a `dev.tenmoku.tenmoku` process, so the user's
Tenmoku wasn't running. `~/Applications/Tenmoku.app/Contents/Info.plist` is from Nov 9
15:45, before the card. `/Applications/Tenmoku.app/Contents/Info.plist` is from Nov 16
05:56, after the card started but outside every UI run (Nov 16 05:03 to 05:42 and 11:52 to
12:05, Nov 17 17:55 to 19:15), so it is the user's own install, not the suite's. The script
never names `/Applications`, and no test launches by bundle id.

**The two flakes share one signature.** The 11:52 run on 2026-11-16 (a `typeText` after a
cmd-Z) and this one (a cmd-S): an event synthesis stalls about 10 to 11 s, then the key is
lost. That is two in 117 test runs on the final code, both under load averages of about
15 to 30. No run at the 05:31 load stalled. The cause looks to be on XCTest's side, its
event synthesis under load, not the app's. The suite doesn't hide it with retries.

### The first 39 of 39: `--iterations 3` before review round 1 (2026-11-16)

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

**The user's Tenmoku and the installed copies were untouched.** The 36 exported launch
records name only `<worktree>/target/e2e-macos/bundle/Tenmoku.app/Contents/MacOS/tenmoku`.
That is one per launching test and iteration. Review finding F2 showed that the four
relaunch tests' second launch records (12 in all) shared a name with their first, and
only one copy survived the export. Each of those launches was still checked in the
test itself: `recordLaunch()` asserts that the running `dev.tenmoku.tenmoku.e2e` is the
bundle under test, and every test passed. Records are now numbered per kind
(`launched-<test>-<iteration>-<n>`), the copy refuses two attachments with one name, and
the script fails a run if any launch record names no process or another one. The audit
compares the executable path as a fixed string (`grep -F`, review finding F5), and so
does `--only`'s check. With the `fake-launch` stand-in under `CARGO_TARGET_DIR=<tmp>/build[1]`,
it reported `launch records: 2, of which 0 name … other than '<tmp>/build[1]/…/Tenmoku.app'`.
With `MODE=other` it reported `3, of which 1` and named the `/Applications` record. The
old regex `grep` counted both correct records as other processes. None notes a
`dev.tenmoku.tenmoku` process: the user's own Tenmoku wasn't running during the run.
`/Applications/Tenmoku.app/Contents/Info.plist` and
`~/Applications/Tenmoku.app/Contents/Info.plist` are both from Nov 9 15:45, before the
card. The vim tests put the pasteboard back as they found it.
