---
task_uid: "01m5sq8b97mazgamptxwqn0fff"
task: "T1174"
rev: 5
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "peek_robustness_tests::encoded_targets: a CRLF target, a UTF-8 BOM target and a UTF-16 (or latin-1) target each highlight exactly the definition range and Enter opens at it, closed and open in a tab"
    required: true
    manual: false
  - id: "C2"
    text: "peek_robustness_tests::select_before_load: selecting the second list row before its baseline loads never shows or opens the first target's excerpt or range; it shows the right excerpt once loaded"
    required: true
    manual: false
  - id: "C3"
    text: "peek_robustness_tests::resize_refits: resizing the window with the reservation row count unchanged re-reveals the widget, and refit scrolls to the definition's current anchor, not the line cached at open"
    required: true
    manual: false
  - id: "C4"
    text: "peek_robustness_tests::target_edits_keep_range: edits in the target buffer before, inside and spanning lines of the definition keep its start and end columns and multi-line span; an edit large enough that the diff is skipped re-resolves the definition rather than keeping the stale range; a multi-line definition in an unedited target highlights and opens its full range, every row and both end columns (T1149 review F13); inserting 'pub fn hello() {} ' at the start of a row holding 'pub fn helper() {}' keeps the definition on helper (7..13 maps to 25..31), with a mapper unit test and a Peek/Enter round trip, so a shared character prefix or suffix between different identifiers never captures an endpoint (T1174 review F6)"
    required: true
    manual: false
  - id: "C5"
    text: "T1149's peek_tests pass unchanged; nextest and clippy pass"
    required: true
    manual: false
---

# Fix Peek Definition's known limits: encoded and CRLF targets, list selection before load, resize refit, and ranges after target edits

## Problem
Peek Definition (T1149) shipped with known limits after 3 review rounds (review c6w9bc, full text: `rgt review show T1149`). All are in crates/fe-workbench/src/peek.rs:
- F12/F15: baselines and excerpts are read as raw disk bytes (`Buffer::from_reader`), but LSP Location ranges refer to decoded, newline-normalized text, so CRLF, BOM and non-UTF-8 targets highlight and open at shifted positions.
- F8: selecting a row in the definitions list before that target's baseline has loaded shows/opens the previous excerpt or a stale range.
- F9/F16: resizing the window while the reserved row count stays the same does not re-reveal the widget, and refit scrolls to the excerpt line cached when it was opened.
- F13/F14: when the target buffer is edited while the peek is open, the range mapper loses columns and multi-line spans, and a TextDiff that bails out (`too_large`) is treated as "no change".

## Change
- Load baselines and excerpts through the editor's normal encoding-detecting, newline-normalizing loader.
- List selection waits for (or re-resolves) a valid baseline before showing or opening a target.
- Track viewport geometry independently of the reservation size; resolve the current definition anchor when placing the scroll on refit.
- Map both range endpoints through edits (columns and multi-line spans kept); when the diff is skipped as too large, or correspondence is lost, re-resolve the definition instead of keeping the stale range.

## Implementation notes
- F12/F15: excerpts and baselines already went through `loader::read` (`Buffer::from_file`, which detects the encoding and normalizes line endings), the same text the LSP's `load` maps unopened files' ranges into. `encoded_targets` pins that down for CRLF, BOM, UTF-16 and windows-1252 targets, closed and open in a tab, with edits above and on the definition's row.
- F13: `map_through` maps both ends, each by a byte of the definition: the start by its first, the end by its last, so text inserted at the start goes before it and at the end after it, as `target_of` maps, whole rows included. `Mapper` follows a byte through the rows both texts start and end with alike, then the row diff of the rows between (same column on a row it kept), then, in rows it changed, a diff of their tokens (words, white space, other chars); past 64 KiB a side, first the bytes they start or end with alike, backed off to whole words so a shared `hel` of `hello` and `helper` takes no end, then that diff of the tokens between. In a single word that diff changed (`helper` to `xhelper`), the bytes it starts or ends with alike. A byte is followed only where it lands on the same byte. The result is a byte range, so multi-line definitions keep their span.
- F14: where an end isn't followed (a byte deleted or moved, or the rows between over `MAX_DIFF_BYTES`), or the ends come out out of order, `Mapper::find` looks for the definition's rows, whole, in the buffer: just once, widened by up to 8 rows around them until that's so, and only in rows the row diff says are new (a copy the diff kept is other text), or, with no diff, with a row around them. Its name in comments, calls or other identifiers never counts. Not found, it's its start if that was followed, else the row it'd be near, unhighlighted. A target whose baseline isn't read (budget, size, unreadable) shows from the file as on disk, where its location is, instead of the open buffer at an unmapped range; Enter maps that text into the buffer as above.
- F8: a target on disk whose file is open in an editor shows nothing, and opens nothing, until its file's baseline is read (`PeekView::place`); `baselines_read` then shows it.
- F9/F16: the layer re-reveals the peek whenever the editor's bounds change size, whatever its row count. The excerpt keeps an anchor at the definition's start; refit scrolls to its row and the header shows its line:col where it is now.
- `peek_robustness_tests` is declared from `peek_tests.rs` (one `#[path] mod` line; no T1149 test changed), so the regression run, which overlays only test files onto the base, builds it against the old `peek.rs`: all five fail there.
