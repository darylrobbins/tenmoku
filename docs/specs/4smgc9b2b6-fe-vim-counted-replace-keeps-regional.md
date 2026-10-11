---
task_uid: "01m4nys3g546gzbq4smgc9b2b6"
task: "T44"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim integration tests in tests/replace_regional.rs: 2R🇦\u003cCR\u003e\u003cBS\u003e🇧\u003cBS\u003e🇨\u003cEsc\u003e on \"abcdefghijk\" gives \"🇨🇦🇨efghijk\" and 2R🇦🇧🇨\u003cCR\u003e\u003cBS\u003e\u003cBS\u003e🇦\u003cEsc\u003e on \"abcdefghijklmno\" gives \"🇦🇧🇦🇦🇦ghijklmno\" (Neovim 0.12.4), each also as a counted dot-repeat; a count of 4294967295 on such a session returns within the test; the two regression tests fail at the base (T226 as landed) and pass with the fix"
    required: true
    manual: false
  - id: "C2"
    text: "Every earlier fe-vim corpus and unit test passes with its files unchanged (replace.toml may only gain cases, with replace.expected.json the recorder's unedited output); src/replace.rs is the only src file that changes and src/tests/mod.rs is untouched; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe"
    required: true
    manual: false
---

# fe-vim: counted replace keeps regional-indicator pairing across copies (T226 review F13) (Mac T359)

# fe-vim: counted replace keeps regional-indicator pairing across copies (T226 review F13)

Follow-up from T226's round-4 review (finding F13, medium). The user chose to land T226 with this case
narrowed out of its C1 and fixed here.

## The defect

`crates/fe-vim/src/replace.rs` simulates the `count - 1` replayed copies of a replace session
(`copies`, ~line 1060, and `again`, called from `repeat_typed` ~line 733; both simulator branches).
Neither branch keeps the non-whitespace row text that precedes the current copy, so regional-indicator
(flag emoji) pairing, which depends on how many indicators precede a character, segments differently
from Neovim 0.12.4:

- `2R🇦<CR><BS>🇧<BS>🇨<Esc>` on `"abcdefghijk"` gives `"🇨🇨efghijk"`; Neovim gives `"🇨🇦🇨efghijk"`.
- Empty-tail branch: `2R🇦🇧🇨<CR><BS><BS>🇦<Esc>` on `"abcdefghijklmno"` gives `"🇦🇧🇦🇦🇧🇦ghijklmno"`;
  Neovim gives `"🇦🇧🇦🇦🇦ghijklmno"`.
- Counted dot-repeat (`.` with a count after such a session) shares both defects.

## The fix (`crates/fe-vim/src/replace.rs` only)

Keep enough preceding row context for grapheme segmentation across copies, while `<BS>` still only
reverses characters the session owns. Include that context in the fast-forward equivalence check, so
the bounded-replay guarantee from T226's F10 fix (a count of 4294967295 returns within the test) still
holds. `copies` already receives the row text before the caret (`before`); counted dot-repeat goes
through `replace::replace` and `replace::retype`, so no other source file needs to change. If the fix
turns out to need one (`insert.rs`, `repeat.rs`, `state.rs`, or any of the reducer/registry files),
block and ask the lead rather than widen the scope.

## Tests (new integration test file)

`crates/fe-vim/tests/replace_regional.rs`, through fe-vim's public API alone (`HeadlessVim::new`,
`feed`, `text`, `cursor`), as `tests/host.rs` drives it. No `mod` line anywhere: cargo finds the file,
and it is not in `src/tests/mod.rs`, so this task shares no file with T227, T228, T229 or the
feature-module refactor.

- `counted_replace_pairs_regional_indicators_after_a_line_break`: the first example above, then the
  same session as a counted dot-repeat (`.` with a count on a fresh row).
- `counted_replace_pairs_regional_indicators_with_an_empty_tail`: the second example, then its counted
  dot-repeat.
- `counted_replace_of_regional_indicators_stays_bounded`: a count of 4294967295 on each session (and a
  counted dot-repeat) returning within the test (`Instant`, a few seconds at most).

Expected results are Neovim 0.12.4's; add the cases to `tests/corpus/replace.toml` too if the recorder
can express them.

### Regression overlay

The first two tests are the task's regression tests: they fail at the base (T226 as landed) and pass
with the fix. At the base, `rgt finish` overlays the candidate's test files (`**/tests/**/*.rs`), so
`tests/replace_regional.rs` is compiled against the base's public API: use only items that exist at
T226's landing (no new public items), and keep the bounded test off the regression list, since it may
pass at the base. Run both at the base before finishing (`git checkout <base> --
crates/fe-vim/src/replace.rs`, run, restore; commit first).

## Out of scope

Any change outside `crates/fe-vim`; other replace-mode behaviour; the GPUI wiring.

## Implementation notes

- `Typing` carries `context`: the last grapheme (at most its last 16 characters) of the row before the copy, when anything but blanks is there. `Typing::row` (shared by `grapheme_back` and the end of `type_keys`) puts it before the copy's characters, so segmentation sees what precedes them; `<BS>` still pops only `Typed::Char` entries, the copy's own. The first copy's context comes from `before`; each copy leaves the next one's.
- The context is part of `Shape`, so the fast-forward check includes it. A copy that leaves an odd number of indicators alternates the context between a lone indicator and a pair, so `steady` also looks for copies repeating in pairs (their combined cut and piece go to `again`). Without that, `4294967295R🇦<CR><BS><Esc>` took ~32s in a debug build; with it, ~3s, the same as the 64 MB write any such count does.
- Eight corpus cases were added to `replace.toml` and recorded with nvim 0.12.4. Six fail at the base; `count_regional_alternating_pairs` and `count_regional_after_lone_indicator` pass there and guard the period-2 path and the context taken from the original row.
- The bounded test times each session (and its counted dot-repeat) against a plain `4294967295R🇦🇧<Esc>` run just before it, which writes the same 64 MB. A case must finish within 4× that baseline plus 1s, and never more than 60s. A fixed 10s bound failed under the gate's parallel load (11.6s); the relative bound still catches replaying copies one at a time (43s against a 2.5s baseline).
