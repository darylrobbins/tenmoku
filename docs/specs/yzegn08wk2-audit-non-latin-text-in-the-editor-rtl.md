---
task_uid: "01m6smzbtmvdxbhqyzegn08wk2"
task: "T1431"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-gpui --test non_latin_text passes, with at least one test whose name starts with each of rtl_, cjk_, grapheme_, ime_ and fallback_ (bare in-binary names, e.g. rtl_logical_caret_steps_one_grapheme)"
    required: true
    manual: false
  - id: "C2"
    text: "Every test in crates/fe-gpui/tests/non_latin_text.rs has a doc comment naming the behaviour it pins; any test pinning a limitation names the doc's gap id (G\u003cn\u003e)"
    required: true
    manual: false
  - id: "C3"
    text: "docs/decisions/\u003clane\u003e-non-latin-text-audit.md exists with sections Status, What was checked, Findings and Proposed cards; each finding G\u003cn\u003e has a minimal repro (buffer text, keys or API calls, expected vs actual), a suspected code site and a severity; each proposed card has a title, type, size, scope globs, dependencies, Linux-OK or Mac-only and the regression test it would add"
    required: true
    manual: false
  - id: "C4"
    text: "The Findings section states whether bidi display, caret placement and hit-testing work for RTL runs, citing the code that decides it (fe-gpui and gpui-pre line_layout.rs index_for_x/x_for_index), and proposes rather than implements any fix"
    required: true
    manual: false
  - id: "C5"
    text: "git diff --stat against the base lists only crates/fe-gpui/tests/non_latin_text.rs and the doc; no rgt task was filed by the card"
    required: true
    manual: false
  - id: "C6"
    text: "rgt ci run --fast passes"
    required: true
    manual: false
---

# Audit non-Latin text in the editor (RTL and mixed direction, full-width CJK, grapheme clusters, IME, font fallback): lock in what works with tests and write up each gap with a repro

Audit how the editor handles non-Latin text, lock in what already works with tests in ONE new integration test file, and write up every gap with a repro so the lead can file bugfix cards. User-approved 2026-11-06. This card files nothing itself and fixes nothing: its scope holds no source file.

## What already exists (do not redo it; read it first)

- Grapheme handling: fe-editor `prev_grapheme`/`next_grapheme` (crates/fe-editor/src/transpose.rs, subword.rs `whole_graphemes`); fe-gpui element.rs `grapheme_start`, `next_grapheme`, `block_rect` (vim's block caret covers one grapheme); hit-testing goes to a grapheme start (element.rs ~line 181, view.rs ~1604 "on the grapheme"); fe-gpui src/vim/tests.rs line ~625 (e + combining accent).
- Widths: `fe_display::CharWidth` with `cluster_width` (crates/fe-display/src/lib.rs); fe-gpui `GlyphWidths` (element.rs ~545) shapes each char and multi-char cluster once, so fallback fonts and ZWJ emoji are measured as drawn. fe-display layout tests use CJK/emoji two cells wide (crates/fe-display/src/layout/tests.rs, long_row_tests.rs); the minimap draws CJK two columns (minimap.rs `is_wide`).
- Vim: crates/fe-vim/src/chars.txt is nvim's `charclass()`/`strdisplaywidth()` table (motion.rs `char_info`, `VS16_WIDENED`), and the nvim oracle corpora already cover CJK, combining and emoji in crates/fe-vim/tests/corpus/{visual_block,virtual_replace,motions,composing,replace,line_motions,...}.toml (visual_block.toml alone has 16 CJK cases; motions.toml has Arabic-Indic digits). Do NOT add fe-vim-engine tests that the oracle already covers; test the editor view's side of vim (block caret width, `gj`/`gk` across wrapped wide rows, visual block drawn over wide chars).
- IME: crates/fe-gpui/src/view/ime.rs with ~30 tests in crates/fe-gpui/src/view/ime_tests.rs: composing leaves the buffer, commit types once at every caret (multi-cursor, `commit_types_once`), painted as an inlay, geometry, caret in view, long rows, UTF-16 offsets with emoji, vim turning on mid-composition; vim insert IME in src/vim/insert_tests.rs and macros in src/vim/macro_tests.rs. T436 (human, inbox) checks Japanese IME in the running app. Only add IME tests for gaps these leave (for example a composition in vim replace mode `R`, in visual-block insert `<C-v>jjI`, or with carets on rows of different widths).
- Fonts: `editor.font_fallbacks` (crates/fe-config/src/settings.rs ~226); the terminal view tests its fallbacks (crates/fe-terminal-view/tests/font_fallbacks.rs), the editor view does not appear to.
- Bidi: nothing in fastedit. GPUI's macOS shaper (gpui-pre-apple 0.3.9 text_system.rs `layout_line`) builds a CTLine, so CoreText reorders RTL runs visually and glyphs keep their logical byte `index`; but gpui-pre 0.3.9 text_system/line_layout.rs `index_for_x`, `closest_index_for_x` and `x_for_index` scan glyphs in visual order assuming byte order (only `ShapedLineCursor` admits "bidirectional shaping can put glyphs out of byte order"). Expect caret x, selection rects and click hit-testing to be wrong inside RTL runs. Establish what fe-gpui itself uses (it may place carets from its own maths) before concluding.
- Columns: rulers sit at N space advances (crates/fe-gpui/src/element/rulers.rs); keyboard column select counts char columns (crates/fe-editor/src/column_select.rs), while VS Code counts a full-width char as two visible columns; the Problems panel already has a display-width column unit (crates/fe-workbench/src/problems_panel.rs, unicode-width).

## Test harness facts

- Host: crates/fe-gpui. Its tests/ dir holds separate binaries, one file each (ai_demo_host.rs, bracket_colors.rs, keystroke_latency.rs, which_key_hold.rs), no tests/it.rs and no mod.rs. So the new file is its own binary: crates/fe-gpui/tests/non_latin_text.rs. Copy the setup from tests/which_key_hold.rs (`fe_gpui::init`, `EditorView::new(EditorModel::new(Buffer::new(..), None), EditorTheme::default_dark(cx), cx)`, `set_vim_enabled`). Public API only: an integration test cannot reach crate-private helpers; if a probe needs one, record that in the doc rather than widen the crate.
- `#[gpui::test]` uses GPUI's `NoopTextSystem`: every char one em wide, no shaping, no reordering, no fallback. CJK, emoji and RTL look like ASCII to it. For anything about shaped widths, fallback or bidi, build the app by hand with the real shaper: `TestAppContext::build_with_text_system(TestDispatcher::new(0), Some(name), gpui_platform::current_platform(true).text_system())` (gpui_platform with test-support is already a dev-dependency; the benches get the real text system this way: crates/fe-gpui/benches/keystroke/main.rs line ~38). Shut the app down as crates/fe-chat-view/tests/markdown.rs `with_app` does. Gate real-shaper tests with `#[cfg(target_os = "macos")]` (macOS ships PingFang, Apple Color Emoji, Geeza Pro and Arial Hebrew), and say why in a comment. No dependency or Cargo.toml change.
- Keep it lean (fast tests, small buffers; nothing over a second each). No screenshots or goldens.

## Areas to probe (each: test what works, write up what doesn't)

1. `rtl_*` Right-to-left and mixed-direction text: an Arabic and a Hebrew string literal and comment in a line of Rust. Rendering order (real shaper: glyph x order vs byte order), caret x at each grapheme boundary, Left/Right arrow (logical or visual, and what VS Code does: logical), shift-selection rects, click hit-testing on an RTL glyph, double-click word select, End/Home. At minimum lock in that text round-trips, logical caret movement steps one grapheme, and nothing panics laying out or hit-testing a mixed row. If bidi display is absent or wrong, say so with the GPUI line_layout.rs cause and propose (do not implement) a fix: where it belongs (fe-gpui vs a GPUI patch), unicode-bidi (MIT/Apache-2.0) vs CoreText runs, and the cost.
2. `cjk_*` Full-width CJK: wrap at a column (does a wide char count as its shaped width; does a wrap ever split a cluster), rulers vs wide chars, keyboard column select and alt-drag box select over rows mixing ASCII and CJK (char columns vs visible columns; compare VS Code), up/down arrow keeping the visual x goal over wide rows, vim's block caret width over a wide char, `gj`/`gk` across a soft-wrapped CJK row, vim `|` and visual block drawn over wide chars (the engine's columns are oracle-tested; test the drawn rects). Note whether any vim `:set` option affects display width here (nvim's `ambiwidth`, `display`): supported, ignored or absent.
3. `grapheme_*` Clusters: family ZWJ emoji, flags (regional indicator pairs), keycaps, e + U+0301 and Hangul jamo, a Devanagari conjunct. Vim off: one Left/Right step per cluster, Backspace and Delete behaviour (record what VS Code does: Backspace on e+U+0301 removes the accent only; on a ZWJ family emoji removes the whole cluster), double-click, clicks inside a cluster land on its start. Vim on: `x`, `l`, `h`, `r`, `~` on a cluster through the view (keystrokes), block caret width = the cluster's shaped width.
4. `ime_*` Only the gaps the existing ime_tests leave (see above), driven through the public `EntityInputHandler` on EditorView.
5. `fallback_*` Font fallback: with the real shaper and an editor font lacking CJK/emoji (the default), CJK and emoji draw from a fallback font and are measured by it (GlyphWidths); `editor.font_fallbacks` order is honoured by the editor view, or record that it is not wired there.

Also note, in the findings, Unicode hazards a code editor should show (Trojan Source bidi controls U+202A..U+202E, U+2066..U+2069; zero-width and confusable chars): whether Tenmoku makes them visible as VS Code's `editor.unicodeHighlight` does. Findings only.

## Deliverables

1. crates/fe-gpui/tests/non_latin_text.rs: passing tests that lock in what works, named with the area prefixes above (`rtl_`, `cjk_`, `grapheme_`, `ime_`, `fallback_`), at least one per area. Each test's doc comment says which behaviour it pins and, if it pins a limitation rather than a target, says so and names the doc's gap id. Do not write a test that asserts a known-wrong behaviour as correct without that note.
2. docs/decisions/<lane>-non-latin-text-audit.md (same `<lane>-` prefix convention as the other files there) with: a status line naming the user as approver for the follow-ups; "What was checked" (per area: what passes, by test name); "Findings": one entry per gap, id G1, G2, ..., each with a minimal repro (buffer text with escapes, keys or API calls, expected vs actual, the VS Code and nvim behaviour where they differ), the suspected code site (file and function), and a severity; "Proposed cards": one per fix or per group of related fixes, each with a title, type (bugfix/feature), size xs..xl, scope globs, dependencies, Linux-OK or Mac-only, and the regression test it would add. One-line fixes are listed there too: this card's scope has no source files.
3. No other file changes. Do not touch crates/*/src/lib.rs, Cargo.toml, TESTING.md or any src file.

## Out of scope

Fixing any gap, adding bidi support, new settings, Cargo changes, fe-vim engine tests the oracle already covers, filing cards (the lead files them from the doc).
