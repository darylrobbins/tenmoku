---
task_uid: "01m6z8akd5eayb01q2jz8kj3fn"
task: "T1650"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Glyphs (cargo nextest run -p fe-workbench): tests::symbol_icon_tints::every_kind_has_its_own_glyph passes. Every OutlineKind maps to an icon in Icon::ALL, pairwise distinct except that all Heading levels share one: Trait is puzzle and Interface stays plug; Method is Tabler math-function and Function stays square-function; Impl is blocks (not link); Macro is wand (not asterisk); Field is key-round; Variant is circle-dot. Other kinds keep their glyph (or take an already-bundled one noted in docs/decisions)."
    required: true
    manual: false
  - id: "C2"
    text: "Tints (cargo nextest run -p fe-workbench): tests::symbol_icon_tints::tints_follow_yohen_and_hakuji_syntax_colours and tests::symbol_icon_tints::a_kind_the_theme_leaves_uncoloured_has_no_tint pass. symbol_tint (outline_panel.rs) takes the colour of the longest listed prefix in the active EditorPalette's [syntax] table, the resolve_theme rule. Under both EditorPalette::fastedit_dark() and fastedit_light(), function/method take function, struct/class/trait/type take type, and constant takes constant. A kind the theme leaves uncoloured (Yohen variable) gives None and keeps list.icon. No kiln token group, IconRef variant or theme file is added."
    required: true
    manual: false
  - id: "C3"
    text: "Selected rows and every surface (cargo nextest run -p fe-workbench): tests::symbol_icon_tints::outline_rows_take_their_kinds_tint_selected_or_not passes. Under Hakuji, the outline rows' TreeRow::icon_tint() is type for Point and function for main, and stays so when the row is selected, focused or unfocused, as tinted file icons do. tests::symbol_icon_tints::symbol_crumb_icons_take_their_kinds_tint passes (crumbs module/type/function). symbol_picker.rs and hierarchy_panel.rs draw their icons through the same symbol_tint."
    required: true
    manual: false
  - id: "C4"
    text: "kiln-ui (cargo nextest run -p kiln-ui): tests::tree_row_icon_color::an_icon_colour_tints_the_icon_in_every_selection_state and tests::tree_row_icon_color::error_and_ignored_rows_still_win_over_an_icon_colour pass. tests::tree_row::colors_follow_state, tests::icon_stroke::every_bundled_icon_strokes_at_1_75 and tests::assets::bundled_files_match_the_enum pass. A TreeRow or Crumb without icon_color draws as before."
    required: true
    manual: false
  - id: "C5"
    text: "Unchanged tests still pass unmodified: tests::outline_kind_icons::every_kind_maps_to_a_bundled_icon, tests::outline_kind_icons::rows_carry_their_kinds_icon, and the symbol picker tests. tests::breadcrumb_symbol_icons::symbol_crumbs_carry_their_kinds_icon passes with one change only: the crumb for fn b inside impl S expects OutlineKind::Method (its math-function glyph, per C1), since the Rust outline reports b as a Method; nothing else in that test changes. cargo nextest run -p fe-workbench passes."
    required: true
    manual: false
  - id: "C6"
    text: "Licences: cargo nextest run -p fastedit --test third_party --test notice passes. THIRD_PARTY.md and kiln/THIRD_PARTY.md list puzzle, blocks, wand and key-round in the Lucide row and math-function in the Tabler row; the counts read 138 and 39; and the Lucide-only (ISC) classification and the Tabler paragraph name them. NOTICE and scripts/bundle-macos.sh are unchanged."
    required: true
    manual: false
  - id: "C7"
    text: "Lint: cargo clippy --workspace --all-targets -- -D warnings is clean (kiln crates need --workspace)."
    required: true
    manual: false
  - id: "C8"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden. workbench.png was regenerated with UPDATE_SCREENSHOTS=1 and looked at if it moved (the util module crumb in Yohen's module colour). No other golden changes."
    required: true
    manual: false
---

# Symbol icons in their syntax colours (function, type, constant, … from the editor theme) in the outline, symbol picker, call hierarchy and breadcrumbs, with distinct glyphs for method, trait, impl, macro, field and variant

# Symbol icons in their syntax colours, with a distinct glyph per kind

User decision: outline and symbol icons should match the file icons. File
rows already draw a line glyph in a theme colour (`TintedIcons`). Symbol rows
draw one muted grey glyph, and several kinds share a glyph or have a weak one.
This card makes two changes:

1. **Tint.** Each symbol-kind icon draws in the editor theme's syntax colour
   for its kind: functions in the function colour, types in the type colour,
   and so on. The colour comes from the active editor palette's `[syntax]`
   table, so every theme, bundled or user, gets it with no new tokens.
2. **Glyphs.** Function and method get different glyphs, and so do interface
   and trait. Impl (`Icon::Link` today) and macro (`Icon::Asterisk`) get
   glyphs that say what they are. Field and variant are fixed too.

It applies wherever `outline_panel::symbol_icon` is drawn:

- the Outline/Structure panel rows (`outline_panel.rs` `render_rows`)
- the go-to-symbol picker (`symbol_picker.rs` `render_match`)
- the call hierarchy rows (`hierarchy_panel.rs`, around :733)
- the breadcrumbs' symbol crumbs (`pane.rs`, around :2995)

## Today

- `crates/fe-workbench/src/outline_panel.rs:366` `symbol_icon(kind) -> fe_ui::Icon`
  maps `fe_syntax::OutlineKind` to a bundled `Icon`.
- `fe_ui` is `kiln-ui` (root `Cargo.toml`: `fe-ui = { package = "kiln-ui" }`).
  Icons are declared by the `icons!` macro in
  `kiln/crates/kiln-ui/src/file_icons.rs`, in a per-directory list:
  `"icons"` for Lucide 1.52.0 and `"tabler-icons"` for Tabler v3.50.0. Each
  SVG is compiled in from `kiln/crates/kiln-ui/assets/<dir>/<name>.svg`. Names
  must not repeat across the two sets.
- `TreeRow` (`kiln/crates/kiln-ui/src/tree_row.rs`) and `Crumb`
  (`kiln/crates/kiln-ui/src/breadcrumbs.rs`) draw a masked icon in
  `list.icon`. The symbol picker draws its `svg()` itself in `ui.icon`.
- The file tree tints its icons through `IconRef::Tinted(Icon, &'static str)`.
  That resolves a **kiln UI token** (`file_icon.*`, in
  `kiln/crates/kiln-core/src/theme/tokens.rs`) from the `COLORS` thread-local
  in `kiln/crates/kiln-ui/src/theme.rs` (`active_color`). Kiln tokens have no
  syntax colours. Those live in the theme file's `[syntax]` table, which the
  host reads into `fe_config::theme::EditorPalette::syntax`
  (`Vec<(String, Color)>`). The workbench gets that palette from
  `fe_gpui::editor_palette(cx)`, the `ActivePalette` global the host sets on
  every theme change.

## Design

### Tint: reuse the editor palette's syntax colours, not new tokens

Do **not** add a `symbol_icon.*` token group or new `IconRef` variant:

- A new token group breaks `design_role_tokens` and `theme_picker_tests`.
- It would also need values in every theme file.
- It could not follow the editor's syntax colours.

Instead:

- In `outline_panel.rs`, next to `symbol_icon`, add one mapping from a kind
  to a highlight name, `symbol_highlight(kind) -> &'static str`, and one
  lookup, `symbol_tint(kind, palette: &EditorPalette) -> Option<Hsla>`.
- The lookup takes the colour of the longest listed prefix of the highlight
  name in `palette.syntax`. That is the rule `fe_syntax::resolve_theme` uses,
  so `function.method` falls back to `function`. Convert the colour to Hsla as
  `fe-gpui/src/theme.rs` `hsla()` does (`rgba(color.to_rgba_u32()).into()`).
- Do one lookup per drawn row. Do not resolve the whole table per row.
- `None` means the theme colours nothing for that kind (Yohen has no
  `variable`). The icon then keeps its colour today: `list.icon` / `ui.icon`.

| OutlineKind | Highlight |
| --- | --- |
| Function | `function` |
| Method | `function.method` |
| Macro | `function.macro` |
| Class, Struct, Enum, Interface, Trait, Type, Impl, Table | `type` |
| Constant | `constant` |
| Variant | `constant.enum_member` |
| Field | `property` |
| Variable | `variable` |
| Module | `module` |
| Heading(_) | `markup.heading` |

T1542 (a dependency) colours symbol crumbs "in their syntax colours (the
kind's highlight)". If it has landed a kind-to-highlight mapping by the time
this runs, do not keep two mappings. Move that mapping into
`outline_panel.rs` as `symbol_highlight`, extend it to every kind above, and
have T1542's crumb label colour and these icons both use it. Where T1542 chose
a different highlight for a kind, keep T1542's choice, so its tests stay
green, and note the difference in the decision file.

### Plumbing in kiln-ui (generic, no syntax knowledge)

- `TreeRow::icon_color(Hsla)` sets a masked icon's tint in place of
  `list.icon`.
  - It follows `label_color`'s precedence: an error row's error colour and an
    ignored row's muted colour still win.
  - A coloured or `Tinted` icon ignores it, as it ignores the tint today.
  - Add a getter for host tests, as `detail_text()` / `status()` do:
    `pub fn icon_tint(&self) -> Option<Hsla>`.
- `Crumb::icon_color(Hsla)`, the same for a crumb: `breadcrumbs.rs:80` uses it
  instead of `list.icon` when set.
- Defaults are unchanged, so the file tree, the kiln gallery and its goldens
  do not move.

### Selected and focused rows: the tint stays

This matches the file tree. A selected `TreeRow` changes only its background
and accent edge (`TreeRow::colors`), and the label stays `text.primary`.
Tinted file icons keep their token colour when selected. So:

- A selected or focused symbol row keeps its kind's tint.
- The symbol picker's selected match keeps it (`render_match` ignores
  `selected` for the icon today, and keeps doing so).
- A hovered crumb keeps it.

### Glyphs

Lucide glyphs unless marked T (Tabler). Each new SVG is the upstream file at
the pinned commit, with the root `stroke-width="2"` changed to `"1.75"`. A
Tabler file also loses its leading metadata comment, as the existing ones did.
`tests::icon_stroke::every_bundled_icon_strokes_at_1_75` enforces the stroke.

| Kind | Today | New | Why |
| --- | --- | --- | --- |
| Module | box | box | unchanged |
| Class | component | component | unchanged |
| Struct | braces | braces | unchanged (`rows_carry_their_kinds_icon` pins it) |
| Enum | list | list | unchanged |
| Interface | plug | plug | a socket to implement |
| Trait | plug | **puzzle** (new) | now differs from interface |
| Impl | link | **blocks** (new) | an implementation block, not a link |
| Type | type | type | unchanged |
| Function | square-function | square-function | unchanged (`rows_carry_their_kinds_icon` pins it) |
| Method | square-function | **T math-function** (new) | now differs from function |
| Constant | hash | hash | unchanged |
| Macro | asterisk | **wand** (new) | expansion, not a wildcard |
| Table | table | table | unchanged |
| Heading(n) | heading | heading | every level shares one |
| Field | circle-dot | **key-round** (new) | a named slot; also JSON/YAML keys (`Field`) |
| Variant | diamond | **circle-dot** (moved) | a radio mark: one of a set |
| Variable | variable | variable | unchanged |

New `Icon` variants:

- Lucide list (`"icons"`), under the "outline's symbol kinds" comment:
  `Puzzle => "puzzle"`, `Blocks => "blocks"`, `Wand => "wand"`,
  `KeyRound => "key-round"`.
- Tabler list (`"tabler-icons"`): `MathFunction => "math-function"`.
- All five were checked to exist upstream.
- Keep `Link`, `Asterisk` and `Diamond`. Other surfaces still use `Diamond`
  and `CircleDot` (diagnostics picker, `.sol` files, kiln tests), and removing
  icons is not this card's job.
- A worker may swap a proposed glyph for one that is **already bundled**, and
  must note the swap in `docs/decisions/<lane>-*.md`. A different new SVG
  would be outside the scope, so raise it as a finding instead.

### Licence rows

The four new Lucide icons are Lucide-only. Their `icons/<name>.json` at
`500620a2` credits no Feather contributor (puzzle: karsa-mistmere, ericfennis,
jamiemlaw; blocks and key-round: danielbayley, jguddas; wand: mittalyashu,
ericfennis), so they are ISC. `math-function` is Tabler MIT. In **both**
`THIRD_PARTY.md` and `kiln/THIRD_PARTY.md`:

- add the files to the Lucide and Tabler rows' file lists, alphabetically
- bump "The 134 SVGs" to 138 and "The 38 SVGs" to 39
- add an "added for the outline's symbol kinds" bullet to the Lucide
  classification: `blocks`, `key-round`, `puzzle` and `wand` are
  Lucide-only, so ISC
- extend the Tabler paragraph's list of what it holds (brand marks, letter
  monograms, `markdown`) with `math-function`, a symbol-kind glyph

`NOTICE`, `kiln/NOTICE`, the LICENSE files and `scripts/bundle-macos.sh` do not
change, because no new icon set is added. `crates/fastedit/tests/third_party.rs`
and `notice.rs` check the rows.

## Tests

### New `crates/fe-workbench/src/tests/symbol_icon_tints.rs`

Declare it with one `mod` line in `crates/fe-workbench/src/tests/mod.rs`.

- `every_kind_has_its_own_glyph`
  - Every kind in `outline_kind_icons`' list maps to an icon in `Icon::ALL`.
  - The glyphs are pairwise distinct, except that all `Heading(n)` share one.
  - Spelled out: Function != Method, Interface != Trait, Impl is not `Link`,
    Macro is not `Asterisk`, Field != Variant.
- `tints_follow_yohen_and_hakuji_syntax_colours`
  - Use `EditorPalette::fastedit_dark()` (Yohen) and `fastedit_light()`
    (Hakuji). Under each, `symbol_tint` gives that palette's colour for:
    - Function and Method: `function`
    - Struct, Class, Trait and Type: `type`
    - Constant: `constant`
    - Macro: `function.macro`
    - Module: `module`
  - Function, type and constant differ between the two palettes (Yohen
    `#f2c77a` / `#7fc9be` / `#f0a3c2`, Hakuji `#925a00` / `#0d6e66` /
    `#a32f6b`).
- `a_kind_the_theme_leaves_uncoloured_has_no_tint`
  - Under Yohen, which lists no `variable`, `symbol_tint(Variable)` is
    `None`.
  - A caller given `None` sets no `icon_color`, so the icon draws in
    `list.icon` as today.
- `outline_rows_take_their_kinds_tint_selected_or_not`
  - Set Hakuji with `fe_gpui::set_editor_palette`. Open a Rust file with
    `struct Point;` and `fn main() {}` in the outline (cmd-alt-o, as
    `outline_kind_icons::rows_carry_their_kinds_icon` does).
  - `render_rows(0..2, cx)`: Point's `icon_tint()` is Hakuji's `type`, and
    main's is Hakuji's `function`.
  - Select main, focused and then unfocused. Its tint is unchanged.
- `symbol_crumb_icons_take_their_kinds_tint`
  - Use the `breadcrumb_symbol_icons` setup (module `outer`, `impl S`, fn
    `b`). The crumbs' icon colours are `module`, `type` and `function`
    under the palette set.
  - Record them through a new test hook beside `Pane::drawn_crumbs`, or
    whatever crumb-colour hook T1542 added. Do **not** change
    `drawn_crumbs()`' type, so `breadcrumb_symbol_icons.rs` stays untouched.

### New `kiln/crates/kiln-ui/src/tests/tree_row_icon_color.rs`

Declare it in `kiln/crates/kiln-ui/src/tests/mod.rs`.

- `an_icon_colour_tints_the_icon_in_every_selection_state`
  - `TreeRow::colors(..).icon` is the given colour when unselected,
    selected, and selected and focused. The background and edge are as
    `tree_row::colors_follow_state` expects.
- `error_and_ignored_rows_still_win_over_an_icon_colour`

### Existing tests stay green unmodified

- `tests::outline_kind_icons::*`: `braces` and `square-function` keep their
  kinds.
- `tests::breadcrumb_symbol_icons::*`
- `symbol_picker_tests`
- `tree_row::colors_follow_state`
- `tests::assets::bundled_files_match_the_enum`

## Screenshots (Mac-only)

- `crates/fe-workbench/examples/screenshots/golden/workbench.png` shows the
  `util` module crumb, which takes Yohen's `module` colour. It moves unless
  T1542 already tinted crumb icons.
- No other golden draws a symbol icon:
  - `file_icons_*` and `setup_wizard_*` show the Outline collapsed or no
    editor.
  - fe-gpui goldens have no workbench.
  - The kiln gallery goldens use default crumbs and rows.
- There is no golden scene for the expanded outline, the symbol picker or
  the call hierarchy, and this card adds none.

## Out of scope

- Symbol-kind colours or glyphs in completion menus, workspace symbols
  (cmd-T) or LSP item kinds.
- New theme tokens, or theme file changes.
- Removing now-unused glyphs.
- The Outline-to-Structure rename (T1541).
