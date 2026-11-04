---
task_uid: "01m6hxen62ej92y5qhedgntp82"
task: "T1342"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "setup_wizard_preview_tests::tiles_use_each_themes_colours: with Tenmoku Dark active, every bundled theme's tile swatch colours equal that theme's own surface.panel, surface.background, accent.base and syntax keyword, string and function colours"
    required: true
    manual: false
  - id: "C2"
    text: "setup_wizard_preview_tests::sample_follows_settings: after choosing font size 16 and Light (settings reloaded), the sample EditorView has font size 16 and the light palette, is read-only and takes no focus on click"
    required: true
    manual: false
  - id: "C3"
    text: "setup_wizard_preview_tests::font_list_is_curated: the families listed are Default (\u003cfirst installed\u003e), then the installed MONO_FAMILIES, then a current editor.font_family not among them; never the full system list"
    required: true
    manual: false
  - id: "C4"
    text: "cargo run -p fe-workbench --example screenshots passes on the Mac with the new setup_wizard_dark and setup_wizard_light goldens (Appearance page over the demo workbench, 1000x600, font pinned to Menlo); golden/workbench.png is unchanged and the token probe still allows 0 fixed pixels"
    required: true
    manual: false
  - id: "C5"
    text: "rg -n 'rgb\\(|rgba\\(|hsla\\(|gpui::(white|black)\\(' crates/fe-workbench/src/setup_wizard_preview.rs finds nothing"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Draw the setup wizard's Appearance and Font pages: theme tiles painted in each theme's own colours, a live code sample in the chosen theme and font, and dark and light screenshot goldens

## Problem

The setup sheet's Appearance and Font pages show plain lists of names. These are the pages that sell the
editor on first launch ("beautiful" is a product goal), and the window behind the sheet is often empty
(no tabs since T1307), so the user can't see the font or the syntax colours they're choosing.

## Change

New `crates/fe-workbench/src/setup_wizard_preview.rs`, used by `crates/fe-workbench/src/setup_wizard.rs` for
the two pages' bodies; the rest of the sheet is unchanged.

- **Theme tiles.** Each bundled theme of the shown appearance is a tile (a grid, 3 or 4 across): a small
  window drawn in that theme's own colours, not the active theme's: its `surface.panel` sidebar, its
  `surface.background` / `[editor] background` text area, its `accent.base` as a tab underline, and three
  short "code" bars in its syntax `keyword`, `string` and `function` colours, with the title under the tile at
  12. Colours come from `BundledTheme::file()` (kiln's tokens resolved with their rules) and
  `EditorPalette::for_theme`, read once when the page opens. The chosen tile has a 2 px `border.focused`
  ring; the keyboard focus ring is `state.focus_ring`. Mode is three tiles too (Dark, Light, Match System;
  the last split diagonally between the chosen dark and light themes).
- **Live sample.** Beside the tiles on Appearance, and under the controls on Font, a read-only `EditorView`
  (as `peek.rs` and `git_commit.rs` embed one) over a fixed ~12-row Rust sample with a keyword, string,
  comment, number, function and type, highlighted; not focusable, no caret. It follows the effective theme
  and `editor.*` font settings as they reload (the write-through choices), the same way pane editors do on
  `Workbench::reload_settings`.
- **Font list:** "Default (<first installed of MONO_FAMILIES>)" first, then each installed family of
  `kiln::platform::fonts::MONO_FAMILIES` (`first_installed` per family), then the current
  `editor.font_family` if it's another; each label drawn in its own family. Not the full system font list.

## Screenshots

Add a setup scene to the workbench screenshots example: new
`crates/fe-workbench/examples/screenshots/setup_scene.rs`, wired from `main.rs` / `scenes.rs` there. The demo
workbench (as `capture` builds it, font pinned to Menlo) with the sheet on the Appearance page, at 1000×600,
dark (defaults) and light (`theme.mode = "light"`), goldens `golden/setup_wizard_dark.png` and
`golden/setup_wizard_light.png`. The existing `golden/workbench.png` and the token probe
(`ALLOWED_FIXED_PIXELS` stays 0) must not change. Per TESTING.md the scenes render only under `cargo run` on
macOS; the gate doesn't render them.

## Implementation notes

- **Following the window's font without touching `Workbench::reload_settings`.** The sample observes the
  active pane, which that reload notifies, and copies the metrics from `Pane::theme()`. That keeps a
  project's settings layer, and a font pinned as the screenshots pin Menlo, which the
  `GlobalSettings` global wouldn't. `workbench_root.rs` stays out of the change.
- **The sample starts one effect cycle late.** `Workbench::show_setup` builds the sheet while the workbench
  is being updated, so the sheet can't read it for its pane until `defer_in` runs, before the first frame.
- **The chosen themes of a hidden slot.** Mode's tiles show the chosen dark and light themes even when the
  mode hides one of the lists. They read a clone of `Choices` set to Match System, which lists both through
  the model's own rules for `theme.name` and unset slots rather than a second copy of them.
- **A theme that isn't bundled has no file to read.** Its tile is the active theme if that's of its
  appearance, else the bundled base of the appearance.
- **Carried T1341 findings.** Both are fixed, with their tests in `setup_wizard_preview_tests.rs`, since
  `setup_wizard_tests.rs` is outside this task's scope. The sheet detaches its last write when released, and
  each write also carries every key still on its way to the file, so a later write retires `theme.name` if
  the earlier one fails.
- **Headless font checks.** GPUI's no-op text system answers every family with one font ID, so
  `first_installed` depends on call order there. `font_list_is_curated` tests the filter with a fake
  predicate, and the sheet's list by its shape.
