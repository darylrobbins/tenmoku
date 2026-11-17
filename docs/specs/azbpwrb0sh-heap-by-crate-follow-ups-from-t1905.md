---
task_uid: "01m7kmkb65vyc92zazbpwrb0sh"
task: "T1919"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "An xctrace Allocations recording of a launched bench build is attempted on the Mac; PERF.md says whether its allocations table exports with xctrace export and, if so, heap-by-crate.py reads it (fixture test updated)"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md's Heap by crate section adds a measurement after scrolling a 10k-line Rust file through the --perf harness's programmatic scroll in a MallocStackLogging=lite bench build (top crates by live bytes), with the method; it notes that a hand scroll was not measured"
    required: true
    manual: false
---

# Heap-by-crate follow-ups from T1905 review: verify an xctrace Allocations recording exports its table (or document that it can't) and add hand-scrolled measurements to PERF.md's Heap by crate section (T1905 F5 F6)
