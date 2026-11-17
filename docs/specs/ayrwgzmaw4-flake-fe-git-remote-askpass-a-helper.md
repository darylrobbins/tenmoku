---
task_uid: "01m7kw4yhpd22fatayrwgzmaw4"
task: "T671"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "remote_askpass a_helper_gone_withdraws_its_request_and_a_notice_asks_nothing passes 20 consecutive runs (cargo nextest run -p fe-git --test remote_askpass) with CPU burners (2x ncpu) running; run 01m7ksytwc7k185cx0et21n74p failed once at remote_askpass.rs:621 under load ~90-100"
    required: true
    manual: false
  - id: "C2"
    text: "only crates/fe-git/tests/remote_askpass.rs changes; crates/fe-git/src/** untouched; no regression test (test-only chore)"
    required: true
    manual: false
---

# Flake: fe-git remote_askpass a_helper_gone_withdraws_its_request_and_a_notice_asks_nothing fails under heavy load (fake ssh kills the askpass helper after a fixed 1 s, before the slow-starting helper connects) (Mac T1925)

Cause (lead triage): the real panic is the child's assert at crates/fe-git/tests/remote_askpass.rs:621 (asked is empty); :90 only relays it from isolated(). fake_ssh's vanish host (:164-165) starts the helper (this test binary re-exec'd via helper(), :123) and kills it after a fixed 'sleep 1'. Under load the helper takes longer than 1 s to reach the socket, so the request never reaches the callback and the 500x10 ms wait (:616-620) times out. askpass.rs withdraw logic is not implicated. Fix: in fake_ssh's vanish branch, kill the helper only once it has connected (wait for a marker file the helper or the Askpass callback writes, with a generous bound), and widen the :616 wait to a deadline of tens of seconds. Keep the test's meaning: an unanswered helper is killed, its request withdrawn, and the notice asks nothing.
