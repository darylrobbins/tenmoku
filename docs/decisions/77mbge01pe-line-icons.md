# Line icons at the design's 1.75 stroke (T1516)

Every Lucide SVG in `kiln/crates/kiln-ui/assets/icons/` now has
`stroke-width="1.75"` on its root `<svg>` in place of Lucide's `2`; nothing else
in the files changes. `tests::icon_stroke::every_bundled_icon_strokes_at_1_75`
holds every `Icon` to a 24 grid and to that one stroke width. Thirteen icons are
added from the same Lucide tag (1.52.0), each with an `Icon` variant: no
existing name changes, so which-key `icon=` names keep working.

## Choices

- **`text-select` is `square-dashed-text`.** At Lucide 1.52.0 there is no
  `icons/text-select.svg`: `text-select` (and the older `text-selection`) are
  deprecated aliases in `icons/square-dashed-text.json`. The icon ships under
  Lucide's current name, `square-dashed-text` (`Icon::SquareDashedText`), like
  every other bundled icon. Shipping it as `text-select` would name a file that
  isn't in Lucide's tree at the pinned commit.
- **Already bundled.** `circle-x`, `triangle-alert`, `pin`, `play` and `square`
  from the spec's list were in `Icon::ALL` already and are not added again.
- **Licences.** `chevron-up`, `square-minus` and `square-plus` are in Lucide's
  Feather list (the latter two by their old names); `bell` credits `colebemis`,
  so it is treated as MIT like the earlier such icons. The rest are ISC only.
  Both `THIRD_PARTY.md` files record this and the stroke change.

## Left for a follow-up (outside this card's scope)

These still describe the icons as unmodified, or count 83:

- `NOTICE` and `kiln/NOTICE`: "Lucide icons (unmodified, in …)". ISC asks only
  for the copyright and permission notice, which are unchanged, but the word
  should become "modified: stroke width 1.75".
- `docs/design-language.md`, Icons: "Lucide, unmodified, 83 SVGs".
