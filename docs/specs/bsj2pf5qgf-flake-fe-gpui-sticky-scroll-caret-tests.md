---
task_uid: "01m65hvynyxgdt41bsj2pf5qgf"
task: "T1245"
rev: 4
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Cause: under load (T1213 snapshot gate, an fe-vim-only change; log ~/.local/state/regatta/runs/01m65fy6hggz7w8q0bp8fh8q4a/test.fastedit.log line ~703) assert!(row_top(\u0026view, found, cx).is_some(), \"previewed\") failed after 0.99 s; it also failed in 2 earlier stored run logs (2026-10-24, 2026-10-26), so it predates T1213. Suspected: EditorView::preview_search (fe-gpui vim.rs:1363) calls VimState::search_preview (fe-vim cmdline.rs:131-133), which searches under a 100 ms wall-clock Deadline (SEARCH_PREVIEW_LIMIT); under load it expires, nothing is previewed, autoscroll is never set and the match row is not in the frame. Record in the spec's notes what the investigation shows for this theory (the call path, and whether the failure reproduced at base in the recorded runs); it does not reproduce on demand, so confirming or refuting it is not required, only stating what was and was not shown."
    required: true
    manual: false
  - id: "C2"
    text: "In crates/fe-vim/src/cmdline.rs make the set_search_preview_limit override thread-local (a thread_local Cell\u003cOption\u003cDuration\u003e\u003e replacing the static AtomicU64 PREVIEW_LIMIT_OVERRIDE), keeping the signatures of set_search_preview_limit and search_preview_limit and updating the doc comment from process-wide to this thread. Every read is synchronous on the thread that sets it (VimState::search_preview, the element.rs paint), so parallel tests no longer see each other's overrides. No existing caller changes: fe-gpui incsearch_tests, incsearch_fold_tests and fe-workbench vim_line/incsearch_tests keep working unedited."
    required: true
    manual: false
  - id: "C3"
    text: "In crates/fe-gpui/src/view/sticky_scroll_caret_tests.rs, at the start of a_search_preview_while_parsing_clears_the_band_off_its_match call fe_vim::set_search_preview_limit(Some(Duration::from_secs(60))) and restore it with set_search_preview_limit(None) in a drop guard, as crates/fe-gpui/src/vim/incsearch_tests.rs does. Keep every existing assertion."
    required: true
    manual: false
  - id: "C4"
    text: "If the cause turns out to be elsewhere (the 60 s limit does not stop the failure, or the preview is lost for another reason such as draw or scroll ordering or a production bug in vim.rs), block and tell the lead: it becomes a bugfix with a regression test in a new file under tests/ declared from a tests/**/mod.rs, using only base APIs."
    required: true
    manual: false
  - id: "C5"
    text: "Proof, recorded in the spec notes: with cargo test (one shared process, not nextest) the sticky test passes 20 runs in a row while the rest of the fe-gpui lib tests run alongside it, and the per-test failure counts of the fe-gpui incsearch tests in those runs, and in 10 runs at base, are recorded for information: they flake under cargo test at base too (T1248 tracks it), so they need not pass and no statistical comparison with base is required. Existing tests pass unedited; clippy and fmt pass for fe-vim and fe-gpui. element::minimap::tests::a_cold_fence_seen_only_in_the_minimap_gets_its_colours passes in the nextest gate on main; if it fails only under cargo test, note it and do not chase it."
    required: true
    manual: false
  - id: "C6"
    text: "Nothing outside the scoped files changes, except this lane's own docs/specs file."
    required: true
    manual: false
---

# Flake: fe-gpui sticky_scroll_caret_tests a_search_preview_while_parsing_clears_the_band_off_its_match fails 'previewed' under load: row_top(found) is None after preview_search (sticky_scroll_caret_tests.rs:367)

## Implementation notes

**C1 (cause): the deadline theory is neither confirmed nor refuted.**

Call path (read, not traced at runtime): `EditorView::preview_search`
(fe-gpui vim.rs) calls `VimState::search_preview` (fe-vim cmdline.rs), which
searches under `search_preview_limit()`, 100 ms (`SEARCH_PREVIEW_LIMIT`) by
default. If the deadline expires nothing is previewed, autoscroll is never
set and the match row is not in the frame, which is the observed
`row_top(found)` is `None` at `previewed`. That fits the failure, and the
failure appeared under load (T1213 gate and two earlier logs).

Shown: with a 60 s override the sticky test passed 20/20 `cargo test` runs
beside the rest of the fe-gpui lib tests.
Not shown: the failure never reproduced at base (0/10 failures in the recorded
runs), so those passes cannot tell deadline expiry from another intermittent
cause. No run caught the failure with the deadline observed expiring, and no
controlled run (for example a tiny limit) was done. The override is a
precaution matching what the other preview tests already do.

**First attempt, rejected.** A drop guard on the old process-wide
`AtomicU64` override made parallel `vim::incsearch_tests` fail: the guard's
`None` reset landed while they held their own 60 s override. Hence C2: the
override is now a `thread_local!` `Cell<Option<Duration>>`.

**C5 proof**, `cargo test -p fe-gpui --lib` (one process), failing tests per run.

20 runs with the change:

| Test | Failed runs |
|---|---|
| sticky `a_search_preview_while_parsing_clears_the_band_off_its_match` | 0 |
| `incsearch_tests::a_preview_reveals_its_match_and_a_cancel_puts_the_view_back` | 4 |
| `incsearch_tests::turning_vim_off_ends_the_preview` | 3 |
| `incsearch_tests::a_previews_matches_paint_within_its_time_limit` | 2 |
| `incsearch_tests::a_preview_opens_the_fold_hiding_its_match` | 1 |
| `incsearch_fold_tests::edits_on_both_sides_keep_the_fold` | 1 |
| `element::minimap::tests::a_cold_fence_seen_only_in_the_minimap_gets_its_colours` | 20 |

10 further runs, change vs base `1de0c9c`:

| Test | Base | Change |
|---|---|---|
| `a_previews_matches_paint_within_its_time_limit` | 1 | 2 |
| `a_preview_reveals_its_match_and_a_cancel_puts_the_view_back` | 1 | 0 |
| `turning_vim_off_ends_the_preview` | 1 | 0 |
| `incsearch_set_through_the_shared_registers_reaches_another_editor` | 1 | 0 |
| sticky test | 0 | 0 |
| minimap `a_cold_fence...` | 10 | 10 |

These counts are for information only, with no statistical comparison with
base claimed. Rates varied with load between batches (the 20-run batch was
worse than the 10-run ones). Incsearch tests flake at base too (T1248).
`a_cold_fence` fails under `cargo test` at base as well; not chased.

Panic messages seen:
- `a_previews_matches_paint_within_its_time_limit` (incsearch_tests.rs:330, both
  sides): `assertion left == right failed, left: None, right: Some(1..2)`. The
  test already sets a 3600 s limit, so it is not the 100 ms deadline.
- `a_preview_reveals_its_match…` (:144), `turning_vim_off_ends_the_preview`
  (:367), `incsearch_set_through_the_shared_registers…` (:391), all base:
  `assertion failed: shown(&view, cx).2 > 300.`
