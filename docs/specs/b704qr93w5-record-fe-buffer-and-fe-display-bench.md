---
task_uid: "01m4r8hyhc000bskb704qr93w5"
task: "T32"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "PERF.md has a 'Buffer (cargo bench -p fe-buffer)' section with every case of crates/fe-buffer/benches/buffer.rs: median and fastest sample, machine, commit, date and load average, and each case compared with the budget in buffer.rs's header"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md has a 'Display map (cargo bench -p fe-display)' section with every case of crates/fe-display/benches/display.rs in the same form, compared with display.rs's header budgets, and the Measuring block lists cargo bench -p fe-display"
    required: true
    manual: false
  - id: "C3"
    text: "Only PERF.md changes; any case over its budget is stated in its section with a named follow-up"
    required: true
    manual: false
---

# Record fe-buffer and fe-display bench results against their M1 budgets in PERF.md (Mac T404)

Filed on helium from Mac T404 (~/coordination/helium-cards/T404.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-o.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Helium notes (from the Mac lead, note o)

- Label every number as measured on helium (Linux x86_64): name the machine (CPU model and core count from
  lscpu, e.g. "helium (Linux x86_64, 24 cores)"), the commit, the date and the load average, as C1 asks. Follow
  the Typing latency section's form: a "helium" machine line, and table columns headed "helium median" and
  "helium fastest", so Mac columns can sit beside them later. Don't overwrite or remove any Mac number.
- `sysctl -n vm.loadavg` is macOS-only; on helium read the load average from `cat /proc/loadavg` (or
  `uptime`) at the start and end of each bench run, and record both.
- Other lanes may be building while you run. Note how many were active (load average shows it). The budgets in
  the bench headers are stated for Apple Silicon; compare helium's numbers with them as stated, and say so.
- Title the host commit "... (Mac T404)".

## Problem

M1's exit criterion is "load/edit benches meet budget". The benches exist, but nobody has recorded their
results against those budgets:

- `crates/fe-buffer/benches/buffer.rs:1-4` sets the budgets in its header. A 100 MB / 1M-line file must load
  well inside the 500 ms "interactive" budget. A keystroke edit in it must be far below a frame (8 ms p99 end
  to end). Snapshots must be O(1), and anchor resolution must scale with the edits since the anchor.
  PERF.md lists `cargo bench -p fe-buffer` in its Measuring block (line 13) but has no section for it.
- `crates/fe-display/benches/display.rs:1-5` says a keystroke re-wraps only its row, far inside 8 ms, and a
  new wrap width wraps one slice per frame, which must stay well inside a frame. Its numbers appear only in
  the sliced-soft-wrap decision (T10: "set width plus one slice under 2 ms"). PERF.md has no section for it,
  and the Measuring block doesn't list it.

## Change

Measurement only. No code changes.

- Run `cargo bench -p fe-buffer` and `cargo bench -p fe-display` on current main. Build into a private
  `CARGO_TARGET_DIR`, and delete it afterwards.
- PERF.md gains a `## Buffer (cargo bench -p fe-buffer)` section and a `## Display map (cargo bench -p
  fe-display)` section, placed before `## Syntax`. Each has a table of every case, with criterion's median
  and the fastest sample (min of times/iters from `target/criterion/**/new/sample.json`). Below each table,
  state the machine, the commit, the date and the load average (`sysctl -n vm.loadavg`) during the run. Then
  compare each case with its header budget: load against 500 ms; `edit/*`, `undo/*` and `sync/keystroke*`
  against 8 ms; `snapshot/clone` as O(1); slices against "well inside a frame" (8.3 ms at 120 Hz).
- Add `cargo bench -p fe-display` to the Measuring block, with its approximate run time.
- If a case misses its budget, say so in its section and name the follow-up to file. Don't fix it here.
