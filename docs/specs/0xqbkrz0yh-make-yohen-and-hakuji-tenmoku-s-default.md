---
task_uid: "01m6vvryyees36st0xqbkrz0yh"
task: "T1514"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test unset_slots_are_yohen_and_hakuji (cargo nextest run -p fastedit, crates/fastedit/src/theme.rs tests) passes: with no theme settings the UI tokens and the editor palette are Yohen's in dark mode and Hakuji's in light mode; palette_follows_kiln updated"
    required: true
    manual: false
  - id: "C2"
    text: "themes::find(\"dark\") and find(\"light\") return Yohen and Hakuji (crates/fe-config/tests/bundled_themes.rs); Tenmoku Dark, Light and Ember are still bundled and selectable"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-config -p fe-workbench -p fastedit passes"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (the 28 goldens of fe-gpui, fe-workbench, fe-chat-view and fe-markdown-view; cargo run -p \u003ccrate\u003e --example screenshots matches for each of the four)"
    required: true
    manual: false
---

# Make Yohen and Hakuji Tenmoku's default dark and light themes

# Make Yohen and Hakuji Tenmoku's default themes

Spec: `docs/design/design-system.md#colour`. Today, with no theme set, the UI
is Kiln Dark/Light (kiln's bases) and the editor palette is
`EditorPalette::fastedit_dark()` / `fastedit_light()` from `tenmoku-dark.toml`
and `tenmoku-light.toml`.

## Change

- Tenmoku's unset `theme.dark` / `theme.light` slots mean Yohen / Hakuji, for
  both the UI tokens and the editor palette: `crates/fastedit/src/theme.rs`
  (`select`, `choice`), `crates/fe-config/src/theme.rs`
  (`EditorPalette::bundled`, `fastedit_dark` / `fastedit_light` now from
  `yohen.toml` / `hakuji.toml`). Kiln's own bases (Kiln Dark/Light) stay
  kiln's defaults for other kiln apps.
- `themes::find("dark")` / `find("light")` give Yohen / Hakuji
  (`crates/fe-config/src/themes.rs`).
- The "own" theme ids: `crates/fe-workbench/src/theme_picker.rs` (`showing`),
  `crates/fe-workbench/src/setup_wizard_model.rs` (`own_theme_id`).
- Tenmoku Dark, Light and Ember stay bundled and selectable (whether to retire
  them is the user's call, not this card's).
- Doc comments and the template that name the default theme
  (`crates/fe-config/src/settings.rs` `ThemeSettings`,
  `settings.schema.json`, `settings-template.toml`).
- Tests that pin today's defaults move to Yohen/Hakuji:
  `crates/fe-config/tests/editor_palette.rs`, `bundled_themes.rs`,
  `vscode_2026_dark_theme.rs`, `vscode_2026_light_theme.rs`,
  `monokai_theme.rs`, and `crates/fastedit/src/theme.rs`
  `palette_follows_kiln`. Tests that `include_str!` tenmoku-dark/light for
  their own keys keep doing so.
- Screenshot scenes: the fe-workbench scenes that name `"tenmoku-dark"` /
  `"tenmoku-light"` (`setup_scene.rs`, `icons_scene.rs`) and build the palette
  (`scenes.rs`, `token_probe.rs`) use the defaults; the fe-chat-view and
  fe-markdown-view scenes use the Yohen/Hakuji UI tokens instead of
  `UiTheme::default_dark()` / `Theme::light()`. Regenerate every golden these
  render (28: fe-gpui 17, fe-workbench 5, fe-chat-view 4, fe-markdown-view 2)
  on the Mac with `UPDATE_SCREENSHOTS=1`, and look at each. Kiln's gallery
  keeps Kiln Dark/Light.

Platform: **Mac-only** (goldens).

## Lead ruling (rev 2)

The worker's scope-gap block found 29 out-of-scope failures. Scope widens to them (list A, C, D), with these rulings:

- **B1, terminal ANSI: option (a).** A theme without a `[terminal]` table inherits the default palette's ANSI sets like every other key, so Tenmoku Ember and the non-Tenmoku themes without the table now get Yohen's or Hakuji's. Retarget `crates/fe-config/tests/terminal_ansi_theme.rs` (`a_theme_without_the_table_keeps_todays_sets` and the `assert_ne` against the bundled sets) to that inheritance.
- **B2, opaque diff rows.** In `crates/fe-gpui/src/diff_view_words_tests.rs`, `a_changed_word_keeps_its_text_readable` keeps its WCAG contrast check, and its alpha cap (<= 0.2) applies only when the row colour is translucent. Opaque rows such as Yohen's and Hakuji's `diff_*_row` are checked by contrast alone. Retarget `theme_colours` to the new defaults.
- **A, tests that pin the old defaults:** retarget mechanically to Yohen/Hakuji, or read tenmoku-*.toml through `for_theme` where the test means that theme. Don't weaken any assertion beyond the new values.
- **C:** `fe-config.workspace = true` under `[dev-dependencies]` in `crates/fe-chat-view/Cargo.toml` and `crates/fe-markdown-view/Cargo.toml`, for the scenes only.
- **D:** fix the stale doc lines in `crates/fe-gpui/src/lib.rs` (EditorTheme::default_dark is Yohen), and `APPEARANCES` in `crates/fe-workbench/examples/screenshots/audit_scenes.rs` names the new defaults.

## Lead ruling (rev 3)

After rev 2, `a_changed_word_keeps_its_text_readable` fails only on its comment rule: a comment keeps >= 65% of its bare-background contrast on a changed word. Yohen added is 54%, Yohen removed 63% and Hakuji removed 59%. **Option (a):** keep the rule and its test unchanged, and retune `diff_added_word` / `diff_removed_word` in `crates/fe-config/themes/yohen.toml` and `hakuji.toml`. Lower only the alpha of today's signal hex (now `33`), per key, to the highest value at which the rule holds. The word must still read stronger than its opaque row. The design doc pins no word tint ("stronger tints on the exact words"), so `docs/design/design-system.md` doesn't change. Record the chosen alphas and each comment ratio in the decision file. If `yohen_hakuji_values.rs` or `yohen_hakuji_contrast.rs` pin the word tints, update them (in scope). Regenerate the diff goldens with the rest.
