---
task_uid: "01m54jhg3dznxmmk8zfs37t3xr"
task: "T142"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "viewer_and_editor_tabs_together advances the test clock until the original viewer's is_polling() is false (its index done) before pane.split_right, through a bounded helper that panics with a message rather than hanging, and still asserts shown(\u0026split) == shown(\u0026viewer)"
    required: true
    manual: false
  - id: "C2"
    text: "The hand-in records at least 20 runs of fe-workbench pane::large_file_tests::viewer_and_editor_tabs_together under load (load average stated per run, other copies of it or the fe-workbench suite running alongside), all passing, with commands and results in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "Of the worker's own changes, only crates/fe-workbench/src/large_file_tests.rs changes; no production code changes"
    required: true
    manual: false
---

# Wait for big.log's index to finish before the split in fe-workbench pane::large_file_tests::viewer_and_editor_tabs_together, so both viewers show the same rows

## Problem

`fe-workbench pane::large_file_tests::viewer_and_editor_tabs_together` fails under gate load:

```
panicked at crates/fe-workbench/src/large_file_tests.rs:311:5: assertion `left == right` failed
  left: (43791, "line 000000")
 right: (21846, "line 000000")
```

That is `assert_eq!(shown(&split, cx), shown(&viewer, cx))` right after `run("pane.split_right", cx)`.
Seen 2026-10-17 in a full Linux gate at load about 35, at d3fa52a, a change touching only fe-terminal (run
01m54hg5w1p9pns9b7a23m3155, `~/.local/state/regatta/runs/01m54hg5w1p9pns9b7a23m3155/test.fastedit.log`,
line 2511). It failed the same way, with the same numbers, in T120's finish gate on 2026-10-16. It passes alone.

Why: `shown` returns the viewer's `FileText` snapshot's `row_count()` and its first row. The test builds
`big.log` with `project(.., free = true)`, so its `LineIndex` scans on a real thread
(`loader::test_support::build_index`). `large_text()` is 2 × `fe_fileview::CHUNK_BYTES` (256 KiB) of
12-byte rows plus 100: 43,791 rows in all, and 21,846 after the first chunk. So the original viewer is
showing the index after one chunk, and the split the whole file.

- `FileViewer::split` (crates/fe-gpui/src/file_viewer/pane_item.rs:64) shares the same `Arc<LineIndex>`;
  the split builds no index of its own. `FileViewer::new` snapshots the index as it is at that moment.
- The original viewer only takes new rows in `FileViewer::refresh`, run from a poll loop that waits on
  `cx.background_executor().timer(POLL_INTERVAL)` (100 ms, crates/fe-gpui/src/file_viewer.rs:38; `FileViewer::start_polling`).
  In a gpui test that timer fires only when the test advances the clock.
- `wait_for_rows` (large_file_tests.rs:430) advances the clock only until the viewer shows more than one
  row. Unloaded, the index thread finishes both chunks inside that first 10 ms sleep, so the first refresh
  sees it done. Under load, the refresh lands after one chunk, the viewer keeps polling, and nothing in the
  test advances the clock again (`run` and `draw` only `run_until_parked`). The thread finishes later, and
  the split, made afterwards, snapshots all 43,791 rows while the original is stuck at 21,846.

In production the timer is real, so the original viewer catches up within 100 ms: two viewers of one
index polled at different moments is expected, not a bug. The fix belongs in the test.

## Change

In crates/fe-workbench/src/large_file_tests.rs add a helper beside `wait_for_rows`, `wait_for_index`,
that loops like it (sleep 10 ms, `advance_clock(200 ms)`, `draw`) until `viewer.is_polling()` is false,
which `FileViewer::refresh` sets only after a refresh that read the index as done before its snapshot.
Bound it (500 tries) and panic with a message naming the viewer's path if it never finishes. In
`viewer_and_editor_tabs_together`, call it on `viewer` before `run("pane.split_right", cx)` (call it in
place of `wait_for_rows` at the top if that reads better), and keep the
`assert_eq!(shown(&split, cx), shown(&viewer, cx))` as it is. Leave `wait_for_rows` and
`launch_file_and_restore` alone: they only check the first row. No production code changes.

Prove it with repeated runs under load, not a regression test: at least 20 runs of the test with the load
average stated, run while other copies of it or the fe-workbench suite run alongside, all passing. Record
the commands, the loads and the results in the decision file. If the base can be made to fail under the
same load first, record that too.

## Implementation notes

Added `wait_for_index(viewer, path, cx)` beside `wait_for_rows` (500 tries, panics naming the path) and called it on `viewer` before `pane.split_right`; the `shown(&split) == shown(&viewer)` assert is unchanged.

Load evidence: 24 runs of `target/debug/deps/fe_workbench-e29c8646bf9f3f1f pane::large_file_tests::viewer_and_editor_tabs_together`
(the built `cargo test -p fe-workbench --lib` binary), each with 3 more copies of the same test running at once, on a shared 24-core host already busy with other lanes' gates and builds.
Load average (1/5/15 min) at the runs: runs 1-10 48.00/45.14/40.89; runs 11-18 46.88/44.95/40.85; runs 19-24 46.09/44.82/40.83.
All 24 passed (`test result: ok. 1 passed`, 0.23-0.57 s each). A heavier generator (looping whole suite plus CPU burners) was refused by the auto-mode classifier as interfering with other workloads, so it was not run. The base was not reproduced failing under this load.
