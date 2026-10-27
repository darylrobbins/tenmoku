---
task_uid: "01m5wzvvnaezkfyrse6tkx1qa8"
task: "T1196"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "exhausted_retries_clear_the_swatches advances the gpui executor clock on every poll while waiting for each ask, so a REST timer created after the advance still fires; only crates/fe-workbench/src/lsp_colors_tests.rs changes"
    required: true
    manual: false
  - id: "C2"
    text: "The test keeps its assertions (swatches kept while asks 2..9 retry, cleared after the 10th ask, exactly 10 COLORS requests after a further advance) and is not weakened"
    required: true
    manual: false
  - id: "C3"
    text: "Evidence in the notes: with 4 CPU-burning background loads (e.g. 'yes \u003e/dev/null \u0026' x4), 'cargo nextest run -p fe-workbench -E \"test(exhausted_retries_clear_the_swatches)\"' run 20 times, all 20 pass, each reporting 1 passed, with the command and per-run results (pass/fail, seconds); clippy -p fe-workbench passes"
    required: true
    manual: false
---

# Flake: fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches fails under load (60 s timeout waiting for the next ask)

# Flake: fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches

## Evidence
Failed after about 60 s ("timed out waiting for the next ask", panic at crates/fe-workbench/src/tests/lsp.rs:522, the 60 s `Lsp::until` bound) in Mac gate runs 01m5wmmfk6x1sqp7hx9bhwt9sq, 01m5wqtd0yakfhksctv6ypsjjm, 01m5wsx784e7hnpy43f4kkc96e and 01m5wx549gp22f5wv8khvjamxw (logs: ~/.local/state/regatta/runs/<run>/test.fastedit.log).

## Cause (test-only; the production timer is correct)
The test at crates/fe-workbench/src/lsp_colors_tests.rs:236 expects the server's retry-error to be asked again up to 8 times, each after `REST` (500 ms, lsp_colors.rs `ask_colors_in`, a `background_executor().timer(delay)`). Per round it calls `cx.executor().advance_clock(510ms)` ONCE, then `until_ticking`, which only advances the LSP manager's `ManualClock` (`s.clock`, 20 ms per poll), never the gpui executor clock the REST timer runs on. The next timer is created only when the previous reply lands (`colors_landed` -> `ask_colors_in`), and the reply comes from the fake server's thread. On a loaded host the reply lands after the one `advance_clock`, so the new 500 ms timer is created after the advance, nothing ever advances the executor clock again, and `until_ticking` spins until the 60 s bound. Idle hosts pass because the reply lands first. It is a lost wakeup in the test's clock handling, not a wall-clock bound and not a production race.

## Fix
Advance the executor clock inside the `until_ticking` predicate for each ask (e.g. a local `until` that does `cx.executor().advance_clock(510ms)` plus `draw()` on every poll until `requests(s, COLORS).len() == asked`), so a timer created late is still fired. Keep the assertions: swatches kept while retried (asked < 10), cleared after the 10th ask, and no 11th request.

## Do not touch
lsp_colors.rs (REST, RETRIES, timer logic), tests/lsp.rs (`until`'s 60 s bound, shared by every LSP test), and the other `until_ticking` copies. Do not raise a timeout or add sleeps.
Overlap note: T1113 (blocked, colour swatches) may touch lsp_colors_tests.rs; rebase over it if it lands first.

## Implementation notes

- The spec's fix (advance the executor clock on every poll of each ask wait) was not enough: under load the test then stalled on the final "no swatches" wait, where the 10th ask's reply never landed because nothing advanced the executor clock. That wait now advances it on every poll too. Each ask wait advances only while fewer than `asked` requests are seen, so a poll cannot fire a timer past the ask being waited for.
- Not stack 60: 3aebbbbe touches only shortcuts_editor_tests.rs. The earlier fix alone failed about 40% of runs at 4 loads here, the same rate the full-run failures suggest.
- Evidence: with 4 `yes >/dev/null &` loads, `cargo nextest run -p fe-workbench -E 'test(exhausted_retries_clear_the_swatches)'` x20: all 20 PASS, each "1 passed" (0.15-0.40 s). A further 60 runs at 8 loads: all pass. `cargo clippy -p fe-workbench --tests`: no warnings or errors.

### C3 per-run results

Command (4 background `yes >/dev/null &` loads, then):

```
for i in $(seq 1 20); do echo -n "run $i: "; cargo nextest run -p fe-workbench -E 'test(exhausted_retries_clear_the_swatches)' 2>&1 | rg 'PASS|FAIL|Summary' | tr '
' ' '; echo; done
```

```
run 1:  PASS [ 0.532s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.537s] 1 test run: 1 passed, 1616 skipped 
run 2:  PASS [ 0.227s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.231s] 1 test run: 1 passed, 1616 skipped 
run 3:  PASS [ 0.273s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.277s] 1 test run: 1 passed, 1616 skipped 
run 4:  PASS [ 0.225s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.229s] 1 test run: 1 passed, 1616 skipped 
run 5:  PASS [ 0.269s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.273s] 1 test run: 1 passed, 1616 skipped 
run 6:  PASS [ 0.235s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.239s] 1 test run: 1 passed, 1616 skipped 
run 7:  PASS [ 0.220s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.224s] 1 test run: 1 passed, 1616 skipped 
run 8:  PASS [ 0.232s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.235s] 1 test run: 1 passed, 1616 skipped 
run 9:  PASS [ 0.220s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.224s] 1 test run: 1 passed, 1616 skipped 
run 10:  PASS [ 0.226s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.230s] 1 test run: 1 passed, 1616 skipped 
run 11:  PASS [ 0.230s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.235s] 1 test run: 1 passed, 1616 skipped 
run 12:  PASS [ 0.241s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.245s] 1 test run: 1 passed, 1616 skipped 
run 13:  PASS [ 0.214s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.217s] 1 test run: 1 passed, 1616 skipped 
run 14:  PASS [ 0.241s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.245s] 1 test run: 1 passed, 1616 skipped 
run 15:  PASS [ 0.221s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.225s] 1 test run: 1 passed, 1616 skipped 
run 16:  PASS [ 0.217s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.220s] 1 test run: 1 passed, 1616 skipped 
run 17:  PASS [ 0.229s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.232s] 1 test run: 1 passed, 1616 skipped 
run 18:  PASS [ 0.353s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.357s] 1 test run: 1 passed, 1616 skipped 
run 19:  PASS [ 0.229s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.232s] 1 test run: 1 passed, 1616 skipped 
run 20:  PASS [ 0.217s] (1/1) fe-workbench lsp::colors::tests::exhausted_retries_clear_the_swatches Summary [ 0.221s] 1 test run: 1 passed, 1616 skipped 
```
