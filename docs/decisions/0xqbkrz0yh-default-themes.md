# Yohen and Hakuji as Tenmoku's default themes (T1514)

With no `theme.dark` or `theme.light`, Tenmoku now uses Yohen and Hakuji for
both the UI tokens and the editor palette. `EditorPalette::fastedit_dark()` and
`fastedit_light()` are now read from `yohen.toml` and `hakuji.toml`, so any key
a theme leaves out comes from Yohen or Hakuji. `[theme] name = "dark"` and
`"light"` also mean Yohen and Hakuji. Tenmoku Dark, Light and Ember are still
bundled and selectable. Kiln Dark and Light remain kiln's own defaults for other
kiln apps.

## Rulings (lead, core)

- **Terminal ANSI (rev 2, option a).** A theme without a `[terminal]` table
  inherits the default palette's ANSI colours like any other key. Tenmoku Ember
  and the eight non-Tenmoku themes without the table now use Yohen's or
  Hakuji's. Tenmoku Dark and Light keep the two sets the terminal drew before
  themes existed.
- **Opaque diff rows (rev 2).** Yohen's and Hakuji's `diff_*_row` colours are
  opaque soft grounds. VS Code's cap of a fifth (`#..33`) now applies only to
  see-through rows and to the words; opaque rows are checked only by contrast.
- **Changed-word tints (rev 3, option a).** The rule in
  `fe-gpui diff_view_words_tests::contrast::a_changed_word_keeps_its_text_readable`
  stays as it is: a comment on a changed word keeps at least 65% of its contrast
  on the bare editor. The design's signal at `33` broke that rule for three of
  the four keys. Each key's alpha is lowered to the highest value at which the
  rule holds; the hue is unchanged and the alpha is never raised above `33`.
  `docs/design/design-system.md` pins no word tint, so it doesn't change.

## Word tints

Each ratio is a comment's contrast on the word over its row, as a share of its
contrast on the bare editor. "At +1" is the share one alpha step higher, which
fails the rule; Hakuji added already passed at `33` and keeps it. Comments are
Yohen `#88a18b` on `#1c1613` (6.41:1 on the bare editor) and Hakuji `#69727c`
(the theme's comment) on `#ffffff` (4.89:1).

| Theme, key | Was | Now | Comment on the word | Share | At +1 | Text on the word |
| --- | --- | --- | --- | --- | --- | --- |
| Yohen `diff_added_word` | `#6fd49a33` | `#6fd49a1f` | 4.19:1 | 65.4% | 64.8% | 9.29:1 |
| Yohen `diff_removed_word` | `#f2716f33` | `#f2716f2e` | 4.17:1 | 65.1% | 64.6% | 9.24:1 |
| Hakuji `diff_added_word` | `#17773f33` | `#17773f33` | 3.25:1 | 66.5% | 66.1% | 10.50:1 |
| Hakuji `diff_removed_word` | `#a1100e33` | `#a1100e26` | 3.18:1 | 65.2% | 64.7% | 10.30:1 |

At `33` the shares were 54.2% (Yohen added), 62.8% (Yohen removed) and 59.3%
(Hakuji removed).

The word still reads stronger than its opaque row: over the row, its tint moves
some channel by 20.8 (Yohen added), 34.1, 40.8 and 32.0 of 255.
`diff_view_words_tests::theme_colours` requires at least 8.

## A refused slot (review F2)

For a slot set to a theme that isn't installed, or to one of the wrong
appearance, kiln falls back to its own base (Kiln Dark or Light) for the UI,
under the default's editor palette (Yohen's or Hakuji's). That mix is no listed
theme, so the theme picker marks no row current for it. An unset slot (or a
`theme.name` that nothing bundled has) is a real Yohen or Hakuji, and the picker
marks it. Before this change the fallback was exactly Tenmoku Dark or Light,
because those files had no UI tables. Making the fallback the whole default
theme would mean the host replacing kiln's choice. It would then have to keep
kiln's "isn't installed" diagnostic itself, and re-select when extensions load
later, so that's left to a card on kiln's fallback base.
