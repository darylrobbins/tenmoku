---
task_uid: "01m417z3kx7zf61xp0aehr7x68"
task: "T2"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax test tests::golden::highlights_match_golden_files passes: every language from language_names() has a sample in crates/fe-syntax/tests/golden/ whose whole-file highlights equal its .golden file"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test tests::golden::a_mismatch_shows_a_line_diff passes: changed lines are marked - (expected) and + (actual) with three lines of context"
    required: true
    manual: false
  - id: "C3"
    text: "the syntax_features gate stage passes"
    required: true
    manual: false
  - id: "C4"
    text: "the test stage's JUnit report shows tests::golden::highlights_match_golden_files taking under 1 s"
    required: true
    manual: false
---

# Add golden highlight files for every shipped language

## Problem

The plan's M2 exit criteria include golden highlight tests per language. Today each language has one small snippet with a few spans it must include (`each_language_highlights_a_snippet`, `crates/fe-syntax/src/tests.rs:41`). So a grammar or query crate bump, or a change to precedence (`earlier_patterns_win` in `crates/fe-syntax/src/language.rs`) or to `flatten` (`crates/fe-syntax/src/highlight.rs`), can change real-world output without failing anything. T-12 (injections) and T-14 (more grammars, query overrides) need this to show their effect as a diff.

## Change

- Samples live in `crates/fe-syntax/tests/golden/<language>.<ext>`: one realistic 50–100 line sample per language `language_names()` returns (tsx included), named so `language_for_path` resolves it and its file stem is the language name. Each sample is our own code, written for this; nothing copied from third-party projects. A `.gitattributes` there marks the files `-text`, since golden spans are byte offsets.
- Expected output sits next to each sample as `<sample>.golden`: one span per line, `row:col-col highlight "text"` (1-based rows, byte columns, end exclusive; `row:col-row:col` for a span over several rows; text `{:?}`-escaped).
- The harness is a **unit test module**, `crates/fe-syntax/src/tests/golden.rs`, declared with `mod golden;` in `crates/fe-syntax/src/tests.rs`, not an integration test in `tests/golden.rs`. Reason: `QUERY_BUDGET` (`crates/fe-syntax/src/syntax.rs:41`) is unlimited only under `cfg!(test)`, which an integration test doesn't set for the library, so a whole-file highlight there runs under the 2 ms production budget and could truncate spans on a loaded gate machine. `tests/golden/` has no `main.rs`, so Cargo won't treat it as a test target.
- `highlights_match_golden_files` parses each sample with no time limit, asserts the root has no error, highlights `0..len`, and compares with the `.golden` file. It skips samples whose language isn't in this build (feature matrix) and asserts every built language has a sample. On mismatch it fails with a line diff (`-` expected, `+` actual, three lines of context, `...` between hunks) for every differing file, and says how to regenerate.
- `UPDATE_GOLDEN=1 cargo nextest run -p fe-syntax golden` (or `cargo test`) rewrites the `.golden` files instead of comparing.
- Add a row for golden highlights to the table in `TESTING.md`.

## Prior attempt

Uncommitted Paddock work at `/Users/daryl/code/fastedit.worktrees/paddock-task-57a5495c-1` (based on `76c28ef`) is nearly complete. Copy from it; don't modify that worktree.

- Untracked `crates/fe-syntax/tests/golden.rs` (147 lines): the harness above, with a small LCS line diff, as an integration test. Move it to `src/tests/golden.rs` for the budget reason above and switch it to `crate::` paths.
- Untracked `crates/fe-syntax/tests/golden/`: 16 samples (61–93 lines each), their `.golden` files and `.gitattributes`. They were generated before T-08 (`14a80ca`) changed `flatten`. Regenerate on main and review the diff before committing. Check each sample is original.
- `TESTING.md`: one added table row; its regenerate command names `--test golden`, so update it.
- It has no test of the diff output; add one.
