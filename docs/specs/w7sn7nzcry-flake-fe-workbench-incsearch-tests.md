---
task_uid: "01m5kq9rf1640d7aw7sn7nzcry"
task: "T231"
rev: 1
type: "chore"
complexity: ""
criteria:
  - id: "C1"
    text: "The two previewing tests (typing_a_search_reveals_and_escape_restores and a_typed_search_paints_over_an_open_find_bar) no longer depend on wall time: the preview and command search limits are set effectively unlimited and restored by a Drop guard, so a loaded machine cannot make the preview find no match."
    required: true
    manual: false
  - id: "C2"
    text: "No 'unhurried' guard or skip-on-slow branch is added to either test."
    required: true
    manual: false
  - id: "C3"
    text: "Proof: 20 consecutive runs of typing_a_search_reveals_and_escape_restores (cargo nextest, --retries 0) pass while 3x-ncpu 'yes' burners keep load above 30 (no scripts/load.sh exists; use yes/xargs -P CPU burners)."
    required: true
    manual: false
---

# Flake: fe-workbench incsearch_tests typing_a_search_reveals_and_escape_restores fails under load (search preview hits its 100 ms wall-clock limit, no match previewed) (Mac T1087)

Seen once in a macOS stack gate (gate-stack-55, .worktrees/gate-stack-55.test.run3b.log, load ~119), 0.72 s run:
crates/fe-workbench/src/vim_line/incsearch_tests.rs:70 (assert_eq!(found, Some(world..world + 3)) after typing "wor" into the / line)
assertion left == right failed; left: None, right: Some(1140..1143)
i.e. the editor's search_preview() was empty: the preview found no match. Passed 3/3 in isolation and in the run3 rerun.

Cause: same family as Mac T1084 / T1085 and argon T228. Typing in the / line previews through fe-vim cmdline.rs search_preview, which runs the match under Deadline::after(search_preview_limit().min(search::time_limit())) (100 ms SEARCH_PREVIEW_LIMIT). Under heavy load the wall clock cuts the search off, no match, no preview.

Fix direction (test-only): do not race the wall clock. fe_vim::set_search_preview_limit(Some(Duration)) is already public and on main (T1085; fe-workbench depends on fe-vim). In this test (and the sibling a_typed_search_paints_over_an_open_find_bar in the same file, which also previews) set it to a large value and fe_vim::search::set_time_limit likewise, restoring both via a Drop guard, as T1085 did in fe-gpui incsearch_fold_tests. T228 (argon, in Mac stack 55's picks, not on main) adds the same style of guard to fe-gpui vim/incsearch_tests.rs plus fe_vim::search_preview_limit(); this card does not need it (set_search_preview_limit and set_time_limit exist on main), and T228 has no Mac task so there is no --depends-on. Assert on the work (match found), never add a skip-under-load guard. Chore: test-only, no regression test.

## Implementation notes

C3 proof: 36 (3x the 12 CPUs) `yes` burners were started and the 1-minute load
average was awaited until it passed 35 before the first run. Then
`cargo nextest run -p fe-workbench --retries 0 typing_a_search_reveals` ran 20
times in a row, sampling the load before each: 35.16 at run 1, rising to 37.08
at run 20 (37.39 after), so it stayed above 30 throughout. All 20 passed.
