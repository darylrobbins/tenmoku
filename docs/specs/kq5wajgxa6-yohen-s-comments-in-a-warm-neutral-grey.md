---
task_uid: "01m6xwpvtf6a0de1kq5wajgxa6"
task: "T1613"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests comment_is_a_warm_neutral_grey and comment_clears_4_5_on_every_surface (cargo nextest run -p fe-config --test yohen_comment_grey) pass: Yohen's syntax.comment is a warm neutral grey (OKLab chroma \u003c= 0.010; hue 40-100 degrees when chroma \u003e= 0.002), \u003e= 4.5:1 on chrome, tool window, editor, cursor line, hover, selected, focused selection and overlay and \u003e= 6:1 on editor and cursor line; Hakuji's comment is still #69727c"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-config passes, including yohen_hakuji_values (every_design_role_has_the_design_value against docs/design/design-system.md with the Comment row's new Yohen hex) and yohen_hakuji_contrast (retuned_values_move_in_lightness_only, with the T1512 (rev 2) paragraph unchanged)"
    required: true
    manual: false
  - id: "C3"
    text: "Test diff_view::diff_view_words_tests::contrast::a_changed_word_keeps_its_text_readable (cargo nextest run -p fe-gpui) passes with Yohen's diff_added_word and diff_removed_word unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-gpui --example screenshots, cargo run -p fe-workbench --example screenshots, cargo run -p fe-chat-view --example screenshots and cargo run -p fe-markdown-view --example screenshots match every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (dark goldens that draw a Yohen comment; no light golden moves)"
    required: true
    manual: false
---

# Yohen's comments in a warm neutral grey: #88a18b becomes #9e9b97, still 6:1 on the cursor line and 4.5:1 on every surface

# Yohen's comments in a warm neutral grey

User request: "Maybe we should go for a more subtle grey for comments in our
themes?" Chosen: a neutral grey, keeping the contrast rules.

Spec: `docs/design/design-system.md#colour` (Syntax table, Comment row; every
text colour clears 4.5:1 on every surface it sits on, code text 6:1 in the dark
theme) and `#accessibility-rules`.

Yohen's comment `#88a18b` is a green-tinted wood ash (OKLab L 0.683, C 0.042,
h 148). It becomes **`#9e9b97`**, a warm neutral grey (OKLab L 0.691, C 0.007,
h 75: a trace of Yohen's iron-brown, no green). Its WCAG 2 contrast on Yohen's
surfaces (lead's computation; the tests are the authority):

| Surface | Hex | `#88a18b` (was) | `#9e9b97` |
| --- | --- | --- | --- |
| Chrome | `#120e0b` | 6.88 | 6.94 |
| Tool window | `#17120f` | 6.65 | 6.72 |
| Editor | `#1c1613` | 6.41 | 6.47 |
| Cursor line | `#231c18` | 6.01 | 6.07 |
| Hover | `#2b221d` | 5.57 | 5.62 |
| Selected | `#2f2621` | 5.29 | 5.34 |
| Focused selection | `#3e2717` | 4.99 | 5.03 |
| Overlay | `#2a211c` | 5.64 | 5.69 |

The 6:1 floor on the cursor line pins the lightness, so "subtler" here means
less chroma, not darker. Hakuji's comment `#69727c` is already a grey and passes
every rule: unchanged.

## Change

- `crates/fe-config/themes/yohen.toml` `[syntax] "comment"` → `#9e9b97`. No other
  token derives from it (the `comment.*` TODO badges are signal colours, see
  `docs/decisions/246q270zwp-yohen-hakuji-todo-badges.md`; `comment.documentation.code`
  is ink). Update any nearby comment in the file that names the colour.
- `docs/design/design-system.md`: the Syntax table's Comment row, Yohen column
  `#9e9b97` (Glaze stays "Wood ash", Style Italic). **Leave the "T1512 (rev 2)"
  paragraph as it is**: `retuned_values_move_in_lightness_only` parses exactly
  its five `old` → `new` pairs and holds them to lightness-only moves; this
  change moves chroma. Add one short paragraph after it, separated by a blank
  line and not starting with "T1512", saying this card moved Yohen's comment
  from `#88a18b` to the neutral grey `#9e9b97` (chroma dropped, lightness kept at
  the 6:1 floor on the cursor line). Write it with "from … to …", not an arrow.
- The changed-word tints (`diff_added_word`, `diff_removed_word`) were sized so
  a comment keeps 65% of its bare-editor contrast on a changed word
  (`fe-gpui diff_view_words_tests::contrast::a_changed_word_keeps_its_text_readable`,
  `docs/decisions/0xqbkrz0yh-default-themes.md`). For a comment lighter than
  both grounds that share depends only on the grounds (lead's check: 65.2% for
  both, before and after), so the tints don't move; the test confirms it. The
  decision file's word-tint table records T1514's numbers and stays as it is.
- Regenerate on the Mac every golden that draws a Yohen comment and changed
  (`UPDATE_SCREENSHOTS=1`, all four example sets in scope: fe-gpui, fe-workbench,
  fe-chat-view, fe-markdown-view), and look at each. Light goldens draw Hakuji
  and must not move.

## Tests (new file)

`crates/fe-config/tests/yohen_comment_grey.rs`, WCAG 2 relative luminance and
OKLab as `yohen_hakuji_contrast.rs` computes them (copy the helpers; that file
is not in scope):

- `comment_is_a_warm_neutral_grey`: Yohen's `syntax.comment` has OKLab chroma
  ≤ 0.010, and if chroma ≥ 0.002 its hue is between 40° and 100° (warm, never
  green or blue); Hakuji's comment is still `#69727c`.
- `comment_clears_4_5_on_every_surface`: Yohen's comment ≥ 4.5:1 on chrome,
  tool window, editor, cursor line, hover, selected, focused selection and
  overlay, and ≥ 6:1 on editor and cursor line.

Platform: **Mac-only** (goldens).
