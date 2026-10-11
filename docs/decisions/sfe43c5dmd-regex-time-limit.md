# A wall-clock limit on `:s` and searches

Status: **decided**. Source card T43 (Mac T310), from T218's review, F1.

## Context

fe-vim matches a row through `RowMatches::next`, one `captures_read_at` call
for each match, each resuming where the last ended. On an adversarial
pattern (`a.*z\|a` on a row of `a`) every call scans to the row's end, so
listing a row's matches is O(n²). `:s///g`, backward searches, `/` up to
the cursor's column and counted searches all list a row's matches.

## Decision

Each operation makes a `Deadline` (`search::time_limit()`, 2 s by default,
nvim's `'redrawtime'`) and `RowMatches::next` checks it before each regex
call; the overrun is at most one row's scan. A search that passes it
reports `Interrupted` and the cursor stays. `:s` that passes it changes
nothing and reports `Interrupted`.

- **A limit, not ctrl-c.** Ctrl-c can't reach a command running on the UI
  thread; nvim interrupts because it polls for the key inside its matcher.
- **No partial `:s`.** nvim keeps the rows already done. Here an interrupted
  `:s` leaves no transaction, undo step, jump mark or caret move, so
  `Interrupted` is always "nothing happened".
- **Wall-clock from the operation's start.** One `Deadline` per command is
  checked before every regex call, with no exemptions, and the row loops
  stop as soon as it has run out. Reading rows and building edits count
  too. (A cheaper count, of regex time or of repeat calls only, was tried
  and dropped: it let a multi-row command run past the limit unreported.)
- The limit is process-wide (`set_time_limit`), for a later setting.

## Measured cost without a limit

`:s/a.*z\|a/X/g` on one row of `a`, no limit, release build on this box
(2026-10-11): 10,000 took 67 ms, 20,000 took 261 ms, 40,000 took 1.04 s.
That is quadratic, so the tests' 200,000-`a` row would take about 26 s in a
release build (more in a debug one). nvim 0.12.4 needed 194 s for 40,000
(card T43). The tests' 100 ms limit stands for "would not finish".
