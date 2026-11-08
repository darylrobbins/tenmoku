# TODO comment badge styling (T1497)

## Decision

The `editor.todo_comments` badge (T1413) keeps its tinted background and now
also colours the keyword's text. Each badge is two decorations over the tag:
a `Highlight` in the kind's tint and a `Foreground` naming a highlight
(`comment.todo`, `comment.fixme`, `comment.hack`, `comment.warn`,
`comment.note`, `comment.perf`; TEST shares `comment.note`). The names are in
the highlight vocabulary, and every bundled theme colours them.

A theme that doesn't style a name itself gives it its parent's (`comment`)
colour. For a tinted `Foreground` decoration that means no colour of its own,
so the painter falls back to the tint's colour (`foreground_spans`).

`PERF` has its own `Tint::Perf` (the theme's `change_modified` colour), since
`Tint::Accent` is the cursor colour.

## Bold: out of scope

The badge is not bold. fe-gpui styled spans carry only a colour
(`StyledSpan<Hsla>`), and a shaped line uses one font, so a span cannot change
weight. Bold needs the span style to carry a weight (or a font), the line
shaper to shape runs in several fonts, and the layout, hit testing and
minimap, which measure with one font per line, to cope with widths that vary
within a line. That is a change to the text pipeline, not to this badge.
