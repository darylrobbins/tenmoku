---
task_uid: "01m4rjpjq1zsdjk7fjn6xsf6td"
task: "T88"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "unfocused_while_the_window_is_inactive passes 30/30 consecutive nextest runs under load (load average logged), and still asserts focus is lost while inactive and regained on reactivation"
    required: true
    manual: false
  - id: "C2"
    text: "No other fe-terminal-view test fails in those runs"
    required: true
    manual: false
---

# Make fe-terminal-view's unfocused_while_the_window_is_inactive test deterministic: the pty reader thread wakes GPUI's test scheduler under load

`fe-terminal-view`'s test `tests/view.rs::unfocused_while_the_window_is_inactive` is flaky under load. It failed 7 times on neon on 2026-10-12 at load averages of 13–21: main's fast check (twice), T80's, T82's, T85's and T86's gates, and T82's post-merge check, which reverted T82 and froze the repo. It passes alone at low load (3/3 at load 9–11).

The failure is gpui-pre-scheduler's non-determinism check (test_scheduler.rs:193, `assert_correct_thread`):

    Detected activity on thread Some("fe-terminal pty reader") ThreadId(3), but test scheduler is running on Some("unfocused_while_the_window_is_inactive") ThreadId(2). Your test is not deterministic.

The test opens a TerminalView with the default `TerminalOptions::new(dir)` (the user's login shell), so the pty reader thread delivers output whenever the shell prints. That wakes a local-executor task from the reader thread. Other tests in the file use a scripted shell (`options.shell = Some(...)`) and `cx.executor().allow_parking()`. Note that `allow_parking` alone doesn't bypass `assert_correct_thread`; only `parking_allowed_once` does.

Make the test deterministic without weakening what it checks: focus is lost while the window is inactive and comes back on reactivation. For example, give it a quiet scripted shell like its neighbours and make sure no pty output can arrive while the test scheduler runs. Or check whether the focus behaviour needs a live shell at all. If the right fix is in fe-terminal-view's reader-to-foreground handoff (not just the test), say so in the commit. Other pty tests in the same file may share the hazard; check them, and fix those too if they do.

Verify with repeated runs under load: run the test (and the file) at least 30 times with `cargo nextest run -p fe-terminal-view --profile ci` while the machine is busy (e.g. a parallel workspace build), log the load average, and record the counts in the commit.
