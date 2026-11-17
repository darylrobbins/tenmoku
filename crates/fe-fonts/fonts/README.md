# Bundled fonts

From [google/fonts](https://github.com/google/fonts) at commit
`86aced91de05cd4f96a06838124cca45be337b2b`, all under the SIL Open Font
License 1.1. Each family's `OFL.txt` is copied unmodified beside its files.

| Directory | Upstream (`ofl/…` at that commit) | Files here |
|---|---|---|
| `instrument-sans/` | `instrumentsans/InstrumentSans[wdth,wght].ttf` (SHA-256 `b24f1812584816958afcf22e22d08e44318c5e51651e25d2438efdde389b33b1`) | `InstrumentSans-Regular.ttf`, `-Medium.ttf`, `-SemiBold.ttf`: static instances |
| `spline-sans-mono/` | `splinesansmono/SplineSansMono[wght].ttf` (`e20c1df32aa2f886e828cfcc81ca5c405d3f3b160990f6b892923e2e449bf525`), `SplineSansMono-Italic[wght].ttf` (`073f111f37a86cc6d952b7870b3c1aebbfc78f4056dd3996db28bbc7166667a6`) | `SplineSansMono-Regular.ttf`, `-Medium.ttf`, `-SemiBold.ttf`, `-Italic.ttf`, `-SemiBoldItalic.ttf`: static instances |
| `instrument-serif/` | `instrumentserif/InstrumentSerif-Regular.ttf` (`498efd461f6ddfcb7a111bf9a565709d2085d48201d501ead960d93e84ffbb88`) | `InstrumentSerif-Regular.ttf`, unmodified |

## Why static instances

Upstream ships Instrument Sans and Spline Sans Mono as variable fonts. GPUI's
macOS text system registers an in-memory file as one face, at the font's
default instance, so asking for weight 500 or 600 of the variable file gets
the 400 face: with the variable Instrument Sans registered, 400, 500 and 600
set the same string at the same width (checked with
`crates/fe-workbench/tests/bundled_fonts.rs`). So each weight we use is its
own static file. The OFL allows modified versions under the same licence;
none of these families declares a Reserved Font Name.

Made with fontTools 4.60.1's instancer, from the upstream directory
(`ofl/`) into this one, `--update-name-table` renaming each instance from
the font's STAT table:

```sh
FT="uvx --from fonttools==4.60.1 fonttools varLib.instancer --update-name-table"
$FT -o instrument-sans/InstrumentSans-Regular.ttf   'instrumentsans/InstrumentSans[wdth,wght].ttf' wght=400 wdth=100
$FT -o instrument-sans/InstrumentSans-Medium.ttf    'instrumentsans/InstrumentSans[wdth,wght].ttf' wght=500 wdth=100
$FT -o instrument-sans/InstrumentSans-SemiBold.ttf  'instrumentsans/InstrumentSans[wdth,wght].ttf' wght=600 wdth=100
$FT -o spline-sans-mono/SplineSansMono-Regular.ttf  'splinesansmono/SplineSansMono[wght].ttf' wght=400
$FT -o spline-sans-mono/SplineSansMono-Medium.ttf   'splinesansmono/SplineSansMono[wght].ttf' wght=500
$FT -o spline-sans-mono/SplineSansMono-SemiBold.ttf 'splinesansmono/SplineSansMono[wght].ttf' wght=600
$FT -o spline-sans-mono/SplineSansMono-Italic.ttf   'splinesansmono/SplineSansMono-Italic[wght].ttf' wght=400
$FT -o spline-sans-mono/SplineSansMono-SemiBoldItalic.ttf 'splinesansmono/SplineSansMono-Italic[wght].ttf' wght=600
```

### Spline Sans Mono Italic's one-cell advances

The Italic instance at 400 comes out with `a` and its ten accented forms
(`aacute` … `atilde`, `aogonek`, `aringacute`) 1196 units wide, where every
other one-cell glyph of every face is 1200: an italic `a` would pull the rest
of its row 0.03 px left at 13 px, a little more with each, out of line with
the plain rows (T1600, `crates/fe-gpui/tests/styled_spans.rs`,
`every_face_has_the_same_advance`). Those advances are set to 1200 after
instancing, their outlines left where they are; the two-cell ligature glyphs
(2400) and zero-width marks keep theirs:

```sh
uvx --from fonttools==4.60.1 python -I - spline-sans-mono/SplineSansMono-Italic.ttf 1200 <<'PY'
import sys
from fontTools.ttLib import TTFont
path, cell = sys.argv[1], int(sys.argv[2])
font = TTFont(path, recalcTimestamp=False)
metrics = font["hmtx"].metrics
for name, (advance, lsb) in metrics.items():
    if advance != cell and abs(advance - cell) <= cell // 100:
        metrics[name] = (cell, lsb)
font.save(path)
PY
```

A new file goes in `crates/fe-fonts/src/lib.rs`'s `FACES`, and in
THIRD_PARTY.md's Fonts table; `crates/fe-fonts/tests/fonts.rs` checks that
every file here is listed under the family and weight its name table gives.
