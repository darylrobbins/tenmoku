---
task_uid: "01m4rfjwr2ex4458fh2xk7w9sg"
task: "T49"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "unfocused_while_the_window_is_inactive, and every other test in crates/fe-terminal-view/tests/view.rs that runs a real shell, calls cx.executor().allow_parking() before opening the view"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal-view's tests pass under nextest; no production code changes"
    required: true
    manual: false
---

# Allow parking in the fe-terminal-view focus test, whose pty reader thread wakes the test scheduler (flake)

Flake on argon main 142ba64 (run 01m4rbxw3cjx1zynwbnj2x7fff, fast group, 2026-10-12 15:41Z): `fe-terminal-view::view unfocused_while_the_window_is_inactive` panicked at gpui-pre-scheduler test_scheduler.rs:193 with "Detected activity on thread Some("fe-terminal pty reader") ... Your test is not deterministic." A rerun passed.

Cause: the test opens a TerminalView on a real shell, so the pty reader thread wakes the view's event task from a foreign thread. The other pty-backed tests in crates/fe-terminal-view/tests/view.rs (`sizes_the_pty`, and every test through `painting`) call `cx.executor().allow_parking()` first, with the comment "The pty's threads wake the view's event task." This one doesn't, so whether it fails depends on whether the shell writes before the test ends.

Fix: call `cx.executor().allow_parking();` at the top of `unfocused_while_the_window_is_inactive` (with the same comment). Check whether any other test in the file opens a view on a real shell without it, and fix those the same way. Test-only change; no production code.
