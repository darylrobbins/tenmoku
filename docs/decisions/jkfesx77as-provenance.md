# Keeping authors cheap enough for typing (T397)

Status: **built; within the budget on helium.** The card allows the typing case of
`cargo bench -p fe-buffer --bench buffer` to move by 5%. The spans live in a `sum-tree`, as the card asks, but a
batch isn't spliced into the tree as it applies. It waits in a list until something needs the spans. Measured
2026-11-01 at `ef0154bd` against `663e5721`, the lane's base.

## The numbers

Each case alternated the base binary and this one for 8 rounds of 30 samples (3 s of measuring, 1 s of warm-up),
taking the fastest sample of all the rounds, at load 12–16:

| Case | Base | This card | Ratio |
|---|---|---|---|
| `edit/keystroke in 100MB` | 1,530.6 ns | 1,531.9 ns | 1.001 |
| `edit/keystroke with a full undo history in 100MB` | 1,355.5 ns | 1,414.4 ns | 1.043 |
| `edit/100 cursors in 100MB` | 97.27 µs | 98.31 µs | 1.011 |
| `undo/keystroke after 100 later edits` | 12.09 µs | 26.37 µs | 2.18 |

An earlier 10-round run of the first case, on the first commit, gave 1,451.1 ns and 1,487.2 ns, a ratio of 1.025.
Rounds vary by about 10% at this load, so the two typing cases are within the budget but not shown to be free.

The undo case is the one that pays: its first undo settles the 101 batches its setup applied. It is not a typing
case and has no budget, and 14 µs is well under a frame.

## What deferring moves, and to where

The work is moved to the first read, not removed. These are this binary's own cases, added to the bench, as
10 samples at the same load, median of criterion's estimate:

| Case | Time |
|---|---|
| `provenance/typing in one place, read after each keystroke, in 100MB` (edit and read) | 1.43 µs |
| `provenance/keystroke anywhere, read after each, in 100MB` (edit and read) | 8.93 µs |
| `provenance/first read after 1000 keystrokes anywhere, in 100MB` | 2.52 ms |
| `provenance/first read after 100000 keystrokes anywhere, in 100MB` | 650 ms |

So a gutter that reads every frame pays a few microseconds a frame while someone types. The cost to watch is the
last row: something that first reads after 100,000 unread batches stalls for about 650 ms. A person typing takes
hours to get there, and any undo or redo settles the list on the way, as does `compact_log`, but a host that
applies very many small batches and only then reads will feel it. The next card (the gutter) should read from the
start rather than only once it is shown, if that case turns out to matter.

## Why not splice as each batch applies

The base keystroke takes about 1.5 µs, so 5% is about 75 ns, roughly one cache miss. The bench's keystrokes land at
random offsets in text from disk, so each one adds spans, and the tree reaches hundreds of thousands of runs.

The nearest thing measured to splicing every batch is a first version that deferred only pure insertions. It took the
case to 5.2x base: 7,392 ns against 1,414 ns, fastest of 5 rounds. A fully eager version wasn't built separately. That
one still did nearly all the splicing during the bench, because the corpus has `é` and `😀`: about 0.9% of its random
offsets clip to a replacement of the whole character, which forced a settle of everything pending. Timing the sections
of `edit` in that build put 5–10 µs per keystroke in the provenance step. So every kind of batch has to wait, not just
insertions.

A batch that replaces text can't know the spans it replaced until it settles, so it holds an empty slot shared with
the waiting entry (`ReplacedSpans`). `Buffer::undo` and `redo` settle first, so they never read an empty one.
`Buffer::reload` also settles before it clears, so older transactions still give back what they replaced.

## Decisions for the reviewer

- **Replaced spans aren't counted in `history_bytes`.** They are dropped with their history entry, which is how the
  card says the cap bounds them. Counting them isn't possible for a fresh batch, whose spans aren't known when it is
  charged, and `history_cap`'s existing tests require an undo to be charged what its forward batch was. A span
  (40 bytes) takes at least one byte of old text, which is charged, so text that alternates authors every byte could
  hold about 40x its charge. Ordinary text holds far fewer spans than bytes.
- **The waiting list isn't capped.** A cap would bring the splice cost back into the bench. An entry is 48 bytes and
  shares its edits with the edit log, which also grows with each batch until `compact_log`, so the two grow together.
  No production code calls `compact_log` yet.
- **A row's bytes exclude its line break**, except on an empty row, where the line break is the only byte. This reads
  the card's "any AI-authored byte" so that an AI's `"\nfoo"` at the end of the user's line doesn't claim that line,
  and the user's enter at the end of a disk line doesn't either.
- **A restored backup has no author**, though `from_backup` applies it under `Origin::User`: a backup doesn't record
  who wrote its text, so crediting the user could misattribute an agent's. Persisting spans stays out of this card.
- **Undoing a reload brings the old text back with no author.** The reload clears everything, so its own batch has
  nothing to give back. Undoing a transaction older than the reload still restores the spans that one replaced.

## To reproduce

`cargo bench -p fe-buffer --bench buffer --no-run` at each commit, copy each binary aside, then alternate:

```
./base  --bench '^edit/keystroke in 100MB$' --noplot --sample-size 30 --measurement-time 3 --warm-up-time 1
./after --bench '^edit/keystroke in 100MB$' --noplot --sample-size 30 --measurement-time 3 --warm-up-time 1
```

The fastest sample is the minimum of `times[i] / iters[i]` in criterion's `new/sample.json`.
