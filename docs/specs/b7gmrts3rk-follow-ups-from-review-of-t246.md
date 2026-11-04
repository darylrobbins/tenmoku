---
task_uid: "01m6jp9z6yh4jrzqb7gmrts3rk"
task: "T445"
rev: 4
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T246

Findings of the review of T246 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/insert/keys.rs:981: Characterwise <C-r><C-o> and <C-r><C-p> incorrectly overwrite text. Neovim inserts these forms even in Replace mode: on "abc\nxyz uvw", yiwjRQ<C-r><C-o>0<Esc> produces "abc\nQabcyz uvw", whereas this branch produces "abc\nQabcuvw". C-p has the same defect.
  fix: Preserve the insertion semantics of the C-o and C-p forms for characterwise registers, including indentation and backspace behavior. Add recorded oracle cases for both forms.
  instances: crates/fe-vim/src/insert/keys.rs:981

- F2 medium crates/fe-vim/src/insert/keys.rs:919: Excluding Replace mode from register-key recording makes counts and dot replay lose C-o/C-p register insertions. Linewise insertions bypass the replace ledger entirely, so replay omits the inserted lines. For example, on "abc\ndef\nghi", yyjRQ<C-r><C-o>0W<Esc>j. should insert another "abc" line before the final row; the candidate only replays W. The documented limitation contradicts the requested repeat behavior.
  fix: Record and replay these register operations in Replace sessions, preserving their identity, linewise placement, and surrounding typing. Add counted and consecutive-dot oracle cases for both C-o and C-p.
  instances: crates/fe-vim/src/insert/keys.rs:919

- F3 medium crates/fe-vim/src/insert/keys.rs:983: Sending a multiline characterwise register through replace_text treats its newline as ordinary Replace-mode Enter, losing text Neovim retains. On "ab\ncd\nxyz tail", vjyj0R<C-r>0<Esc> produces "ab\nab\nccd\nxyz tail" in Neovim; this implementation produces "ab\nab\nc\nxyz tail". The same path handles the C-r literal form.
  fix: Implement Neovim's multiline register insertion semantics while retaining the restoration ledger, and add multiline characterwise cases for plain C-r and C-r C-r, including backspacing across the newline.
  instances: crates/fe-vim/src/insert/keys.rs:983

- F4 medium crates/fe-vim/src/insert/keys.rs:994: Linewise C-o/C-p insertions bypass the replace ledger and leave the caret at a row start with no matching ledger entry, making immediate <BS> a no-op. On "abc\ndef\nghi", yyjRQ<C-r><C-o>0<BS><Esc> produces "abc\nabcdQef\nghi" in Neovim, while the candidate leaves the inserted line intact. C-p follows the same path.
  fix: Track the linewise insertion boundary and implement its Neovim backspace transition. Add immediate and repeated backspace oracle cases for both forms.
  instances: crates/fe-vim/src/insert/keys.rs:994

## Lead decision on the block (helium core, 2026-11-04)

crates/fe-vim/src/replace.rs is now in scope. Build the replace-stack model the worker's notes describe, beside the existing entries: an nvim-faithful <BS> pop, with replay (keys_of, retype, repeat_typed) and counts included. Fix F1 to F4 together; they share this model.

Commit the 22 recorded oracle cases into crates/fe-vim/tests/corpus/insert_registers.toml with their expected output, recorded from local nvim 0.12.4, before writing the fix. Existing replace-mode oracle cases must keep passing unchanged. If a case still can't match after the model is in, block with the case named rather than editing its expected output.

## Implementation notes

Blocked, not implemented. Recorded against nvim 0.12.4 (22 new oracle cases, all
four findings reproduce), the fix isn't a local patch to `keys.rs`:

- nvim's Replace-mode backspace pops a *stack*, not text tied to a position.
  Text inserted by `<C-r><C-o>`/`<C-r><C-p>` (characterwise or linewise) and by
  a multiline `<C-r>` pushes nothing, so the next `<BS>` pops an older entry and
  overwrites the character before the caret with it. `yiwjRQ<C-r><C-o>0<BS>`
  gives `Qabxyz uvw`; `yyjRQ<C-r><C-o>0<BS>` gives `abcdQef` (the `d` comes back
  in the wrong place after the break joins). A further `<BS>` with an empty
  stack only moves the caret; at column 0 it joins rows.
- A multiline characterwise `<C-r>` inserts and overwrites nothing (`ab`,
  `ab`, `ccdefgh`), unlike a single-row one.
- `replace::Entry` (replace.rs, out of scope) is keyed to live text before the
  caret, so it can't express those pops. Matching needs a replace-stack
  model beside the entries, replay (`keys_of`, `retype`, `repeat_typed`) and
  counts included.

### Implemented (replace-stack model)

- `replace::Entry` gains `stackless`/`popped`. Register text nvim's replace stack
  knows nothing of (`<C-r><C-o>`, `<C-r><C-p>`, and characterwise text with a line
  break) goes in via `VimState::insert_stackless` and is noted as a stackless entry.
  `stack_back` is `<BS>` after it: pop the stack's top whatever is before the caret
  (overwriting the character before it, or joining at a row's start and putting the
  popped characters back); with the stack empty only step back.
- Register keys are noted (and replayed by `.` and counts) in replace sessions too.
- `visual::typing` keeps `'<`/`'>` before such text; a blank last row of
  `<C-r><C-p>` characterwise text adds no indent.
- Known limits: virtual replace (`gR`) keeps the old overwrite behaviour; a plain
  `<C-r>` of multiline text isn't replayed by `.`/counts; a `<BS>` in a count's
  copies right after a register insert takes nothing back.
