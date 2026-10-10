---
task_uid: "01m4knbe5jh80mg8jy1f5v3vpp"
task: "T334"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "ConfigWatcher re-stats each file's symlink chain every RECONCILE (3 s) and reports every file whose fingerprint changed since it was last reported, recording fingerprints before the sink reads; tests::watch_reconcile::a_file_in_a_directory_made_later_is_reported fails at base and passes"
    required: true
    manual: false
  - id: "C2"
    text: "An idle ConfigWatcher sends no batch when nothing changed (tests::watch_reconcile::an_unchanged_file_is_not_reported_again)"
    required: true
    manual: false
  - id: "C3"
    text: "After a burst of at least 1,000 distinct paths, ProjectWatcher sends [FsChange::Rescan] once the stream has been quiet for 2 s, 15 s and 60 s (overridable in tests through tests::watcher_recovery::SOURCE); tests::watcher_recovery::a_burst_is_followed_by_rescans fails at base and passes"
    required: true
    manual: false
  - id: "C4"
    text: "Fewer than 1,000 paths cause no Rescan (tests::watcher_recovery::a_small_batch_is_not_followed_by_a_rescan), and the ProjectWatcher and ConfigWatcher docs state the recovery and its known limits"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-project -p fe-config passes, with crates/fe-project/src/tests/watcher.rs, crates/fe-config/src/tests/watch.rs, crates/fe-config/src/tests/watch_late.rs and .config/nextest.toml unchanged"
    required: true
    manual: false
---

# Recover from FSEvents' silent event loss: ConfigWatcher re-stats its files every 3 s, ProjectWatcher rescans after a burst settles

# Recover from FSEvents' silent event loss in ProjectWatcher and ConfigWatcher

## Problem

FSEvents silently drops events when something writes many files. T331's worker found this
(commit 1d4250d, "Implementation notes" in `docs/specs/xycnrbgaz7-flaky-under-load-fe-project-tests.md`).
They ran an unfiltered Swift probe (`FSEventStreamCreate` over one fresh directory with
`FileEvents | NoDefer | UseCFTypes` and latency 0) that wrote one sentinel a second for 70 s. Meanwhile
fe-project's `tests::large::a_100k_file_tree_loads_lazily` and `tests::finder::a_100k_file_index_bounds_query_work`
wrote about 100k files elsewhere. Sentinels 1–4 were reported. Sentinels 5–31 (written 4.6–33 s) were
never reported in any form, and no event carried `MustScanSubDirs`, `UserDropped` or `KernelDropped`.
Delivery resumed at 36 s, and a few later sentinels were lost too. In the same run, `ConfigWatcher` never
reported the test's rewrites of `a/settings.toml` or a project edit, and no rescan followed.

Our code doesn't lose events: T331 checked `Batch::add` and `Tracker::batch`. The problem is that nothing
recovers from a loss FSEvents doesn't flag:

- `crates/fe-project/src/watcher.rs`: `ProjectWatcher` emits `FsChange::Rescan` only for `need_rescan()`
  or an error. After a `git checkout`, a build or `npm install` in the project, the tree, quick open and
  the symbol index can stay stale until something else touches the same paths.
- `crates/fe-config/src/watch.rs`: `ConfigWatcher` reports a file only when an event names its chain.
  A settings or keymap save made during such a burst is ignored until the next save.

## Change

ConfigWatcher and ProjectWatcher need different fixes. Config files are few and cheap to stat. A project
can hold 100k files, and a `Rescan` walks the whole root in the finder and in the symbol index.

### ConfigWatcher: re-stat the files every 3 s

1. In `Tracker`, keep a fingerprint for each file: for each path in its chain, `symlink_metadata`'s
   (dev, ino, file type, len, mtime ns, ctime ns), or "missing" or "error".
2. Add `Tracker::reconcile(&mut self, watch) -> Vec<PathBuf>`. It recomputes every fingerprint. It treats
   each file whose fingerprint changed exactly as an event naming that file: re-resolve its chain, register
   any new directory through `register` (which reports every file, as `batch` does), and return the files
   in order. Both `Tracker::new`/`register` and `Tracker::batch` record the fingerprints of the files they
   report before returning, so the fingerprints are always taken before the sink reads the file. A later
   change then always differs from the recorded fingerprint, and a file an event already reported isn't
   reported again.
3. In `ConfigWatcher::with_hook`'s thread, wait with `rx.recv_timeout(RECONCILE)` (`const RECONCILE:
   Duration = 3 s`). On a timeout, run `reconcile` (with the same `watch` closure `batch` uses) and pass
   any non-empty result to the sink. The thread still ends on `Disconnected`. Fingerprints for the first
   batch are taken before `new` returns.
4. This also picks up a config directory created after launch: a file that was missing changes
   fingerprint, and `register` then watches its new directory. Update `ConfigWatcher`'s doc comment, the
   `Tracker::new_dirs` log text ("creating it takes a restart…") and `Tracker::batch`'s doc to say so.
   Also say that changes no event reports are picked up within `RECONCILE`. Don't edit
   `crates/fastedit/src/main.rs`: T223 and T245 hold it, and its `create_dir_all` stays harmless.

### ProjectWatcher: rescan after a burst settles

5. In `ProjectWatcher`'s thread, count the distinct paths delivered since the stream was last quiet for
   2 s. When the count reaches `BURST` (1,000), the watcher is "after a burst". After a burst, it sends
   the sink `vec![FsChange::Rescan]` once the stream has been quiet for 2 s, 15 s and 60 s (`SETTLE:
   [Duration; 3]`, measured from the last event received). Any event in between restarts the quiet clock
   and keeps the burst state. The state clears after the 60 s rescan. With no burst pending, the thread
   blocks in `recv` as now. Batches and their dedupe are unchanged.
6. Why this schedule: during a flood, the loss window looks like quiet, so the 2 s rescan can land
   mid-flood. In T331, delivery resumed about 3 s after the flood's last write, and the last event
   delivered was about 30 s before that. The 60 s rescan covers a loss window that outlasts the last
   delivered event by up to a minute. A rescan costs one walk in each consumer (`FinderIndex::apply`,
   `SymbolIndex::plan`, which re-extracts only files whose length or mtime changed) and a re-list of the
   tree's loaded directories.
7. Document the known limits on `ProjectWatcher`. A flood outside the root that drops this root's events
   isn't detected, and neither is a loss window more than 60 s past the last delivered event.

### Test seams (all new tests must build against the base API: Regatta overlays `src/tests/**` at base)

8. fe-project: `crates/fe-project/src/tests/watcher_recovery.rs`, declared `pub(crate) mod
   watcher_recovery;` in `crates/fe-project/src/tests/mod.rs`. It defines the hook that src reads:
   `pub(crate) struct Source { pub(crate) events: mpsc::Receiver<notify::Result<notify::Event>>,
   pub(crate) settle: [Duration; 3] }` and `thread_local! { pub(crate) static SOURCE:
   Cell<Option<Source>> }`. Under `#[cfg(test)]`, `ProjectWatcher::new` takes `SOURCE` on the calling
   thread. If it's set, the thread reads `events` instead of notify's receiver and uses `settle` instead of
   `SETTLE`. The thread ends when `events` disconnects. At base nothing reads the hook, so the synthetic
   events never reach the watcher.
   - `a_burst_is_followed_by_rescans` (the regression test): settle `[200 ms, 600 ms, 1 s]`. Send 1,000
     distinct `Create` events. The sink gets batches naming exactly those paths, then three `[Rescan]`
     batches in order, none sooner than its settle period after the last event, and nothing else within
     2 s of the last one. Use generous upper deadlines (10 s each) so load can't fail it. At base no
     `Rescan` arrives, and it fails at the first deadline.
   - `a_small_batch_is_not_followed_by_a_rescan`: 999 events, then no `Rescan` within 2 s.
9. fe-config: `crates/fe-config/src/tests/watch_reconcile.rs`, declared `mod watch_reconcile;` in
   `crates/fe-config/src/tests/mod.rs`. No real FSEvents loss can be made on demand, and every change in a
   watched directory is delivered at base (a write through a hard link outside it is reported under the
   watched name, checked with a Swift probe on 2026-10-10). So the test makes a change that gets no event
   at all, using only the public API:
   - `a_file_in_a_directory_made_later_is_reported` (the regression test): `ConfigWatcher::new` over
     `<tmp>/missing/settings.toml` with `missing/` absent. Take the first batch. Create `missing/` and
     write the file. Assert that a batch naming it arrives within 30 s, and that a `SettingsStore` reload
     applies its value. At base the directory is never watched, so no batch comes.
   - `an_unchanged_file_is_not_reported_again`: after the first batch, no batch within 2 × `RECONCILE`
     (7 s) while nothing changes.
10. Leave `crates/fe-project/src/tests/watcher.rs`, `crates/fe-config/src/tests/watch.rs`,
    `watch_late.rs` and `.config/nextest.toml` alone (T331 holds the first, second and last). The
    existing config tests loop over extra batches, so an extra reconcile batch is fine.

### Alternatives not taken

- Periodic project `Rescan` on a timer: sound, but it walks up to 100k files per tick in two consumers.
- Re-stat the loaded directories' mtimes on a timer: sound for the tree's listings, but the watcher would
  need to know each consumer's directories (the tree, the finder, the symbol index in fe-workbench).
- A canary heartbeat (a private sentinel file written every few seconds, with a rescan when one goes
  missing): detects floods anywhere. But it writes a file forever, and it assumes in-order loss, which
  T331's sporadic later losses (sentinels 35–62) contradict.
- FSEvents history replay from the last event id (`sinceWhen`): notify doesn't expose it, and nobody has
  checked whether fseventsd's log keeps dropped events.
- A burst trigger for config: config floods come from elsewhere (`npm install` in the project), so the
  config watcher's own non-recursive directories wouldn't see them.

## Implementation notes

- **ConfigWatcher re-stats on a deadline, not on an idle timeout.** Each loop turn first runs the
  re-stat if it's due, and otherwise waits with `recv_timeout` until it is; draining queued events also
  stops at the deadline. The deadline moves only when a re-stat runs. A plain `recv_timeout(RECONCILE)`
  would never time out under a steady stream of events in a watched directory, so a lost change there
  could stay lost (codex review F1).
- **fe-config has a test seam too**: `tests::watch_reconcile::SOURCE`, a receiver that `with_hook`
  reads instead of notify's, so `watch_reconcile` is declared `pub(crate) mod`. Real events in a
  watched directory can't stand in for a starved re-stat: every `Tracker::batch` registers any directory
  that appeared, so the missing-directory change is found by whichever event comes next.
  `a_change_amid_unrelated_events_is_reported` feeds unrelated synthetic events every 10 ms and rewrites
  the tracked file, which the watcher then hears nothing about; it fails within 30 s under a loop that
  pushes the re-stat back on every event.
- **A file's fingerprint resolves its chain afresh** and holds each path with its stat, so a link
  retargeted without any stat on the old chain changing still counts. `Tracker::batch` and
  `Tracker::reconcile` share one `report` step (re-resolve the changed chains, register, take the
  reported files' fingerprints). Files a batch doesn't report keep their old fingerprints, so a change
  whose event was lost is still found by the next re-stat.
- **`an_unchanged_file_is_not_reported_again` lets early batches through.** FSEvents reported the
  fixture's own writes (`Create(File)`, `Modify(Data)` on `settings.toml`, made before the watcher
  started) to the new stream in about 1 of 3 parallel runs, which batched the file once more. The test
  instead requires a 7 s window with no batch to begin within 30 s: a re-stat that reported the
  unchanged file every 3 s never allows one.
- **The burst count uses `settle[0]` as its quiet window**, so the tests' 200 ms applies to it too. Paths
  stop being counted once a burst is pending, and the set is freed then. It counts every path the raw
  events name, before `Batch` turns a rescan-flagged event into `[Rescan]` (codex review F2,
  `a_rescan_events_path_counts_towards_a_burst`).
- **`watcher_recovery` queues every synthetic event before `ProjectWatcher::new`**, so the thread can't
  see a scheduling pause in the fixture as quiet and split the burst. `Burst` exists before any wait,
  and its `Drop` drops the sender before the watcher joins its thread, so a failed wait reports
  instead of hanging (codex review F3, F4).
- **At base, both `watcher_recovery` tests fail on an unexpected change** before their first deadline:
  nothing reads `SOURCE`, so the real stream's late report of the temp directory's creation reaches the
  sink. With the change, `SOURCE` replaces notify's receiver, so only synthetic events arrive.
