---
task_uid: "01m4mgtg35vrtcm85frh58kkr5"
task: "T343"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T224

Findings of the review of T224 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/commands.rs:318: Visual puts bypass the new cutting/deleting adjustments. On "abc\ndef\nghi" from (0,1), ma0ylvj"0p incorrectly deletes mark a; Neovim preserves it at (0,1). Conversely, ma0ylV"_p retains the replaced row's mark, whereas Neovim deletes it. Both p and P are affected.
  fix: Apply selection-aware mark adjustments around visual puts, preserving charwise boundary-row marks and removing linewise replaced-row marks; add oracle cases for p, P and empty-register puts while keeping src/visual/ unchanged.
  instances: crates/fe-vim/src/commands.rs:318

- F2 medium crates/fe-vim/src/insert.rs:1228: The typing adjustment covers ordinary and resumed insertion, but initial block sessions use insert::block and never call it. On "abc\ndef\nghi" from (0,1), ma0<C-v>I<CR><Esc> moves mark a to row 1, whereas Neovim keeps it at (0,1). Block c beginning at column zero has the same defect.
  fix: Apply marks.typing when initial block insertion/change sessions begin, using their insertion position and snapshot, and add block I and c cases that insert a line break at column zero.
  instances: crates/fe-vim/src/insert.rs:1228

- F3 medium crates/fe-vim/src/marks.rs:267: Deletion boundaries retain the topology from mark creation until a Vim editing command calls follow. Set a at (1,1) in "aa\nbb", externally insert "\ncc" at 5, then externally delete 3..6: a incorrectly survives at (1,1) on cc instead of becoming unset. End::Kept similarly misses deletion of an originally sole row after another row is appended. These edits are unambiguous and require no intervening Vim command.
  fix: Track row lifetime and refresh deletion boundaries through each external edit batch, rather than resolving frozen boundaries only against the final snapshot; test consecutive external edits for originally last and sole rows.
  instances: crates/fe-vim/src/marks.rs:88, crates/fe-vim/src/marks.rs:89, crates/fe-vim/src/marks.rs:131, crates/fe-vim/src/marks.rs:132, crates/fe-vim/src/marks.rs:267

- F4 medium crates/fe-vim/src/operator.rs:683: Charwise mark adjustments run separately for each caret before deletion ranges are merged. With "a\nbb\ncc\ndd", mark b at (1,1), mark a at (3,1), and carets at (0,0) and (1,0), d`a merges into deletion 0..9. The second cutting call protects b as its first-row mark, so b survives on the remaining "d", although its entire row was removed inside the merged deletion.
  fix: Compute mark preservation from the effective merged deletion ranges, protecting only their actual boundary rows; cover overlapping multi-caret delete and change motions.
  instances: crates/fe-vim/src/operator.rs:683, crates/fe-vim/src/marks.rs:252

## Implementation notes

- F3: marks follow the buffer's edit batches (a `Subscription` started with the first mark) and each `Place` carries a `Life` (row start/end, replayed per batch from the text it left); the frozen `End` anchors remain the fallback when a place's life isn't current.
- F1: `visual::replaced` reports the selection (charwise range or rows) and `vim.visual_put` applies `cutting`/`deleting` before `visual::put`, restoring the marks if the put is refused. `src/visual/` is unchanged.
- F2: `insert::block` calls `marks.typing` for the first row. Block cases are unit tests, not corpus cases: nvim keeps `'<`/`'>` at (0,0) there while fe-vim moves them, a separate divergence.
- F4: `operator.rs` merges the carets' overlapping ranges before `cutting`/`typing`. nvim has no multi-caret, so that's a unit test.
