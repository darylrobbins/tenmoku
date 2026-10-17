# T180: an edit past the undo history limit trims in time proportional to what it drops

`Buffer::history` is a `VecDeque`; `trim_history` finds the origin's newest
droppable entry by a reverse scan, walks from the front dropping the oldest
until the origin is at its limit, and removes only the prefix up to the last
dropped entry (`pop_front`, then drain that prefix and push the kept entries
back). There is no vector that grows with the history.

## C3: bench evidence

Machine helium (Linux, x86_64), 2026-10-17, load average about 17-20 (other
lanes building), `cargo bench -p fe-buffer --bench buffer -- edit/keystroke`,
100MB buffer, criterion mean.

| case | base (f06fb34 `buffer.rs`, new bench) | candidate (33e81ba2) |
|---|---|---|
| `edit/keystroke in 100MB` | 290.8 us | 2.66 us |
| `edit/keystroke with a full undo history in 100MB` | 285.1 us | 2.43 us |

Candidate full-history case / its empty-history keystroke case: 0.91 (limit 3x).
At base both cases pay the trim (the keystroke case fills the history during
its own iterations); the candidate pays none of it.

The bench fills the history with at least one batch of 1000 edits before it
checks that `history_bytes` stopped growing, so it fills even when run alone
by a criterion filter. The numbers above were taken with an earlier form of
that loop in a full `edit/keystroke` run, where the preceding case had already
put edits in; the fill is the same.

Run alone with the corrected fill (`-- "full undo history"`), same machine and
day, load about 15: 3.51 us, 1.3x the empty-history keystroke case.
