---
task_uid: "01m4gc1ppmtpkhvgbps0mctzh2"
task: "T277"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "In tests::search_panel_followups::invalid_utf8_before_a_match_shifts_its_highlight_and_selection, a.txt and b.txt keep their malformed bytes and matches past more than 64 KiB of valid UTF-8, and the test asserts Buffer::from_reader_detecting labels each 'UTF-8'"
    required: true
    manual: false
  - id: "C2"
    text: "The test again asserts the editor's decoded text: after each click the text holds U+FFFD where the invalid bytes were and the selection is 'needle'; the 'valid multi-byte UTF-8 after the bad byte' workaround is gone"
    required: true
    manual: false
  - id: "C3"
    text: "src/search_panel.rs is unchanged; the other tests in search_panel_followups.rs pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "The implementation notes in docs/specs/hj6ffj8dhc-open-files-in-their-detected-encoding-in.md say the fixtures keep malformed bytes past the detection window, and the follow-up bullet names the search-panel encoding card"
    required: true
    manual: false
---

# Make the search panel's invalid-UTF-8 test use fixtures that detection takes for UTF-8

# Make the search panel's invalid-UTF-8 test use fixtures that detection takes for UTF-8

## Problem

Review of T250, finding F2 (medium), at `crates/fe-workbench/src/tests/search_panel_followups.rs:226`.
The test `tests::search_panel_followups::invalid_utf8_before_a_match_shifts_its_highlight_and_selection`
is meant to prove the search panel's lossy-UTF-8 offset mapping against the editor. Its two fixtures have
invalid bytes inside the first 64 KiB:

- `a.txt` is `ab\xFFneedle`;
- `b.txt` starts with `\xF0\x9F`.

`Text::from_reader_detecting` (`crates/fe-text/src/encoding.rs`) calls `detector.guess(None, false)`
there, which never answers UTF-8, so the editor opens both in a legacy encoding. The valid UTF-8
appended after the bad byte doesn't change that.

T250 then dropped the assertion on the decoded text, so the test passes without exercising the lossy
UTF-8 path it claims to test. The T250 implementation note
(`docs/specs/hj6ffj8dhc-open-files-in-their-detected-encoding-in.md:61`) describes the workaround as
working.

The other half of T277's original F1 is split out:

- fe-nav extraction and references are T287 and T288.
- The search panel's mapping for BOM-prefixed and legacy-encoded files is a separate bugfix card that
  depends on this one.

## Change

Test and docs only. `crates/fe-workbench/src/search_panel.rs` doesn't change.

- **`crates/fe-workbench/src/tests/search_panel_followups.rs`**, in
  `invalid_utf8_before_a_match_shifts_its_highlight_and_selection`:
  - Rebuild both fixtures so detection deterministically picks UTF-8. Each file starts with more than
    64 KiB of valid UTF-8 lines (plain ASCII will do), and the malformed bytes and the matches come
    after that.
  - Keep what each file exercises: `a.txt` has an invalid byte just before `needle`; `b.txt` has a
    2-byte invalid sequence before a match beyond the preview's reach, so the preview starts mid-line.
  - Drop the "valid multi-byte UTF-8 after the bad byte" workaround.
  - Assert that `fe_buffer::Buffer::from_reader_detecting` of each fixture reports UTF-8 (`label()` is
    `"UTF-8"`).
  - Restore the decoded-text assertion: after each click, the editor's text has U+FFFD where the
    invalid bytes were, and the selection is `needle`.
  - The rows' expected `line_number`s follow from the new fixtures.
- **`docs/specs/hj6ffj8dhc-open-files-in-their-detected-encoding-in.md`**, "Implementation notes":
  - Correct the first bullet: the fixtures now keep their malformed bytes past the 64 KiB detection
    window.
  - Point the "Follow-up" bullet at the search-panel encoding card.
- Leave the other tests in the file unchanged.
