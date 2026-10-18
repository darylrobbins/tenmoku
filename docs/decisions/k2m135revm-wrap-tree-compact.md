# The wrap tree kept compact under edits (T192, follow-up to T181 / T159 / Mac T813)

Status: **built; meets the budgets on helium.** On a wrap tree that has taken 100,000 typed
keystrokes, `fold` among 200k folds takes as long as on a fresh tree: in
each candidate round the after-100k case is 0.80–1.01x the fresh-tree case
(budget: at most 1.5x), where base's is 5.0–5.7x. It is 5.35x faster than
base's (3.92 against 20.98 ms). No `fold/` or `sync/` case is slower than
base by more than 1%; `sync/keystroke, 1M lines wrapped` is 2.4x faster.
The slowest single `map.sync` of the 100k typed keystrokes is 1.06 ms at
most in the candidate (budget 4 ms), 4.05 ms in base. Measured 2026-10-18.

## What the tree loses to edits

The measurement comes first, as the card asks. It is the ignored test
`wrap_map::compact_tests::measure`
(`crates/fe-display/src/wrap_map/compact_tests.rs`), run in release:

```
cargo test --release -p fe-display --lib wrap_map::compact_tests::measure -- --ignored --nocapture
```

It wraps the display bench's 1M-line corpus at 80 columns. It then splices
in keystrokes the way `DisplayMap::sync` does, at random offsets with a
fixed seed. It counts leaves and internal nodes from the tree's `Debug`
output, which names each node, and leaf "jumps" from the items' addresses:
a jump is a leaf that doesn't sit just after the one before it in memory
(backward, or more than 1 KiB forward). It also times the fold bench's
rebuild walk, `first_rows` over 200k runs one every 5 rows, as the fastest
of 30. Below, typing `x` with base's splice. The figures come from this
lane's first run of the test, before the change, at load 18–28:

| Wrap tree | Leaves | Items per leaf | Internal nodes | Height | Jumps | Walk |
|---|---|---|---|---|---|---|
| fresh | 83,334 | 12.00 | 7,579 | 5 | 19 | 2.8–3.0 ms |
| after 5,000 keystrokes | 87,634 | 11.41 | 11,642 | 6 | 19,693 | 6.4–7.6 ms |
| after 100,000 keystrokes | 117,014 | 8.55 | 16,089 | 6 | 115,681 | 16–22 ms |
| after 100,000, full repack (`SumTree::from_iter`) | 83,334 | 12.00 | 7,579 | 5 | 25–40 | 3.9–4.4 ms |

The full repack itself took 250–320 ms.

**Where the memory sits costs far more than the leaf count.** After 5,000
keystrokes the tree has only 5% more leaves, but the walk takes twice as
long. By then 19.7k of its 87.6k leaves aren't next to the one before them.
After 100,000 keystrokes nearly every leaf is somewhere else (115,681 jumps
in 117,014 leaves). The walk is 5–6x slower, against 40% more leaves.

Each splice slices the tree at the edit and appends the rest. That makes a
new part-full leaf on each side of the seam, plus new internal nodes up
the path. The allocator puts each new node in whatever hole a recent splice
freed, which is anywhere in the tree. A fresh tree's leaves sit in order,
so a walk streams through memory. A spliced tree's walk misses the cache at
almost every leaf (about 140 ns each, against 30 ns fresh).

The repack wins back nearly all of the slowdown: 3.9–4.4 ms against
16–22 ms. That is also because its leaves come out in order (25–40 jumps).

Re-packing in windows can't do the same, because the allocator decides
where the new leaves go. In this lane, sweeping the whole 100k-keystroke
tree in windows of 48 to 12,288 items, with no splices in between, gave
full leaves (83,334). But every leaf was still a jump (83,278–83,330), and
the walk went only from 15–16 ms to 10–18 ms. Each window allocates its
leaves into the holes that the windows before it freed, scattered as they
were.

Re-packing the seams wouldn't help either. `sum_tree`'s `append` already
merges a leaf under half full into its neighbour, which is why leaves keep
at least 6 items (8.55 on average after 100k). And the public API can't
find leaf boundaries, so a re-pack around a seam would make new seams of
its own.

## What changed

In `crates/fe-display/src/wrap_map.rs`, with no change to `sum_tree`:

1. **An edit that keeps a row's number of wrap rows rewrites the row's
   item where it is.** Typing within a row almost always does that.
   `WrapMap::splice` wraps the new rows first. When the edit replaces as
   many rows as it adds, and each new row has as many wrap points as the
   item it replaces, `rewrite_rows` stores the new wrap points into the
   items in place. A run of unwrapped rows stays as it is for a row that
   still fits. The tree's shape, summaries and nodes don't change, so
   nothing is allocated and nothing moves. Otherwise the splice goes
   through `replace_rows` as before. A failed rewrite may have rewritten
   some of the rows, which is harmless: they hold what `replace_rows` then
   puts there anyway.

   A wrapped row's wrap points are `Breaks(Box<[AtomicU32]>)`. They have to
   be atomic because `sum_tree` hands out only shared references to items,
   and `Relaxed` loads and stores cost the same as plain ones.
   `Breaks::clone` copies the values, so a cloned item is independent of
   the original. Only `WrapMap`, through `&mut self`, writes them. No other
   tree shares `lines`' nodes by then: `WrapMap` isn't `Clone`, and every
   rebuild of `lines` drops the old tree at once. `DisplayMap` stays `Send`
   and `Sync`. Unlike a `Cell`, the atomics don't change its auto traits.

2. **Splices that do move items are followed by a rotating window
   re-pack** (`Compactor`). Every `every` such splices (8), the next
   `window` items (384) are collected and pushed back as full leaves. Each
   window starts where the last one ended. One that reaches the end starts
   the next sweep, as long as edits keep adding rows. No keystroke pays for
   more than one window, whatever the tree's size. There is no whole-tree
   repack anywhere. Both thresholds are fields of `WrapMap`'s compactor,
   which the tests set lower. As measured above, the windows keep the leaf
   count down but can't put leaves back in order. Typing Enter 100,000
   times (each splice adds a row):

   | Enter ×100k | Leaves | Items per leaf | Internal | Jumps | Walk | Mean splice |
   |---|---|---|---|---|---|---|
   | no windows | 128,597 | 8.55 | 17,742 | 127,265 | 13.2–20.3 ms | 9.1–17.3 µs |
   | windows (8, 384) | 101,452 | 10.84 | 14,049 | 101,449 | 13.0–16.8 ms | 11.9–28.6 µs |
   | full repack after | 91,667 | 12.00 | 8,336 | 45–91 | 3.3–5.4 ms | — |

   Ranges are over this lane's runs at load 10–30. The windows cost a few
   µs on such a splice and save a few ms on the next fold. They are the
   backstop the card asks for. The fix for typing is (1).

Typing `x` with the change, from the same test at load 15–18:

| Wrap tree | Leaves | Items per leaf | Internal nodes | Height | Jumps | Walk |
|---|---|---|---|---|---|---|
| fresh | 83,334 | 12.00 | 7,579 | 5 | 19 | 2.60 ms |
| after 5,000 keystrokes | 83,334 | 12.00 | 7,579 | 5 | 19 | 2.62 ms |
| after 100,000 keystrokes | 83,334 | 12.00 | 7,579 | 5 | 19 | 2.63 ms |

None of the 100,000 keystrokes moved an item. The corpus's rows are 90–95
columns wide, or 1 for `}`. A few added chars don't change a row's wrap
count at 80 columns: a row 1 or 2 chars wide still fits, and a 90–95 column
row would need ~70 more to wrap a second time. A splice went from 12 µs on average to 3–5 µs.

Correctness, in `crates/fe-display/src/wrap_map/compact_tests.rs`:

- `wrap_tree_matches_a_fresh_map_after_random_splices` runs 4,000 random
  splices on 3,000 chars wrapped at 8 columns. Most are one char, some up to
  12 with deletes of up to 5, and they include newlines and `é`. It runs a
  5-item window after every splice that moves items. Every 100 splices it
  compares `row_count` and every `segment` with a freshly wrapped map. It
  asserts both paths ran often (over 500 rewrites in place and over 500
  windows) and that the windows went round the tree more than 5 times.
  Making the in-place store a no-op fails it.
- `windows_sweep_a_growing_tree` types 1,000 newlines with a window after
  each. A first version moved a window that reached the end to the end
  row, and edits that added rows then kept it there. This test catches
  that (0 sweeps). Typing Enter 100k times on that version left 127,587
  leaves, against 101,452 now.
- `an_edit_keeping_wrap_rows_leaves_the_tree_in_place` checks that typing a
  char keeps every item at its address and moves the row's wrap points,
  and that a newline moves items.

The existing wrap, fold and round-trip tests are unchanged and pass,
`fold_rows_across_edits` included.

## How it was measured

Machine: helium (Linux x86_64, AMD Ryzen 9 5900X, 12 cores / 24 threads).
Base `25c7711` (the card's base) with this card's bench file over its
sources; candidate ``0d3845f``. Each was built with `cargo bench -p
fe-display --no-run` into its own `CARGO_TARGET_DIR` under `~/bench-T192`.
They ran alternately for 3 rounds, base first, with `--bench --warm-up-time
1 --measurement-time 3` and `'fold/'`, then the same with `'sync/'`.
"Fastest" is the smallest times/iters over every sample of the case in
`new/sample.json`. "Median" is the lowest of the 3 criterion medians.

Both protocols ran 2026-10-18, `'fold/'` 00:39:49–00:42:13 UTC and
`'sync/'` 00:42:13–00:45:40 UTC. The `fold/` run started once the
1-minute load fell below 10. 1-minute load averages (`/proc/loadavg`), from
other lanes: 9.26 at the start, 11.73 between the two, 12.13 at the end,
and 9.09–18.65 between runs. An earlier pair of runs at load 16–37
(00:13–00:33 UTC) gave the same picture with wider swings. There the
candidate's after-100k round ratios were 1.02, 0.75 and 2.35, and the
slowest syncs reached 7–10 ms in both binaries: rounds where 50 syncs in a
row took over 1 ms, from contention. Those runs aren't used.

### `fold/`

| Case | Base median | Base fastest | Candidate median | Candidate fastest | Base / candidate | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| `fold/one more fold among 200k, wrapped` | 4.16 ms | 4.03 ms | 5.01 ms | 4.06 ms | 0.99 | not over 10% slower than base | meets (0.7%) |
| `fold/one more fold among 200k, wrapped, fresh tree` | 3.94 ms | 3.92 ms | 5.10 ms | 3.94 ms | 0.99 | not over 10% slower than base | meets (0.5%) |
| `fold/one more fold among 200k, wrapped, after 100k keystrokes` | 21.7 ms | 21.0 ms | 4.05 ms | 3.92 ms | 5.35 | not over 10% slower than base | meets |

Fastest per round, and the after-100k case over the fresh-tree case:

| Round | Base fresh | Base after 100k | Base ratio | Candidate fresh | Candidate after 100k | Candidate ratio (budget: at most 1.5) |
|---|---|---|---|---|---|---|
| 1 | 4.17 ms | 21.0 ms | 5.03 | 4.45 ms | 4.48 ms | 1.01 |
| 2 | 4.28 ms | 23.4 ms | 5.47 | 4.91 ms | 3.95 ms | 0.80 |
| 3 | 3.92 ms | 22.4 ms | 5.72 | 3.94 ms | 3.92 ms | 0.99 |

The candidate's `fold` code is base's. Its medians for the first two cases
(5.0–5.1 ms against 3.9–4.2) come from its noisier rounds 1 and 2. In
round 3 it matched base (4.06 and 3.94 ms against 4.03 and 3.92). The
walk is memory-bound and other lanes share the cache
(`docs/decisions/ez74dvmwrv-fold-rows-one-cursor-pass.md`).

### `sync/`

| Case | Base median | Base fastest | Candidate median | Candidate fastest | Base / candidate | Budget | Verdict |
|---|---|---|---|---|---|---|---|
| `sync/keystroke, 1M lines unwrapped` | 4.1 µs | 3.7 µs | 3.9 µs | 3.5 µs | 1.05 | not over 10% slower than base | meets |
| `sync/keystroke, 1M lines wrapped` | 17.3 µs | 15.2 µs | 6.5 µs | 6.4 µs | 2.37 | not over 10% slower than base | meets |
| `sync/keystroke, 1M lines unwrapped, 10k folds` | 16.3 µs | 14.9 µs | 15.0 µs | 14.6 µs | 1.02 | not over 10% slower than base | meets |
| `sync/keystroke, 1M lines wrapped, 10k folds` | 30.1 µs | 26.9 µs | 17.5 µs | 17.2 µs | 1.56 | not over 10% slower than base | meets |
| `sync/keystroke, 1M lines wrapped, 200k folds` | 239 µs | 233 µs | 229 µs | 226 µs | 1.03 | not over 10% slower than base | meets |

The wrapped keystrokes are faster because the splice no longer rebuilds
the tree's path. The unwrapped ones keep no tree, so they don't change.

### The slowest `map.sync` of the 100k typed keystrokes

The bench types them before the after-100k `fold/` case, timing each sync,
so every run of either filter types them once:

| Run | Base slowest | Base 10th slowest | Base syncs over 1 ms | Candidate slowest | Candidate 10th slowest | Candidate syncs over 1 ms |
|---|---|---|---|---|---|---|
| `fold/` round 1 | 1.85 ms | 59 µs | 1 | 45 µs | 18 µs | 0 |
| `fold/` round 2 | 3.07 ms | 124 µs | 4 | 200 µs | 22 µs | 0 |
| `fold/` round 3 | 200 µs | 49 µs | 0 | 366 µs | 18 µs | 0 |
| `sync/` round 1 | 4.05 ms | 3.04 ms | 18 | 233 µs | 14 µs | 0 |
| `sync/` round 2 | 744 µs | 59 µs | 0 | 1.06 ms | 46 µs | 3 |
| `sync/` round 3 | 321 µs | 82 µs | 0 | 40 µs | 17 µs | 0 |

**The candidate's slowest is 1.06 ms (budget 4 ms); base's is 4.05 ms.**
No candidate sync of this typing runs a compaction window: no keystroke
moved an item. A window, where one runs, is 384 items: 0.1–0.2 ms in the
measure test's slowest splices. The ms-scale outliers in either binary
are preemption at load 9–19 on 24 threads. A sync here does a few µs of
work.
