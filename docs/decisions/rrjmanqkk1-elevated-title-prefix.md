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
