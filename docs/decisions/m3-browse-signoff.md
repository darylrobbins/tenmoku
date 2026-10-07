# M3 "Browse" sign-off

Status: **M3's done-when holds: all three items met.** Checked 2026-10-07
(card T82). The plan's row reads "Done when: Tree handles a 100k-file repo;
finder and search budgets met". The numbers are in
[PERF.md, "M3 Browse sign-off"](../../PERF.md#m3-browse-sign-off), from one
run at commit `b16f69b`.

| Done-when item | Met? | Evidence |
|---|---|---|
| Tree handles a 100k-file repo | met | `tests::m3_signoff::a_100k_file_project_end_to_end` (fe-workbench): on 100,000 files the tree lists only the root, one expansion lists once and builds only the visible rows. PERF.md rows: `open` 0.25 ms, `expand` of 10,000 files 14 ms. |
| Finder budget: first results under 50 ms on 100k files | met | Same test: Cmd-P indexes every path, and a file-name query ranks its file first having computed at most 50 match indices. PERF.md: 1.4 to 2.5 ms per query. |
| Search budget (provisional: first batch under 50 ms, T65; the plan gives no number) | met | Same test: a unique needle is found in exactly one file, a common one stops at the cap with the indicator, and clicking the match selects it. PERF.md: first batch 0.93 ms. |

The same test also covers the outline (three rows for a Rust file with three
fns) and revealing a deep file (only its unloaded ancestors are listed).

## Misses

None, so no follow-ups.

## Caveats

- The bench run was on a machine with a load average of about 90, so the
  numbers are pessimistic; each target was still met by more than an order
  of magnitude. Quieter-machine numbers are in the per-bench sections of
  PERF.md.
- The search target is the provisional one T65 set, and the benched corpus
  is 10,000 files (40 MB), not 100,000. Searches that find little read every
  file (about 600 ms here for 10,000), which is not part of either budget.
- The test asserts counts only; it checks no time.
- Not done here, as the card says: GUI `--perf` runs, frame timing and the
  plan's manual pass (`cargo run -p fastedit` on this repo).
