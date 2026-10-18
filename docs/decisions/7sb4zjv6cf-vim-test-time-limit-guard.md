# Serialising the raised search time limit in fe-vim tests (T202)

- `tests::raised_time_limit()` (`crates/fe-vim/src/tests/mod.rs`) takes a static `Mutex` (poison ignored), sets the process-wide search limit to 600 s and returns a guard; dropping it restores `DEFAULT_TIME_LIMIT` and releases the lock.
- `counted_search_is_bounded` and both blocks of the substitute test use it, so no test sets or restores the limit outside the lock. Assertions are unchanged, including `ROWS_SEARCHED <= 6`. No production code changed.

## C2 stress evidence

20 of 20 runs of `cargo test -p fe-vim --lib -- counted_search_is_bounded substitute --test-threads 64` passed (3 tests each). Each ran while a background `cargo test -p fe-vim -p fe-buffer -p fe-editor -p fe-display`, then `cargo test -p fe-vim`, ran the heavy workspace tests in parallel on a shared host with other lanes active. One-minute load average at the start of each run:

```
run 1: pass, load1 at start 64.28, 3 passed
run 2: pass, load1 at start 71.54, 3 passed
run 3: pass, load1 at start 69.32, 3 passed
run 4: pass, load1 at start 66.91, 3 passed
run 5: pass, load1 at start 66.86, 3 passed
run 6: pass, load1 at start 64.95, 3 passed
run 7: pass, load1 at start 64.00, 3 passed
run 8: pass, load1 at start 67.51, 3 passed
run 9: pass, load1 at start 68.44, 3 passed
run 10: pass, load1 at start 63.97, 3 passed
run 11: pass, load1 at start 65.43, 3 passed
run 12: pass, load1 at start 76.94, 3 passed
run 13: pass, load1 at start 93.12, 3 passed
run 14: pass, load1 at start 96.32, 3 passed
run 15: pass, load1 at start 93.27, 3 passed
run 16: pass, load1 at start 83.93, 3 passed
run 17: pass, load1 at start 76.19, 3 passed
run 18: pass, load1 at start 70.19, 3 passed
run 19: pass, load1 at start 72.04, 3 passed
run 20: pass, load1 at start 68.18, 3 passed
```

`cargo test -p fe-vim` passes; `cargo clippy -p fe-vim --tests -- -D warnings` is clean.
