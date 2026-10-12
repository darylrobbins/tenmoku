---
task_uid: "01m4rff19pchvvy2peqp9tw3aw"
task: "T494"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The test still catches quadratic recording work but no longer fails from machine load: at load ~90 (T416's post-merge check, run 01m4rdtq9ecqd2tsfyv8g56n62) it saw 304.6ms for 10000 batches vs 5.49ms fastest for 1000 (55x), with 1000-batch samples swinging 5-109ms. Prefer a load-independent measure (e.g. counting work, as fe-vim's ROWS_SEARCHED does) over a looser timing ratio."
    required: true
    manual: false
  - id: "C2"
    text: "Proven by 20 repeated runs of the test passing while the machine is under load (e.g. alongside a full workspace nextest run); record the command and result in the evidence."
    required: true
    manual: false
  - id: "C3"
    text: "Nothing outside crates/fe-editor/src/tests/batch_structure.rs changes, except this lane's own docs/specs file."
    required: true
    manual: false
---

# Flake: fe-editor many_batches_recorded_together_take_linear_work fails under heavy load (ratio of fastest timings)
