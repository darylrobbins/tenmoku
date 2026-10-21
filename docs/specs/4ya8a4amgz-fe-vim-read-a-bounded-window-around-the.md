---
task_uid: "01m4sfxb888xc1784ya8a4amgz"
task: "T102"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Cursor::at no longer copies the caret whole row when the row has local word-class transitions. A test in the new crates/fe-vim/tests/caret_window.rs drives w, b, e, ge, iw, aw, f{char} and % from the middle of a single 100 MB row (alternating ASCII word, punctuation and blank runs) through HeadlessVim, checks each lands where it does on a short row with the same local text, and each key completes in under 50 ms in release (or under a step-count bound that the base exceeds). Known limit, documented in the lane docs file and filed as a follow-up: rows with uninterrupted letters, Cyrillic, emoji, tabs or blank runs, and directive detection past the first 16 KiB, may still read the whole row"
    required: true
    manual: false
  - id: "C2"
    text: "Window edges keep real grapheme boundaries: in caret_window.rs, a Prepend character (e.g. U+0600) before ASCII, a regional-indicator pair, and a ZWJ emoji sequence each straddling the window edge give the same w/e/x/iw results as on an unwindowed short row"
    required: true
    manual: false
  - id: "C3"
    text: "Every existing fe-vim test and oracle (motions, text objects, delimiters) passes unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "Nothing outside the scope changes except this lane docs/specs file"
    required: true
    manual: false
  - id: "C5"
    text: "Windowing never changes behaviour or panics: % beside a multibyte character on a huge row does not panic; number increment, sentence objects, counted word objects, counted replace, toggle case, visual counted l, visual vertical start/goal column, quote state across scan windows and a mark jump onto another long row all give the same results as on an unwindowed row, including near window edges, each with a regression test in caret_window.rs"
    required: true
    manual: false
---

# fe-vim: read a bounded window around the caret in Cursor::at instead of the whole row, with real grapheme context at the window's edges (Mac T594)

Split out of T564 (follow-ups from review of T417), round 2. That card's C8 bundled the delimiter clock checks with the shared caret read. The caret read moves here so T564 can land its delimiter work.

Today `Cursor::at` in crates/fe-vim/src/motion.rs builds the caret's whole row as a String. Every motion and text object that starts from the caret pays O(row length): on a 100 MB single-row file, that's a 100 MB copy per keystroke.

T564's lane made a first attempt: lane/gvmzwjxtgy, motion.rs +209, reverted from that lane before it lands. Codex left these open:
- F8: shared caret construction still isn't bounded on every path, and the performance evidence doesn't exercise real motions and text objects.
- F9: treating ASCII positions as grapheme boundaries is wrong under Unicode GB9b. A Prepend character joins the ASCII character after it. Regional-indicator pairs and ZWJ sequences need context on both sides of a window edge too.
Start from that attempt (git show the lane's motion.rs commits) or redo it.

Lead note: the Mac's first attempt is ~/coordination/neon-cards/T594-first-attempt.diff, a +209-line motion.rs diff from Mac T564's lane, base 5b11e1c4. It's the whole of T564's motion.rs change, so drop what this card doesn't use (e.g. `word_ranges` and the class atoms for T564's delimiter patterns). Start from it with `git apply --3way` or redo it. Codex's open findings on it are F8 and F9 above: bound the caret window on every path, and handle grapheme boundaries at the window's edges under GB9b (Prepend + ASCII, regional-indicator pairs, ZWJ sequences).

Respec (2026-10-19, lead, after codex round 3 left F1-F3, F5, F9-F12 open): C1 is narrowed to a documented known limit. Rows with at least three ASCII word-class transitions on each side of the caret must read a bounded window. Rows with uninterrupted letters, Cyrillic, emoji, tabs or blank runs (F3), and directive detection past the first 16 KiB (F12), may still do whole-row work; both are filed as a follow-up card. Record that limit in this lane docs/specs file. The correctness findings (F10 panic on multibyte before the caret, F1, F2, F9, F11, F5: window-relative coordinates reaching whole-row consumers) are NOT narrowed away: they are C5. Scope now includes the consumer files those findings cite (increment.rs, prose.rs, text_object.rs, edit.rs, visual.rs); an acceptable alternative for any consumer is to make it read the whole row explicitly instead of a window.

## Implementation notes

- `Cursor::at` reads a window of a row longer than four times `CARET_WINDOW` (16 KiB): that much each side of the caret, moved out to the third place where the class of two ASCII bytes changes. Edges only fall between two ASCII bytes, where no grapheme rule (Prepend, Extend, ZWJ, SpacingMark, regional-indicator parity) reaches across, so the window's graphemes are the row's. The class margin keeps `iw` / `aw` on a run from meeting the edge.
- `w b e W B E ge gE f F t T ; , h l 0 $` slide the window as they go (`Cursor::step`, `find_forward`, `find_backward`); every other motion calls `Cursor::widen` first and reads the row whole. Code outside motion.rs that reads `cursor.line` as the whole row (visual mode's `line.len()`, goal columns) sees the window on a huge row.
- `%` on a huge row reads `CARET_WINDOW`-sized spans (`Scan::span`). Its double-quote state is taken from each span's start, not the row's, since the row's quote parity can't be had without reading it all.
- Tests: tests/caret_window.rs (100 MB row; Prepend / regional indicator / ZWJ sequences swept across the window's edge, each side) and `caret_window_tests` in motion.rs (bytes read per key).

### Revision 4

- `Cursor::at` still reads a window on a huge row, but consumers that take `cursor.line` as the whole row now say so: `Cursor::whole` (increment, sentence objects in prose.rs, counted `r` and `~` in edit.rs, the visual cursor in visual.rs) and `Cursor::widen` (counted `iw` / `aw` in text_object.rs). `Cursor::motion` widens for every motion it doesn't slide; a widened caret reads whole rows wherever it goes (`bounded`), so `goto` onto another long row (mark jumps) gets that row whole.
- `%` and the bracket objects scan a huge row in 16 KiB spans only if it has no double quote: a string's state runs from the row's start, so a huge row with a quote is read whole, as before. Spans take their backslash context from before their start.
- `Scan::span` clips its start onto a char boundary (the F10 panic with a multibyte char beside the caret).
- visual.rs: an unmatched-bracket motion (`[(`, `])`, ...) in a visual mode starts from the cursor's true offset, not the clamped one (T143 F1).
- **Known limits (follow-up T197):** the window needs at least three ASCII word-class transitions on each side of the caret within reach; a row of uninterrupted letters, Cyrillic, emoji, tabs or blank runs may be read whole. Directive detection for `%` reads only the row's first 16 KiB. Consumers outside the scope files that take `cursor.line` as the whole row (operator.rs block put columns, ex/, visual/*.rs) still see the window on a huge row.

### Revision 4, round 2

- Window edges now lie at the third place, past the caret's reach, where an ASCII blank meets a non-blank (not any class change): a WORD, and so a word, is never cut by an edge, for every consumer (`iW` / `aW`, `cW` in insert.rs, `dW`). A row with no ASCII blanks (minified code) is read whole: this widens the F3 known limit (T197).
- `Line::start_column` and `goal_column` add the columns of the row before a window (taken lazily, once per tab width), so every consumer gets the row's own display columns. `Cursor::vertical` widens first, so its goal is searched on the actual row.
- Not fixed here, for T197: the linewise mark-yank landing (operator.rs, `Cursor::at(mark)` then `line.first_non_blank()`) takes the first non-blank of the window; it is outside this lane's scope.

### Revision 4, round 3

- Window edges prefer the third blank / non-blank place past the caret's reach (so no WORD is cut), looking up to `BLANK_REACH` (128 KiB) past it; with none there, they settle for the third word-class change. A row of alternating letters and punctuation (`a.a.a.`) is therefore read in windows again; a WORD longer than the window, with no blank within `BLANK_REACH`, can be cut.
- `iW` / `aW` (text_object.rs) read the row whole (`Cursor::widen`), like counted objects, so they take a WORD of any length.
- `%` reads a huge row's directive from as much of its head as it takes (`row_directive`), not a fixed 16 KiB.
- Revision 5 (scope widened to operator.rs and insert.rs): `insert::change_word` (`cW`) and the linewise mark-yank landing in operator.rs read the caret's row whole (`Cursor::whole`); regressions `change_big_word_takes_a_whole_row_word_with_no_blank` and `a_linewise_yank_up_to_a_mark_lands_on_the_rows_first_non_blank`.
- The carried T149 F1-F3 and T156 F2 findings in operator.rs (put-size preflight, put marks, nested `:normal` put landing) are unrelated to the caret window and are not addressed here: this lane changed one line of operator.rs.
- A class change counts toward a window's margin only where both ASCII bytes are graphemes of their own (`starts_alone`: the byte before the first is ASCII too), so a Prepend char joining a blank or punctuation can't make a run look cut (`word_objects_take_a_run_joined_by_prepend_chars`).
