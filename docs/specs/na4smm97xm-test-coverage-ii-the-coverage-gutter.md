---
task_uid: "01m7th4h2bmmarzvna4smm97xm"
task: "T701"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-gpui/tests/coverage_gutter.rs `coverage_marks_draw_in_the_lane_beside_the_change_bar`: with a decoration provider giving GutterMark::Coverage marks, drawn_gutter lists covered, uncovered and partial per row, right of and not overlapping the change bar at em 13 and 20 px, with partial a different shape from the other two; no fe-display or gutter width change (existing gutter_b3 tests pass unchanged)."
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-gpui/tests/coverage_gutter.rs `coverage_marks_do_not_hide_or_move_the_change_bar_or_agent_lane`: a row with a git bar and a coverage mark draws both; the dotted agent lane and deletion notches draw as before."
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-config/tests/coverage_colors.rs `coverage_keys_set_in_every_bundled_theme_with_contrast`: coverage_covered/uncovered/partial exist in every bundled theme (explicit in the six that spell out all keys, fallbacks otherwise as DEBUG_FALLBACKS does), are \u003e= 3:1 against the editor background, and covered differs from the git added colour."
    required: true
    manual: false
  - id: "C4"
    text: "The existing design_role_keys, design_role_tokens, minimap_theme_keys and fastedit theme tests pass with the new keys, and `python3 scripts/check_kiln_boundary.py` is clean."
    required: true
    manual: false
---

# Test coverage II: the coverage gutter mark (covered, uncovered, partial) beside the git change bar, and its theme colours (Mac T1983)

# Test coverage II: the coverage gutter mark and its theme colours (renderer only, driven by decorations)

Phase 2 of 3. Independent of cov1 (it only draws `GutterMark::Coverage` decorations that a host supplies; cov3 wires the report to them, with the toggle). Tests feed a `DecorationSet` provider as `element/change_bar_tests.rs` does.

## Gutter layout decision (survey result)
The change lane is one lane of `CHANGE_LANE_EMS` (1 em, `fe-display/src/layout.rs`), in which the change bars (`GutterMark::Bar`) use `change_bar_width(lane) = lane * GUTTER_BAR / (GUTTER_BAR + GUTTER_GAP)` and the rest is the gap before the text (`fe-gpui/src/element.rs`, `element/decorations.rs`). Do NOT change `fe-display` or widen the gutter: T1894 (inbox, breakpoint lane, gutter 81 to 97 px) and the gutter goldens and `drawn_gutter` tests pin the widths. Instead add `GutterMark::Coverage(CoverageMark)` (`Covered | Uncovered | Partial`) in `fe-editor/src/decorations.rs` drawn as a thin strip (about the bar's width, 2 px, snapped to device pixels like `bar`) at the lane's RIGHT edge, in the gap just beside the change bar, never over it; Partial is the same strip drawn as two stacked half-height dashes (shape, not only colour, so it is distinguishable without colour: see `docs/design/design-system.md`, colour-blind notes, `fe-config/tests/color_blind.rs`). Verify by measuring that the strip fits in the gap at the default font sizes (em 13 to 20 px) and does not overlap the bar; if the gap is too thin at small sizes, the strip may overlap the lane's right half only and the card says which in the hand-in. Add the kind to `DrawnGutter` (`drawn_gutter()` test API, `element.rs` around `marks_of`).

## Linux
Yes. Headless GPUI `drawn_gutter` assertions; add no new golden.

## Scope notes
The token files below are listed because past token cards hit them; if another file is needed, stop and ask.

## Theme tokens (gotchas from past token cards, do these all or the gate fails)
New `[editor]` keys: `coverage_covered`, `coverage_uncovered`, `coverage_partial` (`fe-config/src/theme.rs` `EditorColors` list near `agent_lane`, `EditorTheme` in `fe-gpui/src/lib.rs` and `fe-gpui/src/theme.rs`). Every bundled theme that spells out all keys sets them (tenmoku-dark, tenmoku-light, tenmoku-ember, vscode-monokai, yohen, hakuji; the others rely on a fallback list, model on `DEBUG_FALLBACKS` in `theme.rs` and `fe-config/tests/debug_tints.rs`: fallbacks for themes with their own `[editor]` table are the theme's added/removed/warning colours). Insert them before the minimap keys (`minimap_theme_keys` pins them last). Contrast: each at least 3:1 against the editor background in every bundled theme (check with a script before writing, as `debug_tints.rs` does) and distinguishable from the git bar colours (`added`/`modified`): covered is NOT the git green; pick from the design's roles (see `docs/design/design-system.md`) and state the choice. Also expect `fe-config/tests/design_role_keys.rs`, `design_role_tokens.rs`, `fastedit/src/theme.rs` tests (`palette_follows_kiln`) to need the new keys in their lists: they are in scope for this reason.

## Helium filing note

- Lent from the Mac as y151, the user's request (notebook cells NB1-NB3, test coverage COV1-COV3, reading mode).
- **Never touch crates/fe-workbench/src/lsp_consent*.rs** (Mac T1978 and T1701 own it).
- linux-ok: headless tests. The macOS gate runs when the batch lands on the Mac.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Golden and pin tests: a new setting changes the settings schema/template goldens, settings_model.rs and the
  l10n files; a new command changes typed_command_ids_tests.rs and command_catalog_tests.rs; a new binding can
  break binding-count tests in fe-gpui. The lead pre-added the ones it knew of. If another golden or pin
  fails, widen your scope with `rgt task respec --scope` (it replaces the list, so pass the whole list) before
  you finish, and say why in the spec's Implementation notes.
- Workbench-wide settings reach live state only through workbench_root.rs reload_settings; per-editor fields
  ride EditorView::set_wrap_settings/apply_settings. If you need a file that's out of scope for that, respec first.
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if anything in the criteria can't be met as written.
- The card names 6 of the 13 bundled themes. If a test checks every bundled theme for the new keys (minimap_theme_keys.rs does that for its keys), respec the other 7 theme files in.

## Implementation notes

- Design calls and measurements: `docs/decisions/na4smm97xm-test-coverage-ii-the-coverage-gutter-mar.md`.
- The strip fits in the lane's gap at em 13 and 20 (x 77–79 beside a bar at
  68–71 at 13 px; 120–123 beside 105–109.5 at 20 px): no overlap with the
  lane's right half was needed. It sits 2 px (`BAND_EDGE`) short of the code so
  the debugger band's edge doesn't cover it.
- Respecced in the other seven bundled themes (catppuccin ×4, vscode-2026
  dark/light, vscode-dark-modern) so every theme sets the keys explicitly;
  with the card's added-colour fallback they would have drawn covered in the
  git green, against C3. The fallback list (`COVERAGE_FALLBACKS`) still
  applies to user themes: covered falls back to `information`, not added.
- Respecced in `crates/fe-config/tests/editor_palette.rs`:
  `extension_tables_override` pins the palette of a theme with its own
  `[editor]` table, and the coverage fallbacks change it as the debugger's do.
- `design_role_keys.rs`, `design_role_tokens.rs` and `minimap_theme_keys.rs`
  needed no change: none lists every key, and the new keys sit before the
  minimap's.
