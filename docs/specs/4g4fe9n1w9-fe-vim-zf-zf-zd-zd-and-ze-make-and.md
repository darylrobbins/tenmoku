---
task_uid: "01m58zfwk69qb4qa4g4fe9n1w9"
task: "T212"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test fold_keys_make_and_remove_folds (top level, in crates/fe-vim/tests/fold_keys.rs) passes: zfj, zfip, 3zF and visual zf close folds over those rows with no undo step; zd opens the fold at the cursor, zD every fold nested there, zE every fold."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test fold_motions_move_between_folds (top level, same file) passes: zj and zk with and without a count go to the next candidate's first row and the previous one's last row, fail at the ends without moving, and dzj deletes as the module docs say."
    required: true
    manual: false
---

# fe-vim: zf, zF, zd, zD and zE make and remove folds, and zj and zk move between folds, over fastedit's folds (Mac T903)

## Problem

At Mac main 6f032b12 fe-vim's fold keys are `za zc zo zA zC zO zR zM zv` (`crates/fe-vim/src/folds.rs`);
`zf`, `zF`, `zd`, `zD`, `zE`, `zj` and `zk` cancel. VSCodeVim and IdeaVim map them. fastedit's folds are its
own: candidates come from syntax or indentation, and the editor can close any rows
(`EditorModel::fold_rows`, `unfold_rows`, `unfold_all`), but a fold exists only while closed. The user
approved these keys in the "Vim parity" track on 2026-10-17.

## Change

In `crates/fe-vim/src/folds.rs` (its `z` hook):

- `zf{motion or object}` (a new linewise `Operator::Fold`, `crates/fe-vim/src/args.rs`, with arms in
  `crates/fe-vim/src/operator.rs` and the visual operators, `crates/fe-vim/src/visual.rs`), `{count}zF`
  (count rows) and visual `zf` close a fold over the rows (`fold_rows`); it leaves no undo step.
- `zd` deletes the fold at the cursor (opens it, which in fastedit removes it), `zD` every nested fold at the
  cursor, `zE` every fold (`unfold_all`).
- `zj` and `zk` are motions (new `Motion` variants, dispatched to folds.rs from `motion::motion` in
  `crates/fe-vim/src/motion.rs`): to the start of the next fold candidate below / the end of the previous
  one above (`EditorHost::fold_ranges`, else indented blocks), `count` times; with an operator (`dzj`) they
  work as nvim's (probe linewise/charwise).
- The module docs' divergence note covers them: no open manual folds survive (a deleted or opened fold is
  gone), and candidates are fastedit's, so these keys aren't checked against nvim.

Tests in a new integration file `crates/fe-vim/tests/fold_keys.rs` (headless, indentation candidates), each
key and its count, `dzj`, visual `zf`.

## Implementation notes

- `zj`/`zk` need the host's fold ranges, which `motion::motion` isn't given: `folds::with_candidates`
  holds them (with the closed folds) in a thread-local while `motion::move_command` and
  `operator::operate` run, as `line_motions::with_view` does for the viewport.
- Probed nvim 0.12.4: `zj`/`zk` land at column 0; from a fresh operator state `dzj` is exclusive and
  charwise with `:help exclusive`'s adjustments, but nvim doesn't reset the motion type between
  commands, so after `j` its `dzj` is linewise. fastedit's is always exclusive (module docs).
- `zf` goes through `operator.rs`'s `>` row code; visual `zf` is intercepted before
  `visual::operate` (`folds::fold_selection`), so the visual operators only gained unreachable arms
  (scope widened to `visual/operate.rs`, `visual/block.rs`).
- The reducer now starts `z` with an operator pending (`reducer.rs`, scope widened); folds' `z`
  hook, first among the features with one, cancels every key but `j` and `k` there.
