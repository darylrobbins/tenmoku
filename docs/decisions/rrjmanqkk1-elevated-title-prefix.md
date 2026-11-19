# Elevated title prefix kept when a title beats every default title (T1974)

## Cause (confirmed by reading the code path)

`program_title` strips conhost's `Administrator: ` prefix only after
`TITLE_PREFIX` (process-wide) has been learned from some session's default
title. `Listener::send_event` computed the label once, when the event arrived,
and never again. If session B's first title (`Administrator:  build`) arrives
before any session reported a default title, it goes out unstripped, and no
later event for B corrects it. The order of title events across sessions
decided the label, which is why `tab_strip` fails intermittently on the
elevated GH runner.

## Fix

Each Listener keeps its last raw console title and the label last sent, in a
slot registered (weakly) in a process-wide list. When `TITLE_PREFIX` is first
learned or changes, every live slot reruns `program_title` on its raw title and
sends a fresh `TerminalEvent::Title` if the label changed. Lock order is
slot, then prefix. Known limit kept: if no session ever reports a default title
nothing can be learned. Off Windows `is_console_default_title` is false, so the
prefix never changes and nothing is relabelled.

## Windows evidence (review F1)

Draft PR #5, sha 30dc31aae (the candidate's fix commit), windows Test. Full
log lines are in the lead's evidence files `t1974-windows-run1.txt` and
`t1974-windows-run2.txt`.

| Run | Job | `a_title_before_any_default_loses_the_prefix_once_learned` | other 2 `console_title_tests` | `terminal_tabs_tests::tab_strip` |
|---|---|---|---|---|
| 44469320022 attempt 1 | https://github.com/tenmoku-editor/tenmoku/actions/runs/44469320022/job/135374645219 | PASS | PASS | PASS (1.210s) |
| 44469320022 attempt 2 (rerun) | https://github.com/tenmoku-editor/tenmoku/actions/runs/44469320022/job/135408783279 | PASS | PASS | PASS |

`terminal_tabs_tests.rs` is unchanged. C2 and C3 are met by these two runs.
