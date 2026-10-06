---
task_uid: "01m46hs5pm69n5ks66ze0506g2"
task: "T62"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-project test tests::finder::walk_indexes_every_unignored_file: walk over a nested fixture indexes exactly its non-ignored files as /-separated relative paths: no directories, nothing under .git, nothing matched by .gitignore or .ignore; progress() then reports (file count, true)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test tests::finder::results_are_ranked_limited_and_highlighted: query returns at most limit results ordered by score, then shorter path, then path; each result carries ascending char indices into its path whose characters spell the query case-insensitively, including for a non-ASCII path"
    required: true
    manual: false
  - id: "C3"
    text: "fe-project test tests::finder::file_name_matches_rank_first: over from_paths fixtures, \"main\" ranks src/main.rs above a path that matches m-a-i-n only across directories, \"lib.rs\" ranks every path ending in /lib.rs above all other matches, and \"fe-ed/view\" finds crates/fe-editor/src/view.rs"
    required: true
    manual: false
  - id: "C4"
    text: "fe-project test tests::finder::empty_query_lists_paths_in_order: an empty query returns the first limit paths in path order"
    required: true
    manual: false
  - id: "C5"
    text: "fe-project test tests::finder::queries_see_paths_indexed_so_far: after extend with a first batch and before finish, query finds paths of that batch only and progress() reports (batch size, false); after a second batch and finish, both batches are found and progress() reports complete"
    required: true
    manual: false
  - id: "C6"
    text: "fe-project test tests::finder::a_cancelled_walk_stops: walk with the cancel flag already set indexes 0 paths"
    required: true
    manual: false
  - id: "C7"
    text: "fe-project test tests::finder::apply_updates_without_rewalking: after creating, deleting and renaming files and removing a directory on disk, apply with their FsChange::Path entries makes queries find the new and renamed paths and not the removed ones; stats().walks rises only by one per new directory; changes to ignored paths change nothing"
    required: true
    manual: false
  - id: "C8"
    text: "fe-project test tests::finder::a_100k_file_index_bounds_query_work: walk over the generated 100,000-file fixture indexes 100,000 paths, and a query with limit 50 returns 50 results while stats().indices_computed rises by at most 50"
    required: true
    manual: false
  - id: "C9"
    text: "crates/fe-project/benches/finder.rs builds in the gate and measures a walk of the 100k fixture and 1-, 3-, 8- and 20-char queries over 100k in-memory paths; PERF.md gains a Finder section with one run of these numbers next to the plan budget of 50 ms (recorded, not asserted)"
    required: true
    manual: false
  - id: "C10"
    text: "cargo deny check licenses bans sources passes with frizbee added, and frizbee adds no other crate to Cargo.lock"
    required: true
    manual: false
---

# Index project paths in the background and fuzzy-match them with frizbee for Cmd-P

# Index project paths in the background and fuzzy-match them with frizbee for Cmd-P

Milestone M3 "Browse", card 2 of the first batch. Depends on card 1 (`fe-project`: its listing
rules, `FsChange`, the fixture generator, and the crate itself). The Cmd-P palette UI is a later
card on top of this one and card 3.

## Problem

M3 is done when "finder and search budgets are met". The plan's finder target is first results in
under 50 ms on 100k files. Cmd-P can't wait for a tree that loads lazily. It needs every non-ignored
file path indexed up front on a background thread, matched fuzzily, and kept current from the same
watcher batches the tree uses.

## Licensing (checked 2026-10-05)

`frizbee` 0.13.0 (https://github.com/saghen/frizbee) is MIT (LICENSE: "Copyright (c) 2024 Liam
Dyer"). Its only optional runtime dependency is `serde`, which we don't enable. `nucleo` (MPL) is a
**dev**-dependency only, so it never enters our graph. Its rust-version is 1.89, and we're on 1.98.
It's already allowed by `deny.toml`, so no policy change is needed. A new crate in `Cargo.lock` needs
a person's approval at merge (`human_paths`), which is expected.

## Change

- Workspace `Cargo.toml`: `frizbee = "0.13"` (default features) in `[workspace.dependencies]`;
  `crates/fe-project/Cargo.toml` uses it and adds `[[bench]] name = "finder"`.
- New `crates/fe-project/src/finder.rs`, declared and re-exported from `crates/fe-project/src/lib.rs`:
  - `FinderIndex`: relative paths as `/`-separated `String`s. Non-UTF-8 names are stored lossily,
    with the real `PathBuf` kept alongside. Files only, not directories. The ignore rules are the
    same as `FileTree` with `show_ignored: false` (reuse `listing.rs`'s builder settings), and `.git`
    is always excluded.
  - Deterministic building API: `FinderIndex::new(root)`, `extend(batch)`, `finish()`,
    `progress() -> (indexed, complete)`, and `from_paths(iter)` for tests and benches with no
    filesystem.
  - `FinderIndex::walk(root, cancel: Arc<AtomicBool>, sink)` runs `ignore::WalkBuilder::build_parallel`
    on the calling (background) thread. It sends batches to `extend` and stops when `cancel` is set.
    The caller owns the thread or executor, which keeps the crate executor-free.
  - `query(&mut self, text, limit) -> Vec<FinderMatch { path, score, indices }>`. frizbee's
    `Matcher::match_list` runs over the indexed paths. The top `limit` are kept by score, then shorter
    path, then path order, and match indices (`match_one_indices`) are computed for those results
    only. `indices` are ascending char indices into `path`. An empty query returns the first `limit`
    paths in path order.
  - Filename preference: a query that matches within the file name beats one that spans directories.
    For example, a basename bonus is added to frizbee's score, or the basename and the full path are
    matched and the better score is kept. The implementer picks the scheme. The tests below fix the
    behaviour.
  - `apply(&[FsChange])` reconciles like `FileTree::apply`. A changed path is re-stat'ed: an existing,
    non-ignored file is inserted or kept, and a missing one is removed. A missing directory removes
    every indexed path under it. A new directory is walked (only it). `Rescan` re-walks. Nothing
    re-walks the whole root except `Rescan`.
  - Counters for tests and benches, `stats() -> FinderStats { walks, indices_computed }`.
  - Queries run over the paths indexed so far while building, so Cmd-P shows results before the
    walk finishes.
- Tests in a new `crates/fe-project/src/tests/finder.rs`, declared from `crates/fe-project/src/tests/mod.rs`.
- Bench `crates/fe-project/benches/finder.rs`:
  - a full `walk` of the 100k-file fixture from `fe_project::fixture`
  - `query` with 1-, 3-, 8- and 20-char needles over 100k in-memory paths (`from_paths`, realistic
    `crates/<name>/src/<dir>/<file>.rs`-shaped paths), limit 50
  - with and without frizbee's `match_list_parallel`, if the single-threaded number is near the
    budget

  Record one run's numbers in a new "Finder (`cargo bench -p fe-project --bench finder`)" section of
  `PERF.md`, against the plan's 50 ms target. Nothing is asserted.

## Out of scope

- The Cmd-P palette UI, recent-files ordering, and opening a result. Those are a later card on top of
  this one and card 3.
- Typo tolerance: keep frizbee's default `max_typos: Some(0)` for now.

## Decided (lead defaults; the user may revisit)

- Results are ordered by score only. Favouring recently opened files is a later card; keep the API
  simple and add no hook for it now.
