---
task_uid: "01m6zgy0fmff403f0975s3x0m3"
task: "T337"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Parser tests pass (cargo nextest run -p fe-workbench): color_literals::tests::hex_forms, color_literals::tests::hex_boundaries, color_literals::tests::rgb_comma_and_space_syntax, color_literals::tests::hsl_hwb_and_hue_units, color_literals::tests::lab_lch_oklab_oklch_convert, color_literals::tests::out_of_gamut_oklch_clips, color_literals::tests::color_srgb_and_named_colours, color_literals::tests::invalid_forms_rejected, color_literals::tests::css_colors_finds_each_literal_in_order"
    required: true
    manual: false
  - id: "C2"
    text: "Workbench tests pass (cargo nextest run -p fe-workbench): lsp::colors::default_colors::tests::a_css_buffer_without_a_server_gets_swatches, lsp::colors::default_colors::tests::hex_in_strings_only_outside_css, lsp::colors::default_colors::tests::style_in_html_gets_swatches, lsp::colors::default_colors::tests::a_server_answer_replaces_them_in_auto, lsp::colors::default_colors::tests::always_ignores_the_server, lsp::colors::default_colors::tests::never_shows_none_without_a_server, lsp::colors::default_colors::tests::color_decorators_off_shows_none, lsp::colors::default_colors::tests::an_edit_rescans_after_the_rest, lsp::colors::default_colors::tests::only_rows_near_the_view_are_scanned"
    required: true
    manual: false
  - id: "C3"
    text: "The existing lsp::colors::tests (lsp_colors_tests.rs, unmodified) still pass: server swatches, their refresh after edits, and editor.color_decorators off behave as before"
    required: true
    manual: false
  - id: "C4"
    text: "fe-config: default_is_auto_and_values_parse and in_the_template_and_schema (cargo nextest run -p fe-config --test default_color_decorators_setting) pass; UPDATE_GOLDEN=1 regenerates settings.schema.json (and the generated settings.ftl and settings-template.ftl if T1530 has landed at the base); cargo nextest run -p fe-config passes"
    required: true
    manual: false
  - id: "C5"
    text: "No new crate dependency (Cargo.toml and Cargo.lock unchanged), nothing in fe-gpui changes, and no screenshot golden changes"
    required: true
    manual: false
---

# Built-in colour swatches without a language server: CSS colour literals (hex, rgb/hsl/hwb/lab/lch/oklab/oklch/color(srgb), named) and hex in other languages' strings, behind editor.default_color_decorators (auto, always, never), as VS Code's editor.defaultColorDecorators (Mac T1646)

# Built-in colour swatches, no language server needed

## Problem

The user asked for colour previews like VS Code's: a small swatch before each
colour value in a CSS file (`#6c6976`, `#ffffff12`, `oklch(0.62 0.08 195)`,
`rgba(0, 0, 0, 0.62)`). Tenmoku already draws swatches
(`crates/fe-gpui/src/color_swatches.rs`, `EditorView::set_color_swatches`,
T1113), but only from a language server's `textDocument/documentColor`
(`crates/fe-workbench/src/lsp_colors.rs`). The built-in server list
(`crates/fe-lsp/src/registry.rs` `builtin_specs`) has no CSS server, so CSS
files get none. VS Code covers this case with `editor.defaultColorDecorators`.
It finds colours itself when no provider answers.

## Change

### 1. Setting `editor.default_color_decorators`

- `crates/fe-config/src/settings.rs`: a new `EditorSettings` field
  `default_color_decorators: DefaultColorDecorators`, an enum with
  `#[serde(rename_all = "snake_case")]` like `InlineDiagnostics`:
  - `Auto` (the default): built-in swatches for a buffer unless its language
    server answers `documentColor` with colours. When it does, the server's
    swatches replace the built-in ones. The two are never shown together.
  - `Always`: built-in swatches only. The server isn't asked for colours.
  - `Never`: the server's swatches only, as today.

  Doc comments on the field and on each variant become the schema text.
  `editor.color_decorators = false` still turns every swatch off, built-in
  ones too. Update `color_decorators`' doc comment so it no longer says
  "a language server finds".
- `crates/fe-config/settings-template.toml`: add
  `# default_color_decorators = "auto"` with a comment after
  `# color_decorators = true`. `tests/template.rs` requires every key.
- `crates/fe-config/settings.schema.json`: regenerate it with
  `UPDATE_GOLDEN=1`. If T1530 has landed at the lane's base
  (`crates/fe-config/l10n/en-US/settings.ftl` exists), regenerate the
  generated `settings.ftl` and `settings-template.ftl` the same way.
  Otherwise T1530's generator picks the key up when it lands.
- Propagation follows `color_decorators_on`. Set the value once when the
  workbench is built (`crates/fe-workbench/src/workbench.rs`, beside
  `workbench.lsp.color_decorators_on.set(...)`) and again on settings reload
  (`crates/fe-workbench/src/workbench_root.rs`, beside the same line). Both
  call one `pub(crate) fn set_default_color_decorators(&mut self, mode, cx)`
  defined in the new module (step 3). A change re-picks every open editor's
  swatches at once.

### 2. Colour parser: `crates/fe-workbench/src/color_literals.rs` (new, pure)

The file has no GPUI and no tree-sitter. It is declared `mod color_literals;`
in `crates/fe-workbench/src/lib.rs`. Its tests live in
`color_literals_tests.rs` (`#[cfg(test)] #[path] mod tests;`). It lives in
fe-workbench, the only user. A later colour-picker card can move it down.

API, with these suggested names:
- `pub(crate) fn parse_color(literal: &str) -> Option<[f32; 4]>`: one whole
  literal to sRGB red, green, blue and alpha, each in `0..=1`.
- `pub(crate) fn css_colors(text: &str) -> Vec<FoundColor>`: every literal in a
  run of CSS text.
- `pub(crate) fn hex_colors(text: &str) -> Vec<FoundColor>`: only
  `#rrggbb` and `#rrggbbaa`.
- `FoundColor { range: Range<usize>, rgba: [f32; 4] }`, sorted by start.

Literals (function and colour names are case-insensitive):
- Hex: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`. A literal must not follow an
  ASCII letter, digit, `_` or `&`, so `&#123456;` is not a colour. It must not
  be followed by a letter, digit, `_` or `-`, so `#1234567` and `#fffg` are
  rejected.
- `rgb()` / `rgba()`: either the legacy comma syntax or the modern space
  syntax, never a mix of the two.
  - Comma syntax: all numbers or all percentages, with an optional alpha.
  - Space syntax: numbers, percentages or `none` (counts as 0), then an
    optional `/ alpha`.
  - Alpha is a number or a percentage, clamped to `0..=1`.
- `hsl()` / `hsla()`: the hue takes a bare number (degrees), `deg`, `grad`,
  `rad` or `turn`. Comma and space syntax as for `rgb()`.
- `hwb(H W% B% [/ A])`: when white and black add up to more than 100%, both
  are scaled to sum to 100%, as CSS Color 4 does.
- `lab(L a b [/ A])` and `lch(L C H [/ A])`:
  - These are CIE Lab in D50. Convert through XYZ D50 → Bradford → XYZ D65 →
    linear sRGB → sRGB gamma, using CSS Color 4's matrices.
  - Percentage scales: L 100% = 100, a and b 100% = 125, C 100% = 150.
- `oklab(L a b [/ A])` and `oklch(L C H [/ A])`:
  - Convert through Ottosson's OKLab → LMS → linear sRGB matrices (as given in
    CSS Color 4).
  - Percentage scales: L 100% = 1, a, b and C 100% = 0.4.
- `color(srgb r g b [/ A])`: components are numbers in 0..1 or percentages.
  Other colour spaces (`display-p3`, `srgb-linear`, …) are not colours here.
- Gamut: after conversion, clamp each channel to `0..=1` (a simple clip,
  documented on the function; CSS's chroma-reducing gamut mapping is out of
  scope).
- Named colours: the CSS Color 4 list of 148 names (including the `grey`
  spellings and `rebeccapurple`), plus `transparent` (`[0, 0, 0, 0]`). Not
  `currentColor` or system colours.
- Anything that isn't a literal makes the whole function "no colour":
  `var(…)`, `calc(…)`, `env(…)`, relative colours (`rgb(from …)`), a missing
  or extra component, a unit where none belongs, or a missing `)`. A hex
  literal inside such a function is still a colour on its own, for example
  the fallback in `var(--x, #fff)` or SCSS's `darken(#336699, 10%)`.

No new crate dependency. `Cargo.lock` has no colour-conversion crate today
(checked: only `color_quant`, an unrelated image quantizer, and `termcolor`),
so the math is written here. It is roughly 150 lines with the matrices.

### 3. Built-in scanning: `crates/fe-workbench/src/default_colors.rs` (new)

This module is a child of `lsp::colors`, declared from `lsp_colors.rs` as
`#[path = "default_colors.rs"] mod default_colors;`. As a child it can reach
the private `self.lsp.colors` state with no change to `lsp.rs`. Its tests are
in `default_colors_tests.rs` (`#[cfg(test)] #[path] mod tests;`).

**Placement: bounded to the rows in view, on the UI thread.** This follows
`todo_comments.rs`. `Syntax` isn't `Send`, and the scan reads its highlights
and trees, so it can't run on another thread. Each editor shown scans:
- the rows `spell_squiggles::rows_to_check(view)` gives, which are the rows in
  view plus `MARGIN` (50) rows either side;
- once the editor's parse is current at its version (the same `behind` and
  `is_loading_language` checks as `scan_todo_comments`);
- right away on first show, then 500 ms after the last edit (`lsp_colors.rs`'s
  `REST`, which a child module can reach as `super::REST`), the same debounce
  as the server request. A scroll to rows not yet scanned scans them after
  `REST`.

Rows already scanned stay scanned until the next edit or language change.
Highlights that didn't finish are retried like `todo_comments`, up to 8 times,
compiling missing injected languages off the thread. There are no built-in
swatches for a buffer over `spell_ranges::MAX_SPELLABLE_BYTES` (2 MiB, the
same bound `todo_comments` uses) or for one with no grammar. The scan runs in
untrusted folders too: it starts no process.

What it finds:
- **CSS-family text.** This is the whole buffer when the host language is
  `css`, `scss`, or any `less`, `sass` or `stylus` registered at run time.
  It is also each injected layer in one of those languages, found with
  `Syntax::injected_nodes(window)` (layer roots: `parent == None`) after
  `checked_highlights(window)` has parsed them. That covers HTML `<style>`
  (tree-sitter-html's own injections) and Vue, Svelte and Astro `<style>`
  (their extension injections, `lang`-less or `type="text/css"`), and
  Markdown ```` ```css ```` fences come free.
  - Within it, `css_colors` runs only on the text of `declaration` nodes,
    which drops `#add`-style id selectors. Walk `syntax.tree()` for the host,
    as `spell_ranges::host_string_nodes` does, and use the injected nodes for
    layers. Comment nodes are skipped.
  - Check tree-sitter-scss's node kind for `$var: #fff` and include it if it
    isn't `declaration`.
  - Named colours count only as a whole `plain_value` token. Property names,
    function names and `--custom` identifiers never count.
- **Every other language.** Only `hex_colors` (6 or 8 digits), only inside
  spans whose highlight is `string` (`checked_highlights` over the window). So
  `"#d4856a"` in a Tenmoku theme TOML gets a swatch, and `#123` issue numbers
  and comments don't.

**Choosing what an editor shows: one owner of `set_color_swatches`.**
- `lsp_colors.rs` stops calling `give_swatches` directly. Each buffer's `Slot`
  keeps its latest accepted server swatches (`Some(non-empty)` only when the
  server answered colours). Every place that gives or clears today (an answer,
  an empty answer, no provider, an error past retries, the server or opening
  moving, the slot dropped) updates that and calls one `show_color_swatches`
  per editor.
- That function, in `default_colors.rs`, applies a pure rule:
  - `color_decorators` off: nothing.
  - `Never`: the server's swatches or nothing.
  - `Always`: the built-in ones.
  - `Auto`: the server's if non-empty, else the built-in ones.
- It calls `set_color_swatches` only when the result differs from what the
  editor shows.
- `sync_colors` doesn't ask the server in `Always`, as if the server had no
  colour provider.
- In `Auto`, built-in swatches may show until the first server answer replaces
  them. That is accepted. A server that later answers empty hands the editor
  back to the built-in swatches without a rescan.

The sync runs from the end of `sync_colors` (`self.sync_default_colors(cx)`),
which `sync_lsp` already runs after each draw, so `lsp.rs` and the render code
don't change. Rendering, edit-following and the caret's skipping are
`set_color_swatches`' as they are today. Nothing in fe-gpui changes.

## Tests (new files only)

`crates/fe-workbench/src/color_literals_tests.rs`:
- `hex_forms`: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, including the
  screenshot's `#6c6976` (`[0x6c, 0x69, 0x76]/255`, alpha 1) and `#ffffff12`
  (alpha `0x12/255`).
- `hex_boundaries`: `&#123456;`, `a#fff`, `#1234567` and `#fffg` are rejected,
  and `#fff;` and `(#fff)` are found.
- `rgb_comma_and_space_syntax`: the screenshot's `rgba(0, 0, 0, 0.62)` is
  `[0, 0, 0, 0.62]`. Also `rgb(255 0 0 / 50%)`, `rgb(100%, 0%, 0%)` and
  `rgb(none 255 0)`.
- `hsl_hwb_and_hue_units`: the same colour by `deg`, `turn`, `rad` and `grad`,
  `hsla` with alpha, and `hwb` with white + black > 100%.
- `lab_lch_oklab_oklch_convert`, each within 1/255 per channel:
  - `oklch(0.62 0.08 195)` (from the screenshot) → `#429595`
  - `oklab(0.5 0.1 -0.1)` → `#81459a`
  - `lab(50 40 -20)` → `#ab5a9a`
  - `lch(50 40 300)` → `#816cb2`
  - `lab(100 0 0)` and `oklch(1 0 0)` → `#ffffff`
- `out_of_gamut_oklch_clips`: `oklch(0.7 0.4 150)` → `#00d600` within 1/255,
  and every channel is in `0..=1`.
- `color_srgb_and_named_colours`: `color(srgb 1 0.5 0 / 0.5)`,
  `rebeccapurple`, `RED`, `grey` = `gray`, `transparent` = `[0, 0, 0, 0]`.
  `currentColor` is not a colour.
- `invalid_forms_rejected`: `var(--c)`, `rgb(var(--r) 0 0)`,
  `rgb(calc(1) 0 0)`, `rgb(from red r g b)`, `rgb(1, 2)`, `rgb(1 2, 3)`,
  `hsl(10px 50% 50%)`, `color(display-p3 1 0 0)`, `rgb(1 2 3` (no `)`),
  `#12345`.
- `css_colors_finds_each_literal_in_order`: the screenshot's four values on
  one line, with their byte ranges.

`crates/fe-workbench/src/default_colors_tests.rs`. These are headless
workbench tests: fake clock, and `crate::tests::lsp::Lsp` for the server ones,
as `lsp_colors_tests.rs` does.
- `a_css_buffer_without_a_server_gets_swatches`: a `.css` file holding the
  screenshot's declarations gets one swatch per value, at each literal's
  start, with the parsed rgba. An id selector `#add {}` and a
  `/* #fff */` comment get none.
- `hex_in_strings_only_outside_css`: a `.toml` file with
  `accent = "#d4856a"`, `# see #123 and #abcdef` and `n = "#abc"` gets exactly
  one swatch, on `#d4856a`.
- `style_in_html_gets_swatches`: `<style>a { color: #ff0000 }</style>` in a
  `.html` file gets one swatch.
- `a_server_answer_replaces_them_in_auto`:
  - A Rust file with `"#00ff00"` in a string shows the built-in swatch.
  - When the fake server answers one different colour elsewhere, the editor
    shows exactly the server's swatch.
  - When the server's next answer is empty, the built-in swatch is back.
- `always_ignores_the_server`: with `"always"`, no `documentColor` request is
  sent and the built-in swatch shows.
- `never_shows_none_without_a_server`: with `"never"`, the `.css` file gets
  none.
- `color_decorators_off_shows_none`.
- `an_edit_rescans_after_the_rest`: typing a new colour shows its swatch only
  once 500 ms have passed with no further edit.
- `only_rows_near_the_view_are_scanned`: in a 2,000-row CSS file, a colour on
  row 1,900 has no swatch until the editor scrolls there and `REST` passes.

`crates/fe-config/tests/default_color_decorators_setting.rs`:
- `default_is_auto_and_values_parse`: the default is `Auto`; `"always"` and
  `"never"` parse; `"sometimes"` is a diagnostic.
- `in_the_template_and_schema`: the template has
  `# default_color_decorators = "auto"`, and the schema's enum is
  `["auto", "always", "never"]` with default `"auto"`.

The existing `lsp::colors::tests::*` (`lsp_colors_tests.rs`, unmodified) and
`color_decorators_setting` tests pass unchanged.

## Out of scope

- A colour picker on click (VS Code's `textDocument/colorPresentation` UI).
  `LspManager::color_presentations` exists for it. That is a later card.
- CSS's chroma-reducing gamut mapping (this card clips), `color()` spaces
  other than `srgb`, relative colours, `var()` resolution, and colours in
  `style="…"` attributes, which the HTML injections deliberately leave as
  strings.
- Vue `<style lang="scss">` (its injections leave that plain), and CSS-in-JS
  template strings.
- A screenshot golden. The swatch drawing is T1113's and is unchanged.

Platform: **Linux-OK**. The tests are headless, assertions are on swatch
offsets and colours, and there are no pixel goldens.

## Implementation notes

- T1530 hasn't landed at the base (`crates/fe-config/l10n/en-US/` doesn't
  exist), so only `settings.schema.json` was regenerated.
- tree-sitter-scss (tree-sitter-grammars, the pinned wasm grammar) parses
  `$var: #fff` as a `declaration` whose `property_name` is `$var` (its
  highlights match `property_name` against `^(--|\$)`), so the one kind
  covers SCSS variables too.
- Which swatches an editor shows is `show_color_swatches`'s alone. Each
  source keeps its colours at the version they were found at (a `Slot`'s
  `answered`, a scan's `found`), and the chooser carries them to the
  buffer's current version with the same edit-following the editor uses
  (`fe_gpui::inlay_hints::follow` is crate-private, so `default_colors.rs`
  has its own copy), then compares with `EditorView::color_swatches()` and
  sets only on a difference. It runs only on an event (an answer, a scan,
  an editor joining, a server or opening moving, a slot dropped, a setting
  change), never per frame, so swatches a source already shows keep
  following edits untouched until that source has something new.
- In `always`, `sync_colors` treats the buffer as having no server, which
  drops its slot; going back to `auto` makes a fresh slot that asks at once.
- `parse_color` is used only by the tests until the colour-picker card
  (`cfg_attr(not(test), allow(dead_code))`).
