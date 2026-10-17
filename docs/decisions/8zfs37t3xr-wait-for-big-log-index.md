# T142: wait for big.log's index before the split

`viewer_and_editor_tabs_together` now calls `wait_for_index(&viewer, &big, cx)`
before `pane.split_right`. It loops (sleep 10 ms, `advance_clock(200 ms)`,
`draw`) up to 500 times until `viewer.is_polling()` is false, and panics
naming the path if it never is. The `shown(&split) == shown(&viewer)` assert is
unchanged. Only `crates/fe-workbench/src/large_file_tests.rs` changed in code.

## C2: runs under load

Command (the binary from `cargo test -p fe-workbench --lib`, at the commit
with the fix), repeated 24 times in a row:

    BIN=target/debug/deps/fe_workbench-e29c8646bf9f3f1f
    T=pane::large_file_tests::viewer_and_editor_tabs_together
    $BIN $T & $BIN $T & $BIN $T &     # three copies alongside
    $BIN $T                           # the counted run

The host is a 24-core machine shared with other lanes' gates and builds, so it
was loaded before the extra copies. Load average (1/5/15 min) at each run:

- runs 1-10: 48.00 / 45.14 / 40.89
- runs 11-18: 46.88 / 44.95 / 40.85
- runs 19-24: 46.09 / 44.82 / 40.83

Result: all 24 counted runs printed `test result: ok. 1 passed; 0 failed`
(0.23 to 0.57 s each). The three companion copies' results were not
inspected. A heavier generator (looping whole-suite copies and CPU burners)
was refused by the auto-mode classifier as interfering with other workloads,
so it was not run, and the base was not made to fail under this load.
