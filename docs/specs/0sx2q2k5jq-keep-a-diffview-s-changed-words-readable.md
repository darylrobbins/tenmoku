---
task_uid: "01m69aessrv7w5wk0sx2q2k5jq"
task: "T382"
rev: 5
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Diagnosed and stated in the hand-in: whether a DiffView's changed-word highlight (Tint::DiffAdded/DiffRemoved, DecorationKind::Highlight, decorations.rs diff_color) is painted after the row's glyphs, and how the word colour composes over the row band. If it paints over the text, it now paints in the background layer with the row bands, before any glyph."
    required: true
    manual: false
  - id: "C2"
    text: "A changed word's text keeps its own syntax colour at full strength on every bundled theme (fastedit-dark/ember/light, catppuccin x4, vscode-2026-dark/light, vscode-dark-modern): each theme sets diff_added_row/diff_removed_row/diff_added_word/diff_removed_word to translucent values no stronger than VS Code's defaults (dark: insertedLineBackground #9bb95533, insertedTextBackground #9ccc2c33, removedLineBackground #ff000033, removedTextBackground #ff000033; light the same), and the word tint over the row band stays visibly distinct from the band."
    required: true
    manual: false
  - id: "C3"
    text: "Regression test (in the existing diff_view_words_tests.rs as mod contrast, since a new test file declared from diff_view.rs has no tests at base and fails the regression check as infra): the painter already draws a changed word's highlight in the background layer before any glyph (C1), so the cause is colour only. The test composites the word tint over the row band over the editor background for fastedit-dark and fastedit-light and asserts the default foreground keeps a WCAG contrast of at least 4.5:1 and the comment keeps at least 65% of its bare-background contrast (a dark comment has only 2.9:1 on the bare background, so an absolute 4.5:1 is unreachable without recolouring comments); it fails at base."
    required: true
    manual: false
  - id: "C4"
    text: "Existing diff_view_words_tests and the screenshot examples pass; cargo test -p fe-gpui and clippy clean."
    required: true
    manual: false
---

# Keep a DiffView's changed words readable: their tint sits behind the glyphs at VS Code's strength, not over or through the text (Mac T1252)

## Problem
The user, looking at a DiffView of commit_rows.rs in the default theme: "This delta is basically unreadable." In modified rows the changed words get a strong, near-solid red/green block and their text (comments especially) is dimmed to barely visible. T1218 (helium T378, landed in stack 66, 69157f2c) added diff_*_row (#..33) and diff_*_word (#..59) theme colours and paints the word highlight via decorations.rs diff_color; either the highlight is drawn over the glyphs, or the 35% word over a 20% row is too strong, or both.
## Change
Find which; paint the word tint behind text with the row bands; bring every bundled theme's diff colours to VS Code's translucent levels (word distinct from row but never hiding text). Text colour stays the syntax colour. Do not change the row hatch (diff_filler) or the gutter.

## Implementation notes

- C1: the word highlight was already painted in `Layer::Backgrounds` (element.rs, before the text pass), right after the `RowBackground` bands via `diff_color`; the cause was colour only. The old words (#..59 over #..33 rows) lightened the dark background to near the comment colour.
- C2: all ten bundled themes now set the four keys explicitly (row #..14 / word #..1f, i.e. 8% / 12%, weaker than VS Code's #..33). Even VS Code's full #9ccc2c33 took a dark-theme comment to 1.46:1, so ours are weaker still.
- C3: the test is colour-based. A dark comment has only 2.9:1 on the bare background, so 4.5:1 is unreachable there. It asserts 4.5:1 for the default foreground and that the comment keeps at least 65% of its bare-background contrast. It fails at base.
- The contrast test lives in `diff_view_words_tests.rs` (`mod contrast`), not a new `diff_view_word_contrast_tests.rs`: a new test file declared from a non-test file has 0 tests at base, which fails the regression check as infra.
