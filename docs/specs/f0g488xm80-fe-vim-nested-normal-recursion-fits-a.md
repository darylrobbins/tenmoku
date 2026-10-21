---
task_uid: "01m5g4cxr94phb7zf0g488xm80"
task: "T270"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The integration test nested_normal_fits_a_small_stack (top level, crates/fe-vim/tests/normal_stack.rs) passes: :normal @a inside register a on a 512 KiB-stack thread ends with the message 'E192: Recursive use of :normal too deep' and the text unchanged, and at the base it overflows the stack; normal_inside_itself_stops still passes unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim and cargo clippy -p fe-vim --all-targets -- -D warnings pass; the depth chosen and the measured stack per level are in the finish notes"
    required: true
    manual: false
---

# fe-vim: nested :normal recursion fits a small stack on every platform, so normal_inside_itself_stops no longer overflows the macOS workspace build (T255 follow-up)

## Problem (found by the macOS gate, Mac y37)

`fe-vim::ex_global normal_inside_itself_stops` (`crates/fe-vim/tests/ex_global.rs`) aborts with a **stack overflow** in the macOS workspace build, from T255 (Mac T908, vim `!` filter, `77ac0156`) on: it passes after T254, with only fe-vim built, and on the Linux gate (frame sizes differ by platform). `:normal @a` inside register `a` recurses until `NORMAL_DEPTH` (100, `crates/fe-vim/src/ex/global.rs:813`) and each level's keys run on the thread's stack; T255 added frames to that path, so 100 levels no longer fit in a 2 MiB test thread's stack on macOS. The limit must not depend on the platform's frame size.

## Change

Make the nested-`:normal` recursion safe on a small stack, in `crates/fe-vim/src/ex/global.rs` (and the dispatch path only if it is the cheaper fix):

- Lower `NORMAL_DEPTH` far enough to leave a wide margin (the doc comment already says "fewer here than nvim's 1000, as each level's keys run on this thread's stack"; pick the depth by measuring the stack one level uses in a debug build, and give at least 4x headroom against a 512 KiB thread), or make the nested `:normal` iterative or heap-allocated, or shrink the frames T255 added. Keep E192 ("Recursive use of :normal too deep") at the limit, and keep `normal_inside_itself_stops`'s assertions (text unchanged, the E192 message) unchanged.
- New test `crates/fe-vim/tests/normal_stack.rs` (top level, `nested_normal_fits_a_small_stack`): the same `:normal @a` self-recursion run on a `std::thread::Builder` thread with `stack_size(512 * 1024)` (precedent: `crates/fe-vim/src/tests/search_pattern.rs:232`), asserting the E192 message and the unchanged text. It must overflow (abort) at the base on Linux with the 512 KiB stack, and pass after the fix; a platform-independent check, with no wall-clock or frame-size constants in the assertions.

Depends on nothing; T255 (and T265) are already on helium `main`. Re-batch with T255 and T265 after it lands (lead does that).

## Implementation notes

Measured in a debug build on Linux: 25 nested levels fit a 512 KiB thread and 30 overflow, so one level uses about 17-20 KiB (macOS more). NORMAL_DEPTH is 5, leaving over 4x headroom.
