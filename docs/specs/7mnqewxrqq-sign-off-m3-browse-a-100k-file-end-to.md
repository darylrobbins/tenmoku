---
task_uid: "01m46tzwhg1vvadc7mnqewxrqq"
task: "T82"
rev: 10
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::m3_signoff::a_100k_file_project_end_to_end: on a 100,000-file fe_project::fixture, the tree loads only the root and one expansion lists once with only visible rows built; Cmd-P indexes 100,000 paths and a file-name query ranks that file first with indices_computed rising by at most 50; search finds a unique needle in exactly one file and a common one is capped with the indicator; clicking the match selects it; the opened Rust file shows 3 outline rows; revealing a deep file lists only its unloaded ancestors. Counts only, no timing assertion"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md has an M3 Browse sign-off section: a table of the finder target (50 ms to first results on 100k files), the provisional search target (50 ms to first batch) and the tree benches, each with the number from one run of cargo bench -p fe-project --bench tree/finder/search on a named commit and met or not met"
    required: true
    manual: false
  - id: "C3"
    text: "docs/decisions/m3-browse-signoff.md states for each M3 done-when item (100k-file tree, finder budget, search budget) met or not met, citing the test or PERF.md row, and for each miss names the cause and a proposed follow-up; no production code changes"
    required: true
    manual: false
---

# Sign off M3 Browse: a 100k-file end-to-end test, recorded budgets, and a status note

# Sign off M3 "Browse": a 100k-file end-to-end test, recorded budgets, and a status note

Milestone M3 "Browse", last card. The plan's M3 row reads: "**Done when:** Tree handles a 100k-file repo;
finder and search budgets met". Performance targets:

- **Finder:** first results under 50 ms on 100k files.
- **Search:** T65 set a provisional target of a first batch under 50 ms on its bench corpus. The plan
  gives no number.

## Problem

Each M3 card tests its own piece. Nothing checks the pieces together on a repo of M3's size, and no
document says whether M3's done-when holds.

## Change

- **End-to-end test.** New `crates/fe-workbench/src/tests/m3_signoff.rs`, declared in `tests/mod.rs`.
  It's **one** test, so `nextest`'s process-per-test generates the fixture once. It runs on a
  `fe_project::fixture` tree of 100,000 files (100 directories × 1,000) in a `tempfile` dir, in
  headless GPUI, and asserts behaviour and bounded work only (counts, never times):
  1. **Tree.** After `Workbench::new` and the background executor run, the tree has loaded only the
     root (`FileTree::stats().listings == 1`). Expanding one directory of 1,000 files raises listings
     by 1, and the frame builds no more rows than fit plus overscan.
  2. **Cmd-P.**
     - Open it; the walk indexes 100,000 paths.
     - Type a file's name: the first row is that file, and the query computed indices for at most 50
       results (`FinderStats::indices_computed`).
     - Enter opens it in a tab.
  3. **Search.**
     - A literal needle written into exactly one file finds exactly that file.
     - A common needle stops at the panel's cap with the truncated indicator shown.
     - Clicking the single match selects it in the editor.
  4. **Outline.** The opened file, a Rust file the test writes with three fns, shows three outline
     rows.
  5. **Reveal.** Opening a file deep in an unexpanded directory through Cmd-P lists only its unloaded
     ancestors.
- **Benches, recorded.** Rerun T61's, T62's and T65's criterion benches on one commit:
  - `cargo bench -p fe-project --bench tree`
  - `--bench finder`
  - `--bench search`

  Add an "M3 Browse sign-off" section to `PERF.md`. It's a table of each M3 target, the measured
  number, met or not met, and the commit. It links the per-bench sections. Nothing is asserted.
  Benches are not part of the gate.
- **Status note.** New `docs/decisions/m3-browse-signoff.md` states for each done-when item (100k-file
  tree, finder budget, search budget) whether it's met, with evidence: the test name or the PERF.md
  row. For any item not met, it names the cause and the follow-up the lead should file. The worker
  does not file tasks.

No production code changes in this card. If the end-to-end test finds a bug, block with it rather than
fixing it here.

## Out of scope

- GUI `--perf` runs, frame timing and startup numbers.
- New benches.
- Fixing misses.
- The plan's manual verification pass (`cargo run -p fastedit` on this repo), which the user does.

## Decided (lead defaults; the user may revisit)

- Search's sign-off target is T65's provisional 50 ms to first batch, since the plan sets none.
- The sign-off note lives in `docs/decisions/`, with the numbers in `PERF.md`.
