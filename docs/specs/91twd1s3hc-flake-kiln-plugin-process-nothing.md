---
task_uid: "01m5ngmaczjwmb8j91twd1s3hc"
task: "T220"
rev: 1
type: "chore"
complexity: ""
criteria:
  - id: "C1"
    text: "The final printf spawn in nothing_outlives_a_call_that_ends runs under its own State with a generous (30 s) deadline, so a loaded machine cannot make it TimedOut; the holder and sleep calls keep the 300 ms deadline."
    required: true
    manual: false
  - id: "C2"
    text: "Every existing assertion is kept (holder and sleep TimedOut, elapsed \u003c 2 s, printf stdout == b\"ok\", threads() == before); no skip-under-load guard is added and no deadline of the timeout cases is loosened."
    required: true
    manual: false
  - id: "C3"
    text: "Proof: 20 consecutive runs of nothing_outlives_a_call_that_ends (cargo nextest, --retries 0) pass while 3x-ncpu 'yes' burners keep load above 30."
    required: true
    manual: false
---

# Flake: kiln-plugin process nothing_outlives_a_call_that_ends fails under load (the final printf spawn misses its 300 ms deadline, TimedOut) (Mac T1119)

Seen once in main's fast check under load 16-20 (run 01m5nexpbxsg2cqjxaqj01nha9, test.fastedit.log ~line 5120), 1.1 s run:
kiln/crates/kiln-plugin/tests/process.rs, nothing_outlives_a_call_that_ends (~line 416):
`state.spawn("printf", &args(&["ok"]), Some(&unread)).unwrap()` panicked on Err(TimedOut { program: "printf", deadline: 300ms }).
Spawning a copied printf missed the test's 300 ms wall-clock deadline on a loaded machine. Passed 3/3 alone (~0.75 s each).

Cause: the test builds one State with `.with_deadline(Duration::from_millis(300))`. That short deadline is needed for the two calls that must time out (the holder script that leaves `sleep 3 &` holding stdout/stderr, and `sleep 5` with unread stdin). The final printf call is the success path (a program that ends without reading its stdin ends the call), and it does not need a short deadline. Under load, process startup alone can exceed 300 ms.

Fix (test-only): leave the first State at 300 ms for the holder and sleep calls. For the final printf call build a second State from the same record (`State::new(record, &Limits::default()).with_deadline(Duration::from_secs(30))`; record is moved after the first State, so clone it earlier, as the test at line 351 already does). Keep every assertion: holder and sleep still TimedOut, the elapsed < 2 s bound, printf stdout == b"ok", and the `threads() == before` check that nothing outlives the call. Do not add a skip-under-load guard and do not loosen the 300 ms deadline of the timeout cases. Chore: test-only, no regression test.
