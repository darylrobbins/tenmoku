---
task_uid: "01m4saeqwkkmwvdk3ndrs9fnns"
task: "T100"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test in tests/marks_special_oracle.rs: tests/corpus/marks_special.toml gains at least one case per finding F18, F21, F22, F23 and F24 (a clone's [ ] marks after adoption, \u003cC-v\u003ejy on 'abc\\ndef' giving ] at (1,1), a clone's . after a counted/block insert, counted inserts with an empty final segment and counted o/O, v$d over whole rows giving . at (1,0)); each matches nvim 0.12.4"
    required: true
    manual: false
  - id: "C2"
    text: "Every other corpus expected file is byte-identical and every earlier fe-vim test passes"
    required: true
    manual: false
---

# Follow-ups from review of T420: special-mark freshness, block-yank endpoints, counted-insert and charwise whole-row '.' marks (F18 F21-F24), checked against nvim (Mac T577)

Follow-up from review of T420 (vim [ ] . ^ and A-Z marks; built on argon as T42, landed in host stack 5 at 3eec614a). The review deferred these nvim-parity findings:

- F18 (insert.rs:448): special-mark freshness uses the repeat-change counter (last_change), so a clone's m[ / m] / yank marks are overwritten when its outcome is adopted. Track special-mark freshness independently of last_change and adopt each closing outcome only while its marks are still current.
- F21 (visual/block.rs:383): block-yank endpoints use a clipped normal-mode caret: on "abc\ndef" `<C-v>jy` puts `]` at (1,0); nvim puts it at (1,1). Resolve block mark columns independently of normal-mode clipping.
- F22 (marks.rs:1913): the counted/block-insert correction to `.` is applied only to the closing outcome, not published through Left, so a clone's `.` is wrong. Publish the corrected change mark through the batch metadata.
- F23 (insert.rs:1750): counted inserts whose final typed segment is empty, and counted o/O, leave `.` at the first copy. Determine the last change from the actual copied edits, including terminal line breaks and empty opened rows.
- F24 (marks.rs:950): a characterwise delete covering whole rows is read as linewise: on "abc\ndef\nghi" `v$d` leaves `.` at (0,0); nvim (1,0).

Each fix gets an oracle case in the marks corpus checked against nvim 0.12.4.

## Implementation notes

- F18: `Changes::closing` tracks which special marks were set since a session was left to another state ending it (`Marks::closing`, from `insert::end`'s clone path), per mark rather than by `VimState::changes`: a yank sets `'[`/`']`, `m[`/`m]` one of them, a change `'.` too, a newer session `'^`. `adopt_insert` takes only the rest. The batches the ending state makes after the clone left (the count's copies, `Esc`'s unused-indent removal, a block's other rows) are noted `Note::Ending`. A clone reading them doesn't overwrite its newer marks. Transaction ids can't tell those batches from the clone's own: a clone's edit made while the session's transaction is still open joins it.
- Notes (`Note`) now live in the `Feed` that every clone shares (capped at 64), not per state, so a clone reads a batch with what its command noted.
- F22: the counted/block-insert `'.` is noted with the session's last batch (`Note::Left`) and also carried in `marks::Left`. A clone running inside the ending state's measuring can read the copies' batch before the note is written, so adoption needs `Left` as well.
- F23: `repeat_typed` works out `'.` from the copies it puts in (`last_change`): the start of text typed after the last copy's last line break (past the indentation the session made there), else that row's start if nvim would have indented it only for `Esc` to take it out, else the line break. If `Esc` takes indentation out of the row it's left on, `'.` goes to that row's start. All were checked against nvim with `getpos("'.")`, indented rows included.
- F21: the review's `"abc\ndef"` example already matched nvim (`block_yank_short_mid_end`). The mismatch shows where the block's column is past a shorter row's end (`"abc\nd"` from (0,2): nvim `']` (1,1), fe-vim had (1,0)). The corners now lie past the row's end there. A corpus case can't see this: a mark jump clamps the column, and fe-vim (unlike nvim) clamps it in visual mode too. So the unit test `a_block_yanks_corners_lie_past_a_short_rows_end` holds nvim's `getpos()` values.
- F24: operator deletes that aren't linewise note `Note::Chars`, so whole rows they take leave `'.` on the row after, past the last row when they were the last (nvim's E19, as in `visual_delete_whole_mid_row_change`).
- The clone paths are covered by unit tests in `src/tests/marks_special.rs` (`closed_by_a_clone`, `adopted`). The corpus covers the same key sequences on one state against nvim.
