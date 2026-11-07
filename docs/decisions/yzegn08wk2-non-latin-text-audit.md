# Non-Latin text in the editor: audit (T1431)

## Status

Audit done on macOS (2026-11-07). What works is locked in by `crates/fe-gpui/tests/non_latin_text.rs` (24 tests);
11 gaps are written up below as G1 to G11, with 7 proposed cards. Nothing was fixed and no card was filed: the lead
files the cards below, and the user (who approved this audit on 2026-11-06) approves the follow-ups. Linux was not
run: every finding about shaping is from CoreText, and is unverified under the Linux shaper (cosmic-text through
`gpui-pre-wgpu`).

## What was checked

How the tests run: logic tests use `#[gpui::test]` (GPUI's `NoopTextSystem`, every char one em, no shaping) and run
on any OS. Tests of what shaping decides use CoreText and are `#[cfg(target_os = "macos")]`. GPUI builds CoreText's
text system only inside `MacPlatform::new`, which panics off the main thread ("Mac platform not created on main
thread"), and libtest runs every test on a thread of its own, so `TestAppContext::build_with_text_system(..,
gpui_platform::current_platform(true).text_system())`, as the spec suggested, can't run in a `#[test]`. The file
builds the platform before `main` instead, from a `__DATA,__mod_init_func` initialiser (module `shaped`), and keeps
it for the process; no Cargo change was needed. Geometry is read through the public `EntityInputHandler`
(`bounds_for_range` gives the caret's bounds, `character_index_for_point` hit-tests), clicks are simulated mouse
events, and glyph order comes from `window.text_system().layout_line` in the editor's font.

Not observable from the public API, so checked by reading the code only: the vim block caret's width (`block_rect`,
`crates/fe-gpui/src/element.rs:2530`, spans the head to the next grapheme's x: one cluster's shaped width, or a
space where that x isn't right of the head, as inside an RTL run, G1) and the selection rects' x (fe-display
`lay_out_frame`, `crates/fe-display/src/layout.rs:1495`, one rect per display row from the x of its first selected
offset to the x of its last).

Per area, what passes:

- **RTL and mixed direction.** `rtl_logical_caret_steps_one_grapheme`: Right steps one grapheme in byte order
  through `let s = "שלום"; // مرحبا`, shift-Right selects in byte order, Home and End go to the logical ends (VS Code
  moves logically too). `rtl_shaper_paints_rtl_runs_right_to_left`: CoreText applies the Unicode bidi algorithm, so
  the display order is right (and the same as VS Code's, neutrals `"; // ` between the two RTL strings included).
  `rtl_hit_testing_a_mixed_row_lands_on_char_boundaries`: hit-testing and clicking along a mixed row never panics and
  always gives a char boundary on the row. `rtl_double_click_selects_the_rtl_word_under_the_pointer`: a double-click
  on a Hebrew or Arabic glyph selects its word. Limitations pinned: `rtl_carets_inside_an_rtl_span_share_one_x` (G1),
  `rtl_click_on_the_last_painted_rtl_glyph_lands_at_the_row_end` (G2), `rtl_bidi_controls_reorder_text_and_draw_nothing`
  (G4).
- **Full-width CJK.** `cjk_vim_gj_gk_move_by_display_row_over_wrapped_wide_text`: `gj`/`gk` move by display row
  over a soft-wrapped CJK row, keeping their place. `cjk_vim_bar_goes_to_the_display_column`: `4|` on `一二三abc`
  lands on `二`, `7|` on `a`. Soft wrap only breaks between chars. Rulers sit at N space advances
  (`crates/fe-gpui/src/element/rulers.rs`), in pixels, as VS Code's do: not a gap. No vim option changes display
  width: `:set ambiwidth` and `:set display` are absent (G11). Limitations pinned:
  `cjk_soft_wrap_at_a_column_counts_the_shaped_width` (G8), `cjk_up_down_keep_the_char_column_not_the_x` (G5),
  `cjk_keyboard_column_select_counts_chars` and `cjk_alt_shift_drag_selects_a_box_of_chars` (G7),
  `cjk_vim_visual_block_edges_over_wide_chars_do_not_line_up` (G8).
- **Grapheme clusters.** `grapheme_left_right_step_over_whole_clusters`: a ZWJ family, a flag, a keycap, `e` +
  U+0301, Hangul jamo (U+1112 U+1161 U+11AB) and a Devanagari conjunct (क्ष) each take one Left or Right step.
  `grapheme_backspace_and_delete_remove_whole_clusters`: Backspace and Delete take each whole (G9 notes the VS Code
  difference). `grapheme_clicks_inside_a_cluster_land_on_its_edges`: the family is one glyph, its caret advance is
  that glyph's, a click inside lands on its nearer edge, and a double-click selects it.
  `grapheme_vim_keys_act_on_whole_clusters`: vim's `l`, `x`, `r` and `~` act on the whole cluster through the view.
  Limitation pinned: `grapheme_up_down_can_land_inside_a_cluster` (G6).
- **IME** (only what `view/ime_tests.rs`, `vim/insert_tests.rs` and `vim/macro_tests.rs` leave).
  `ime_commit_in_vim_block_insert_repeats_on_every_row`: a composition in `<C-v>jjI` stays out of the buffer and the
  commit is repeated on every row at `<Esc>`, one undo step. `ime_composition_in_replace_mode_overwrites_wide_chars_on_commit`:
  in `R` over CJK the composition leaves the buffer until the commit, which overwrites one grapheme per char, and
  `<BS>` restores them. `ime_composition_at_carets_on_rows_of_different_widths`: with carets after CJK and after
  ASCII, the input method's window is at the primary caret, as wide as the composition is drawn, and the commit types
  once at each caret. An IME window inside an RTL run is placed by G1.
- **Font fallback.** `fallback_cjk_and_emoji_draw_from_other_fonts_and_measure_as_drawn`: with the default font
  (JetBrains Mono) CoreText draws `一` and `😀` from two other fonts, and every caret x is the shaped advance
  (`GlyphWidths`, `crates/fe-gpui/src/element.rs:551`, measures clusters through the same shaper). Limitation pinned:
  `fallback_the_editor_font_names_no_fallbacks` (G10).

## Findings

### Bidi display, caret placement and hit-testing for RTL runs

Display works; caret placement doesn't; hit-testing half works. CoreText decides display: gpui-pre-apple 0.3.9
`text_system.rs` `layout_line` builds a `CTLine` and copies `glyph_runs()` in visual order, each glyph keeping its
logical byte `index`, so an RTL run's glyphs come with indices descending as x rises (and neighbouring runs of one
font are merged into one `ShapedRun`). Everything the editor derives from x goes through gpui-pre 0.3.9
`src/text_system/line_layout.rs`, which assumes indices ascend with x:

- `LineLayout::x_for_index` returns the x of the first glyph in paint order whose index is at or past the one asked
  for. Inside an RTL span that is always the span's leftmost glyph (the logically last), so every offset in the span
  gets one x. fe-gpui uses it for carets and selections through `impl MeasuredLine for Line`
  (`crates/fe-gpui/src/element.rs:466`, `x_for_index`), which fe-display's `RowLayout::x_for_offset`
  (`crates/fe-display/src/layout.rs:326`) and `Frame::caret_bounds` (`:581`) call; fe-gpui has no maths of its own
  for it.
- `LineLayout::closest_index_for_x` returns the index of the first glyph at or right of x (or the one before it),
  scanning in paint order, and `self.len` past the last glyph. fe-gpui uses it for clicks and the input method's
  `character_index_for_point` (`Line::index_for_x`, element.rs:475, through fe-display `RowLayout::offset_for_x`,
  layout.rs:380), and `LineLayout::index_for_x` for `FrameLayout::char_at` (element.rs:184). A glyph's logical index
  comes back, so a double-click lands inside the right word; but which edge of an RTL glyph is which is reversed, and
  a click right of the last glyph's left edge is "past the end".

### G1: Carets inside a right-to-left span all get the span's left edge

- Repro: buffer `let s = "\u{5e9}\u{5dc}\u{5d5}\u{5dd}"; // \u{645}\u{631}\u{62d}\u{628}\u{627}` (`let s =
  "שלום"; // مرحبا`), caret at 8, press Right 10 times.
- Expected: the caret moves one letter at a time across the Hebrew right to left (VS Code: logical order, visual
  caret on the letter's logical edge), then the Arabic.
- Actual: offsets 9 to 31 are all drawn at one x, the left edge of the RTL span (`rtl_carets_inside_an_rtl_span_share_one_x`).
  The caret jumps to the span's left edge and stays there from offset 9 to 31, 14 presses; the vim block caret there is a space wide at that x (`block_rect`); a
  selection inside the span is a zero-width rect; the IME candidate window opens at that x.
- Suspected code site: gpui-pre `LineLayout::x_for_index` as above, reached from fe-gpui `Line::x_for_index`
  (element.rs:471).
- Severity: high for anyone editing Arabic, Hebrew or Persian strings or comments; nil otherwise.

### G2: Hit-testing reverses RTL glyph edges, and the last-painted RTL glyph hits the row end

- Repro: buffer `let s = "שלום` (the row ends in Hebrew), click on the right half of the rightmost glyph (ש, byte 9).
- Expected: caret at 9 (right edge of ש, its logical start), as VS Code does.
- Actual: caret at the row's end, 17 (`rtl_click_on_the_last_painted_rtl_glyph_lands_at_the_row_end`).
  Inside a span, a click at a glyph's left edge gives that glyph's index, its logical start, though in RTL that
  edge is its logical end.
- Suspected code site: gpui-pre `LineLayout::closest_index_for_x` and `index_for_x`; fe-gpui `Line::index_for_x`
  (element.rs:475), `FrameLayout::char_at` (element.rs:184).
- Severity: medium (clicks land a glyph off or at the row end; nothing is corrupted).

### G3: A selection over an RTL span is one rect per row

- Repro: the G1 buffer, select 9..13 (`של`) with shift-Right, or 5..20 across `"שלום";`.
- Expected: the highlighted glyphs are the selected ones; a logical range crossing a direction change is several
  visual pieces (VS Code draws one rect per visual piece).
- Actual: fe-display `lay_out_frame` (layout.rs:1495) pushes one rect per display row from x(start) to x(end); with
  G1 a selection inside the span is zero wide, and with G1 fixed it would still cover unselected glyphs wherever the
  range crosses a direction change.
- Suspected code site: `crates/fe-display/src/layout.rs` `lay_out_frame`, the selection rect loop.
- Severity: medium for RTL users (the selection isn't shown; copying it works).

### G4: Bidi controls and invisible chars draw nothing (Trojan Source)

- Repro: buffer `a\u{202e}bc\u{202c}d e\u{200b}f`.
- Expected (VS Code `editor.unicodeHighlight.invisibleCharacters`, on by default, and its bidi warning): the RLO,
  PDF and zero-width space are drawn as visible marks, so `cb` reads as reordered text.
- Actual: CoreText reorders `bc` to `cb` and none of the three gets a glyph
  (`rtl_bidi_controls_reorder_text_and_draw_nothing`): the row reads `acbd ef` with no hint why. The same holds
  for U+202A..U+202E and U+2066..U+2069, and other zero-width chars (U+200B..U+200D, U+2060, U+FEFF); confusables
  (Cyrillic `а` for Latin `a`) aren't marked either. fe-vim's width table counts the zero-width space as
  six display columns, as nvim draws it `<200b>` (`crates/fe-vim/src/chars.txt`, `widest` in
  `crates/fe-vim/src/motion.rs:569`), but the view draws nothing, so vim's columns on such a row disagree with the
  screen. Nothing in fastedit looks for these chars.
- Suspected code site: none exists; the place is fe-gpui's row painting (`crates/fe-gpui/src/element.rs`, with
  `element/whitespace.rs` as the model of a per-char overlay).
- Severity: medium (a security hazard in review, CVE-2021-42574).

### G5: Up and Down keep a char column, not a visible column

- Repro: buffer `abcdefghij\n一二三四五六七八\nabcdefghij`, caret at 6, press Down.
- Expected: VS Code keeps the visible column (a wide char is two), landing after `三` (offset 20); nvim's `j` keeps
  the display column likewise.
- Actual: lands after `六` (offset 29), the sixth char, drawn about 1.67 times as far right
  (`cjk_up_down_keep_the_char_column_not_the_x`). Down again goes back to column 6, so the caret zigzags.
- Suspected code site: `crates/fe-editor/src/lib.rs:1112` `EditorModel::vertical` (`char_column`,
  `display_col_for_char_column`); the read-only viewer's copy, `crates/fe-editor/src/viewer.rs:290`.
- Severity: low (wide-text editing is awkward, nothing breaks).

### G6: Up and Down can land inside a grapheme cluster

- Repro: buffer `abcdef\ne\u{301}xyz\n👨\u{200d}👩\u{200d}👧xyz`, caret at 1, press Down; then caret at 3, Down Down.
- Expected: carets on cluster boundaries only (VS Code and nvim never put one inside a cluster).
- Actual: 8, between `e` and U+0301; then 25, after the family's man, ZWJ and woman
  (`grapheme_up_down_can_land_inside_a_cluster`). Typing there splits the cluster (`eX́`, broken family emoji).
- Suspected code site: the same `EditorModel::vertical` (and `viewer.rs`): the char column is turned into an offset
  with no grapheme snapping.
- Severity: medium (silently corrupts text that's then typed in).

### G7: Column selection counts chars, not visible columns

- Repro (keyboard): buffer `ab一二cd\nabcdefgh`, caret at 1, `cmd-shift-alt-right` twice then `cmd-shift-alt-down`.
  Actual: `b一` above and `bc` below (`cjk_keyboard_column_select_counts_chars`); expected (VS Code): `bcd` below,
  visible columns 1 to 3.
- Repro (mouse): buffer `abcdefgh\nab一二cd`, alt-shift-drag from column 1 of row 0 to the x of its column 5 on
  row 1. Actual: `bcd` and `b一二`, right edges a third of a column apart (`cjk_alt_shift_drag_selects_a_box_of_chars`);
  expected: a box of the same visible columns on both rows.
- Suspected code site: `crates/fe-editor/src/column_select.rs` (its doc says "char column"), `EditorModel::select_box`
  (`crates/fe-editor/src/multi_cursor.rs:431`), fe-gpui `EditorView::box_point_at` (`crates/fe-gpui/src/view.rs:1588`)
  and `FrameLayout::display_column_at` (element.rs:204), which return `char_column`s.
- Severity: low.

### G8: A wide char is about 1.67 columns, not 2

- Repro: default font (JetBrains Mono, 13 px: space 7.8 px), PingFang's `一` is 13 px, 1.67 spaces.
  1. Soft wrap at `wrap_column` 20 (`wrap_mode = "column"`) over 30 CJK chars: 11 to a row
     (`cjk_soft_wrap_at_a_column_counts_the_shaped_width`); VS Code and nvim fit 10 (a wide char is 2 columns).
  2. Vim, buffer `abcd\n一二三`, `<C-v>lllj`: the block is display columns 1 to 4 on both rows (`abcd`, `一二`), but
     their right edges are 5.2 px apart (`cjk_vim_visual_block_edges_over_wide_chars_do_not_line_up`); in nvim the
     cells line up.
  3. Rulers at column N fall mid-char over CJK, and columns don't line up across rows mixing ASCII and CJK.
- Suspected code site: no cell grid is applied to fallback glyphs: `crates/fe-gpui/src/element.rs` `Shaper::shape`
  and `GlyphWidths` take the shaped advance; `EditorView::wrap_limit` (`crates/fe-gpui/src/view.rs:838`) is
  `wrap_column` times a space's width.
- Severity: low (cosmetic, but it is what makes G5 and G7 visible, and vim block edits look wrong).

### G9: Backspace on a combining sequence removes the whole cluster

- Repro: buffer `e\u{301}`, caret at 3, Backspace.
- Expected (VS Code): only U+0301 goes, leaving `e`; VS Code takes a whole ZWJ emoji, as here. nvim's insert-mode
  `<BS>` takes the whole char unless `'delcombine'` is set, as here.
- Actual: the buffer is empty (`grapheme_backspace_and_delete_remove_whole_clusters`). Same for Hangul jamo (the
  whole syllable goes) and the Devanagari conjunct.
- Suspected code site: `crates/fe-editor/src/lib.rs:1363` `EditorModel::backspace` (`prev_grapheme`).
- Severity: low; a design choice to confirm rather than a bug (fixing a typo'd accent means retyping the letter).

### G10: The editor has no font fallback list

- Repro: `EditorTheme::default_dark(cx).font().fallbacks` is `None`; `[editor] font_fallbacks = ["Sarasa Mono SC"]`
  is not a setting (`fallback_the_editor_font_names_no_fallbacks`). `[terminal] font_fallbacks` exists and the
  terminal view honours it.
- Expected: a user can name the fonts for chars the editor font lacks (a CJK font of a matching width, Nerd Font
  symbols), as in the terminal, VS Code's `editor.fontFamily` list, or Zed's `buffer_font_fallbacks`.
- Actual: CoreText's system cascade picks (PingFang, Apple Color Emoji); it works, but can't be changed.
- Suspected code site: `crates/fe-gpui/src/lib.rs:274` `EditorTheme::font` (sets no `fallbacks`);
  `crates/fe-config/src/settings.rs` `EditorSettings` (no field); compare `TerminalSettings::font_fallbacks` (:226).
- Severity: low.

### G11: No vim option affects display width

- Repro: `:set ambiwidth=double` or `:set display=uhex`.
- Expected (nvim): `ambiwidth` makes East Asian Ambiguous chars (`○`, `→`, Greek and Cyrillic in CJK fonts) two
  cells; `display` changes how unprintable chars show.
- Actual: both are unknown options (`crates/fe-vim/src/ex/set.rs` `option` knows 12 options); widths come from the
  fixed `crates/fe-vim/src/chars.txt` (ambiguous = 1), and the view draws by shaped width anyway.
- Suspected code site: `crates/fe-vim/src/ex/set.rs` `option`; `crates/fe-vim/src/motion.rs` `char_info`.
- Severity: low; no card proposed (`ambiwidth=double` only makes sense once wide chars are drawn two cells wide,
  card 4, and nobody has asked for it).

### Bidi fix: where it belongs and what it costs

Caret x and hit-testing (G1, G2) can be fixed in fe-gpui without patching GPUI: `Line` (element.rs) already wraps the
`LineLayout` and reads its glyphs (`with_glyphs_moved`), so its `MeasuredLine::x_for_index` and `index_for_x` can do
their own maths over the glyph positions instead of calling GPUI's. They need each glyph's direction, which GPUI
drops (CoreText's `kCTRunStatusRightToLeft` isn't kept). Two ways:

- Infer it from the glyphs: within a run, indices descending as x rises are RTL. Free, but ambiguous for a one-glyph
  run, and it breaks if GPUI ever stops merging runs.
- Run `unicode-bidi` (MIT/Apache-2.0, 0.3.18, already in Cargo.lock through cosmic-text, gpui-pre-wgpu and usvg, so a
  new direct dependency but no new crate) over the row's text for its embedding levels: exact, the same algorithm
  CoreText applies, and the same on Linux. It costs one pass over each shaped row with any char of a strong RTL class,
  skipped for rows with none (a byte scan).

Recommended: unicode-bidi levels in fe-gpui's `Line`, computed only for rows with RTL chars; a caret at a direction
boundary goes on the edge of the run its logical predecessor is in (Chromium's and VS Code's affinity). Selection
rects (G3) need fe-display's rect loop to ask the line for visual pieces of a range (a new `MeasuredLine` method
with a default of the one rect, so the test lines in fe-display stay as they are). A GPUI patch (fix
`line_layout.rs`) would fix every GPUI text field too, but costs a vendored gpui-pre fork for three functions, kept
across every gpui-pre bump; not worth it for the editor alone.

## Proposed cards

1. **Bidi-aware carets and hit-testing in RTL runs** (G1, G2)
   - Type: bugfix. Size: l.
   - Scope: `crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/element/**`, `crates/fe-gpui/Cargo.toml`,
     `Cargo.toml`, `crates/fe-gpui/tests/non_latin_text.rs`.
   - Dependencies: none. Mac-only to verify (Linux shaper unverified; a follow-up check on the Linux box).
   - Regression test: `rtl_carets_inside_an_rtl_span_share_one_x` becomes `rtl_caret_x_moves_right_to_left_through_an_rtl_span`
     (each Right in the Hebrew moves the caret left) and `rtl_click_on_the_last_painted_rtl_glyph_lands_at_the_row_end`
     becomes `rtl_click_on_an_rtl_glyph_lands_on_its_nearer_logical_edge`.
2. **Selection rects follow bidi runs** (G3)
   - Type: bugfix. Size: m.
   - Scope: `crates/fe-display/src/layout.rs`, `crates/fe-display/src/layout/**`, `crates/fe-gpui/src/element.rs`,
     `crates/fe-gpui/tests/non_latin_text.rs`.
   - Dependencies: card 1. Mac-only to verify.
   - Regression test: `rtl_selection_across_a_direction_change_draws_one_rect_per_visual_piece` (the frame's
     selection rects for 5..20 of the G1 buffer cover exactly the selected glyphs' x ranges), in fe-display with a
     scripted RTL `MeasuredLine` so it runs anywhere.
3. **Show invisible and bidi control characters** (G4)
   - Type: feature. Size: m.
   - Scope: `crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/element/**`, `crates/fe-config/src/settings.rs`,
     `crates/fe-config/tests/**`, `crates/fe-gpui/tests/non_latin_text.rs`.
   - Dependencies: none. Linux-OK.
   - Shape: draw U+202A..U+202E, U+2066..U+2069, U+200B..U+200F, U+2060, U+FEFF as a boxed `<202e>`-style label
     (an inline hint, as vim's `ga` names it), behind `editor.unicode_highlight` (on by default, as VS Code's).
     Confusables are left to a later card.
   - Regression test: `rtl_bidi_controls_are_drawn_as_labels` (the frame has one label per control, and the caret
     steps over each).
4. **Wide chars drawn two cells wide** (G8)
   - Type: feature. Size: m.
   - Scope: `crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/element/**`, `crates/fe-gpui/tests/non_latin_text.rs`.
   - Dependencies: none. Mac-only to verify (fallback fonts differ by OS).
   - Shape: as terminals and nvim's GUIs do, centre each East Asian Wide/Fullwidth glyph (`unicode-width` 2, already a
     dependency of fe-workbench) in exactly two space advances, through `Line::with_glyphs_moved` and `GlyphWidths`.
     Needs a user call: VS Code doesn't, so text looks slightly looser than there.
   - Regression test: `cjk_wide_chars_are_two_columns` (caret x after `一` is two spaces) turning
     `cjk_soft_wrap_at_a_column_counts_the_shaped_width` (10 a row) and
     `cjk_vim_visual_block_edges_over_wide_chars_do_not_line_up` (edges equal) around.
5. **Up/Down by visible column, on grapheme boundaries** (G5, G6)
   - Type: bugfix. Size: s.
   - Scope: `crates/fe-editor/src/lib.rs`, `crates/fe-editor/src/viewer.rs`, `crates/fe-editor/src/*_tests.rs`,
     `crates/fe-editor/Cargo.toml`, `crates/fe-gpui/tests/non_latin_text.rs`.
   - Dependencies: none (card 4 makes it look right, not work right). Linux-OK.
   - Shape: the goal is a visible column, a wide char counting two (East Asian Width, as VS Code's
     `CursorColumns`; fe-vim's `chars.txt` table or `unicode-width`), and the target offset is snapped back to a
     grapheme start. The G6 half is a one-line
     snap (`self.prev_grapheme(offset + 1)`-style clip) if the visible-column half is deferred.
   - Regression test: `grapheme_up_down_can_land_inside_a_cluster` turned around
     (`grapheme_up_down_land_on_cluster_boundaries`) and `cjk_up_down_keep_the_visible_column`.
6. **Column selection by visible column** (G7)
   - Type: bugfix. Size: s.
   - Scope: `crates/fe-editor/src/column_select.rs`, `crates/fe-editor/src/multi_cursor.rs`,
     `crates/fe-editor/src/*_tests.rs`, `crates/fe-gpui/src/view.rs`, `crates/fe-gpui/src/element.rs`,
     `crates/fe-gpui/tests/non_latin_text.rs`.
   - Dependencies: card 5 (shares the visible-column helper). Linux-OK.
   - Regression test: `cjk_keyboard_column_select_counts_visible_columns` (`bcd` below `b一`) and the alt-shift-drag
     test turned around.
7. **`editor.font_fallbacks`** (G10)
   - Type: feature. Size: s.
   - Scope: `crates/fe-config/src/settings.rs`, `crates/fe-config/tests/**`, `crates/fe-gpui/src/lib.rs`,
     `crates/fe-gpui/src/*_tests.rs`, `crates/fe-workbench/src/**`, `crates/fe-gpui/tests/non_latin_text.rs`.
   - Dependencies: none. Linux-OK.
   - Shape: as `terminal.font_fallbacks`: `EditorTheme::font` sets `FontFallbacks`, reloaded with the font.
   - Regression test: `fallback_the_editor_font_carries_the_settings_fallbacks`, replacing
     `fallback_the_editor_font_names_no_fallbacks`.

Not carded: G9 (keep whole-cluster Backspace, as nvim's default; if the user wants VS Code's behaviour it is an xs
change to `EditorModel::backspace`, taking one char when the cluster's last char is a combining mark, with
`grapheme_backspace_and_delete_remove_whole_clusters` turned around for `e\u{301}`), and G11 (see above).
