# Yohen and Hakuji: TODO comment badge colours (T1512)

T1497 added six highlight names for the `editor.todo_comments` badge
(`comment.fixme`, `.hack`, `.warn`, `.note`, `.todo`, `.perf`) and coloured them
in every theme bundled then. Yohen and Hakuji, written before it landed,
inherited Tenmoku Dark's and Light's colours for them, which broke their code
contrast rule (6:1 in Yohen, 4.5:1 in Hakuji, on the editor and the cursor
line) in the re-integration gate.

## Ruling (lead, core)

Use only hexes from `docs/design/design-system.md`'s Signals or Syntax tables,
by the kind's meaning: FIXME shinsha, WARN and HACK kiseto, NOTE ruri, TODO and
PERF as T1497 maps them (TODO the information colour, which these themes take
from ruri; PERF the keyword's). Where the meaning's colour misses the contrast
rule, fall back to the nearest design value (OKLab ΔE) that clears it.

## Mapping

| Badge | Meaning | Yohen | Hakuji |
| --- | --- | --- | --- |
| FIXME | shinsha | `#f0a3c2` peach bloom (fallback) | `#a1100e` shinsha |
| HACK | kiseto | `#e8b440` kiseto | `#776a00` kiseto |
| WARN | kiseto | `#e8b440` kiseto | `#776a00` kiseto |
| NOTE (and TEST) | ruri | `#8db1f2` lapis (fallback) | `#1f68cf` ruri |
| TODO | ruri (information) | `#8db1f2` lapis (fallback) | `#1f68cf` ruri |
| PERF | keyword | `#c7a5e6` jun | `#7b36a8` jun |

Yohen's fallbacks, with the meaning colour's contrast on the cursor line
`#231c18` and the nearest passing design colours by OKLab ΔE (×100):

- Shinsha `#f2716f` is 5.89:1. Nearest: hare's fur `#e0874c` (7.9), peach
  bloom `#f0a3c2` (13.1). Hare's fur is skipped: the design's Never column
  forbids it as decoration or a second accent, and it means "you". Peach bloom
  is also what Yohen's `markup.deleted` uses, for the same reason.
- Ruri `#4f95f5` is 5.56:1. Nearest: lapis `#8db1f2` (10.7).

Every Hakuji meaning colour clears 4.5:1 on both surfaces (lowest: ruri at
4.97:1 on the cursor line). `hakuji_code_clears_4_5` and
`yohen_code_clears_6_on_editor_and_cursor_line` check every capture, these
included.
