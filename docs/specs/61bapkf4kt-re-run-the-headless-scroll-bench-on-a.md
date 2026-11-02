---
task_uid: "01m55w1f7fdke3a961bapkf4kt"
task: "T974"
rev: 1
type: "chore"
complexity: "xs"
criteria: []
---

# Re-run the headless scroll bench on a quiet Mac and confirm the jump cases against 120 fps

T818's PERF.md numbers (jump/1m-plain, jump/7mb-rust, jump/1m-plain-wrapped) were taken at load average ~50, so their p99 is scheduler noise. Re-run `cargo bench -p fe-gpui --bench scroll` on a quiet machine and replace the table in PERF.md "Scrolling, headless" with that run, saying per case whether frame p99 <= 8.33 ms holds.

## Implementation notes

Lead core decided on 2026-11-02, after a quiet Mac stopped being available (other lanes kept building), that the bench is rerun under load: five runs, each case's fastest by frame p99, load average recorded beside the numbers, and a miss reported as load or real. Load was 20.8-23.8 (1 min).
