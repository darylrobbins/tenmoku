---
task_uid: "01m4tj95cf06891k3svjaag6zz"
task: "T652"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-text test char_at_inside_a_char_is_none (tests/char_at_inside_char.rs, as given in the spec) passes: Text::char_at and TextSource::char_at answer None inside é, 世 and 😀 and the char at every boundary; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "TextSource::char_at's doc comment says None at or past the end and inside a char; cargo nextest run -p fe-text -p fe-fileview passes unchanged"
    required: true
    manual: false
---

# Answer None from Text::char_at inside a multi-byte char instead of panicking, as TextSource's contract and FileText do

## Problem

`fe_text::TextSource` documents "Methods are infallible" (`crates/fe-text/src/source.rs`, the trait doc),
and fe-fileview's `FileText::char_at` (`crates/fe-fileview/src/source.rs`) answers `None` for an offset
inside a valid char. `Text::char_at` (`crates/fe-text/src/lib.rs`) is
`(offset < self.len()).then(|| self.rope.char(offset))`, and ropey 2.0.0-beta.1's `Rope::char(byte_idx)`
panics when `byte_idx` is not a char boundary ("Panics … If `byte_idx` is not a char boundary"). So
`Text::new("é").char_at(1)`, directly or through `&dyn TextSource`, panics where `FileText` answers
`None`: the two implementations disagree, and a caller holding `dyn TextSource` (fe-display's
`long_row.rs`, fe-editor's `multi_cursor.rs`) can't rely on the trait's contract. Verified 2026-10-13
at e6988aba: the test below panics at `crates/fe-text/src/lib.rs:282`.

## Change

- `crates/fe-text/src/lib.rs` `Text::char_at`: answer `None` when `offset` is not a char boundary
  (`self.rope.is_char_boundary(offset)`), as at or past the end. Add a doc comment: the char starting at
  `offset`; `None` at or past the end and inside a char.
- `crates/fe-text/src/source.rs`: give the trait's `char_at` the same doc comment, so every
  implementation answers alike.

Nothing else changes; `is_char_boundary` and `clip_offset` already behave. A prototype of the one-line
fix passed the test below and `cargo nextest run -p fe-text -p fe-fileview` unchanged.

The test goes in the new `crates/fe-text/tests/char_at_inside_char.rs`. Use this file as written (it
panics at the base):

```rust
//! `Text::char_at` answers `None` for an offset inside a multi-byte char,
//! as it does past the end, instead of panicking: `TextSource`'s methods are
//! infallible, and fe-fileview's `FileText` already answers `None` there.

use fe_text::{Text, TextSource};

#[test]
fn char_at_inside_a_char_is_none() {
    let raw = "aé世😀\nz";
    let text = Text::new(raw);
    let source: &dyn TextSource = &text;
    for offset in 0..raw.len() + 2 {
        let want = match raw.is_char_boundary(offset) {
            true => raw.get(offset..).and_then(|rest| rest.chars().next()),
            false => None,
        };
        assert_eq!(text.char_at(offset), want, "Text::char_at({offset})");
        assert_eq!(
            source.char_at(offset),
            want,
            "TextSource::char_at({offset})"
        );
    }
}
```
