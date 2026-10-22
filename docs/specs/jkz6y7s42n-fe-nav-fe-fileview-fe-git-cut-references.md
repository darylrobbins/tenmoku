---
task_uid: "01m5fzzegbq8c5j2jkz6y7s42n"
task: "T216"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "references::dropped_hits_count_towards_the_cap (19 s) takes under 5 s in a debug build on an idle machine (cargo nextest run -p fe-nav references); the legacy-line test references_encoding::a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work stays at about 7 s idle, a documented limit (its shrunk baseline falls under the 0.3 s floor that keeps the ratio load-proof), to move to the slow set"
    required: true
    manual: false
  - id: "C2"
    text: "changed::truncation_and_rewrites_never_panic (21 s) takes under 5 s (cargo nextest run -p fe-fileview changed)"
    required: true
    manual: false
  - id: "C3"
    text: "stage_many_paths::stage_and_unstage_more_paths_than_arg_max (13 s) takes under 5 s (cargo nextest run -p fe-git stage_many_paths) and still stages more paths than the argument limit the fixture assumes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-nav, cargo nextest run -p fe-fileview and cargo nextest run -p fe-git pass"
    required: true
    manual: false
---

# fe-nav, fe-fileview, fe-git: cut references dropped_hits and legacy-line, changed truncation fuzz and stage_many_paths tests (13 to 21 s) (Mac T1070)

Test-only speed-up, user-approved 2026-10-21. T336 and T710 made the legacy-line test a ratio of two timed runs; keep its ratio design (eighth vs full, cancel at 24x).

- dropped_hits_count_towards_the_cap: 10,000 comment hits to drop with a cap of 1,000. The properties are "dropped + kept == hits" and "truncated". Use 2,500 hits with a cap of 250 (the same 10x) so the dropped count stays well over the cap.
- a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work: the doc says a 0.7 s baseline is needed so load does not swamp it, so shrink the full line from 8,000 to 4,000 hits and the eighth to 500 (same 8x), only if the eighth stays at least 0.3 s; otherwise leave this one alone and say so in the hand-in (the lead puts it in the slow group).
- truncation_and_rewrites_never_panic: 4 MiB file, 1,000 iterations, up to 64 KiB random writes and a fresh FileView every 20th. Use a 1 MiB file (set_len ceiling 2 * MIB instead of 5 * MIB) and 250 iterations, a fresh view every 5th; same seed and same three op kinds.
- stage_and_unstage_more_paths_than_arg_max: many_names() builds enough paths to exceed ARG_MAX; each round stages and unstages them via git subprocesses. Keep the count that crosses the chunking threshold (read the constant the git wrapper chunks on; use 1.5x it, not more), and keep both rounds.
Pure Rust, runs on Linux.

## Implementation notes

- `dropped_hits_count_towards_the_cap`: left alone. The cap is the `MAX_HITS` const (10,000) in fe-nav's src, not the `search(root, N)` argument (that is `verify_files`), so fewer hits would never truncate and the property would be lost. It takes 3.7 s on an idle machine, already under 5 s (the 19 s was under load).
- `a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work`: left alone. With the eighth at 500 hits it measured 0.20 s, under the 0.3 s floor in the spec. The full test takes about 7 s idle; for the lead's slow group.
- `truncation_and_rewrites_never_panic`: 1 MiB file, 250 iterations, fresh view every 5th, set_len ceiling 2 MiB (0.9 s).
- `stage_and_unstage_more_paths_than_arg_max`: `git add`/`reset` read paths from stdin, so there is no chunking constant to read. Instead of fewer paths, the paths got longer: the directory is nested to about half of PATH_MAX (portable, via `getconf`), so far fewer files still exceed ARG_MAX (2.6 s; the discard test shares `many_names` and is faster too).
