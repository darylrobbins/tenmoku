# Symbol icons in their syntax colours (T1650)

Spec: `docs/specs/q2jz8kj3fn-symbol-icons-in-their-syntax-colours.md`.

## What changed

- **Glyphs** (`outline_panel.rs` `symbol_icon`): Trait is `puzzle`
  (Interface keeps `plug`), Impl is `blocks`, Method is Tabler
  `math-function` (Function keeps `square-function`), Macro is `wand`,
  Field is `key-round`, Variant is `circle-dot`. No glyph was swapped for
  another bundled one. `Link`, `Asterisk` and `Diamond` stay bundled.
- **Tint** (`outline_panel.rs` `symbol_tint`): the colour of the longest
  listed prefix of the kind's `symbol_highlight` in the active
  `EditorPalette`'s `[syntax]` table (`fe_gpui::editor_palette`), the
  `fe_syntax::resolve_theme` rule, so `function.method` falls back to
  `function`. One prefix scan per drawn row. `None` (Yohen has no
  `variable`) leaves the icon in `list.icon` / `ui.icon`.
- **Surfaces:** the outline's rows and the call hierarchy's set
  `TreeRow::icon_color`; the symbol picker's `render_match` tints its
  `svg()` (selected or not); symbol crumbs set `Crumb::icon_color`.
- **kiln-ui:** `TreeRow::icon_color(Hsla)` and its getter `icon_tint()`;
  an error row's icon is still the error colour, and an ignored row's
  icon still keeps the list's colour (as today, "dim, not disabled").
  Selection changes only the background and edge. `Crumb::icon_color(Hsla)`
  (and the public `icon_color` field beside `color`). A coloured or
  `Tinted` icon ignores both, as it ignores the tint today. Defaults are
  unchanged.
- **Test hook:** `Pane::crumb_icon_colors()` beside `crumb_colors()`;
  `drawn_crumbs()` keeps its type.

## Differences from the spec

- **Table's highlight stays `property`.** T1542 had already added
  `symbol_highlight` (Table and Field → `property`); the spec's table says
  Table → `type`. As the spec says, T1542's choice is kept, and the
  crumbs' label colours and these icons share the one mapping.
- **Crumb labels** keep T1542's lookup (`EditorView::theme().syntax_color`
  through `fe_syntax::Highlight`), which resolves the same palette by the
  same rule; only the icons use `symbol_tint`.

## The breadcrumb test's method (respec)

Rust's outline reports a function in an `impl` as `OutlineKind::Method`, so
`fn b` inside `impl S` now draws `math-function`. The first run failed
`tests::breadcrumb_symbol_icons::symbol_crumbs_carry_their_kinds_icon`,
which expected `symbol_icon(OutlineKind::Function)` for `b`, and the task
was blocked. The respec put that test in scope, and C5 now lets it expect
`OutlineKind::Method` for `b`.

The test states `b`'s icon twice: once for its crumb (`drawn_crumbs()`),
and once, after clicking that crumb, for `b`'s row in the quick outline
(`symbol-picker-icon:b:<path>`). Both say the same thing, that `b` is a
Method, so both lines changed from `Function` to `Method`. Only
`OutlineKind::Function` → `OutlineKind::Method` changed in those two lines;
the rest of the test is untouched. With only the crumb line changed, the
picker assertion failed, since the picker draws `b`'s Method glyph too.

The crumb's colour is unaffected: `function.method` falls back to
`function`, so `symbol_crumb_icons_take_their_kinds_tint` sees
module/type/function as C3 asks.
