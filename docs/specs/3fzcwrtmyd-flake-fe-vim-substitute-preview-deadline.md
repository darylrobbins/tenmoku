---
task_uid: "01m7pc6wm2cmtzvq3fzcwrtmyd"
task: "T421"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In crates/fe-vim/tests/substitute_preview_deadline.rs no test that bounds the counted units can be stopped by the real clock: the real-time limit is lifted (or a per-thread test seam used) around each counted preview, and the shipped limits are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim --test substitute_preview_deadline passes 30 times in a row while the machine is under load (e.g. a parallel cargo build or stress run), the run count and load noted in the decision file"
    required: true
    manual: false
  - id: "C3"
    text: "cargo clippy -p fe-vim --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: fe-vim substitute_preview_deadline::template_work_stops_at_each_stage fails under load (the preview's real-clock limit fires before the counted work limit) (Mac T1948)

`fe-vim::substitute_preview_deadline::template_work_stops_at_each_stage` failed in T1682's finish gate (run 01m7p71qvwcmgn3txwnfq93be3, load 31-60) with "stage 4: 3911 units counted past 4500": the preview stopped *before* its work-unit limit. The test interrupts the `:s` preview at a counted point (`set_work_limit`), but the preview's deadline also checks the real clock (T256), so under load the wall-clock limit fires first and fewer units are counted.

Fix the test so only the work-unit limit can stop the preview: lift the real-time limit for these counted tests (for example `set_time_limit` to a long duration, restored after, as `find_replace.rs` does) if that is the limit the preview reads; if the preview's deadline uses its own constant (`cmdline.rs` `SEARCH_PREVIEW_LIMIT`), add a `#[doc(hidden)]` per-thread test seam beside `set_work_limit` rather than changing the shipped limit. Check the file's other tests (`tilde_expansion_counts_what_it_writes` and any that bound `counted`) for the same exposure and fix them alike. No production behaviour change.

## Implementation notes

The preview's deadline is `search_preview_limit().min(search::time_limit())`, so the test now lifts both (per-thread `set_search_preview_limit`, process-wide `set_time_limit`, to 600 s) around each counted preview, serialized by a mutex and restored on drop. No production change.

C2 proof: `cargo nextest run -p fe-vim --test substitute_preview_deadline` run 30 times in a row, 30/30 passed (5 tests each), plus 10 threaded `cargo test` runs (all passed), with 6 busy-loop processes and a parallel `cargo build --all-targets` on top of the host's existing load; load average 4 at the start, 13.7 at the end (an earlier 30/30 run at load 10-20 ran before the lock was added). The host's gate was not run by this lane during the proof.
