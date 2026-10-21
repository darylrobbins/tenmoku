---
task_uid: "01m5fzzn6zsard547ja1y7jmzy"
task: "T267"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "history_overhead::default_limit_keeps_typing_undoable (88 s) takes under 5 s in a debug build on an idle machine (cargo nextest run -p fe-buffer default_limit_keeps_typing_undoable), still typing then undoing every edit against the default history limit and ending at \"F\""
    required: true
    manual: false
  - id: "C2"
    text: "tests::ideal::single_origin_matches_reference and tests::ideal::mixed_origins_match_reference (19 s, 18 s) each take under 5 s (cargo nextest run -p fe-buffer ideal::), with the session length unchanged and only the proptest case count lowered; avoidable_refusals_stay_rare is untouched"
    required: true
    manual: false
  - id: "C3"
    text: "tests::anchor_resolution_is_consistent_across_many_versions (14 s) and marker_set::sync_follows_map_offset (11 s) each take under 3 s, still covering anchors created at many versions and marker syncs across several rounds"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-buffer passes"
    required: true
    manual: false
---

# fe-buffer: cut history_overhead default_limit_keeps_typing_undoable, ideal single/mixed origin proptests, anchor_resolution and marker_set sync_follows_map_offset to seconds

Test-only speed-up, user-approved 2026-10-21 (the slow tail: 44 tests over 10 s take 20 of the suite's 46 CPU-minutes). Keep what each test proves; shrink inputs.

- history_overhead::default_limit_keeps_typing_undoable: 10,001 one-char edits plus 10,000 undos takes 88 s, so the cost is superlinear in the edit count. The point is that the default 16 MiB limit does not cap ordinary typing, and 10,001 one-char transactions are ~1 MiB at most, so 2,000 edits proves the same. Keep the default limit (do not call set_history_limit) and keep the final "F" assertion. If 2,000 is still over 5 s, it is a real quadratic in Buffer::edit or undo: say so in the hand-in rather than shrinking further.
- tests::ideal::{single,mixed}_origins_match_reference: proptest with the default 256 cases of run_session(seed). Set ProptestConfig cases to 32 in the existing config block (keep max_shrink_iters: 0). Do not shorten run_session; the avoidable_refusals_stay_rare test keeps 100 seeds for coverage.
- tests::anchor_resolution_is_consistent_across_many_versions: 500 steps, each re-resolving every earlier anchor (quadratic). 100 steps still has anchors from 100 versions.
- marker_set::sync_follows_map_offset: cases 256 -> 48 for this property only (the file's other properties are fast; leave them).
Pure fe-buffer, runs on Linux.
