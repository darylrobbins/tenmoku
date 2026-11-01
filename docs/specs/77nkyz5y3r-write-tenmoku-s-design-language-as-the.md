---
task_uid: "01m6c4e8fm9srcc877nkyz5y3r"
task: "T1273"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "docs/design-language.md has sections Colour tokens, Spacing, Type, Icons, Motion and Gaps, each rule citing the path and symbol it comes from"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-config --test design_language_doc passes: every_token_group_has_a_section finds a ### heading for each kiln_core::theme::TOKEN_GROUPS entry"
    required: true
    manual: false
  - id: "C3"
    text: "On the Mac, cargo run -p fe-workbench --example screenshots passes, including the token probe: it renders the workbench scene under two probe themes, prints the count of identical pixels (at most ALLOWED_FIXED_PIXELS, each region named in a comment and under Gaps) and writes target/screenshots/workbench_tokens.diff.png; the output is in the decision file"
    required: true
    manual: false
  - id: "C4"
    text: "git diff --stat against the base lists no .png and only the five scope paths"
    required: true
    manual: false
---

# Write Tenmoku's design language as the code has it today (colour tokens, spacing, type, icons, motion, gaps) and fail the workbench screenshot run when a new colour bypasses the theme

## Problem

Tenmoku's direction (user, 2026-11-01): "native, Apache-2.0 code editor, curated JetBrains-lite, beautiful,
lightweight and measured". Nothing written says what "beautiful" means in this codebase. The visual rules
exist only as code, scattered:

- Colour: kiln's semantic tokens, `kiln/crates/kiln-core/src/theme/tokens.rs` (`tokens!` groups surface, text,
  border, accent, state, status, list, …; `TOKEN_GROUPS`, `TOKEN_KEYS`), resolved for GPUI in
  `kiln/crates/kiln-ui/src/theme.rs` (`Theme`, `UiTheme`); the editor and syntax colours in the bundled themes'
  `[editor]` and `[syntax]` tables, `crates/fe-config/themes/tenmoku-dark.toml`, `tenmoku-light.toml`,
  `tenmoku-ember.toml`.
- Spacing: no scale. Pixel constants per widget: `ROW_HEIGHT` (24 px, `kiln/crates/kiln-ui/src/theme.rs:22`),
  13 in `crates/fe-workbench/src/settings_editor_layout.rs`, 9 each in `kiln/crates/kiln-ui/src/tree_row.rs`
  and `crates/fe-workbench/src/git_log.rs`, and more in `peek.rs`, `shortcuts_editor.rs`, `dock.rs`,
  `pane_group.rs`, `activity_rail.rs`, `fe-markdown-view/src/style.rs`.
- Type: GPUI text-size helpers and literal sizes per view; the editor font from settings.
- Icons: Lucide (`kiln/crates/kiln-ui/assets/icons/`), file icons (`kiln/crates/kiln-ui/src/file_icons.rs`).
- Motion: whatever animations exist (`rg with_animation`), no stated durations or easing.

Nothing checks that a view takes its colours from the theme rather than a literal.

## Change

1. New `docs/design-language.md`, describing what the code does today (not an aspiration), each rule citing
   `path:symbol` for where it lives:
   - `## Colour tokens`: one `### <group>` subsection per name in `kiln_core::theme::TOKEN_GROUPS`, listing its
     tokens and what each is for (from the doc comments in `tokens.rs`), then the editor and syntax tables of
     the Tenmoku themes and how they relate to kiln's tokens (which wins where, what a theme may leave out).
   - `## Spacing`: the constants in use with their values, grouped by role (row heights, paddings, gaps,
     radii), and the de-facto scale they imply.
   - `## Type`: the sizes and weights in use per role (editor text, chrome labels, headings in Markdown
     preview, status bar, palette), and the fonts.
   - `## Icons`: the sets, sizes in use, how they are tinted (which token), and the licence pointer.
   - `## Motion`: every animation found, its duration and easing; "none" where none exists.
   - `## Gaps`: each inconsistency found (two paddings for one role, a hard-coded colour, a size used once),
     one line each with its file, so later cards can pick them up. This card does not fix them.
2. A token-probe check in the workbench screenshot example (`crates/fe-workbench/examples/screenshots/`):
   new `token_probe.rs` (declared from `main.rs`), run by `cargo run -p fe-workbench --example screenshots`
   after the golden scene. It renders the existing `workbench` scene twice under two probe themes: every kiln
   token (`Tokens::map`) and every colour of the `EditorTheme` the scene passes in replaced by a distinct
   colour, theme A and theme B differing in every token. A pixel that is identical (within the harness's
   ±2/255) in both captures did not come from the theme: a hard-coded colour. The check writes
   `target/screenshots/workbench_tokens.diff.png` (identical pixels in magenta over the dimmed A capture),
   prints the count and the bounding boxes of the identical regions, and fails when the count exceeds
   `ALLOWED_FIXED_PIXELS`, a constant set to what this card measures, with a comment naming each region's
   source (each also listed under `## Gaps`). So the check ratchets: a new hard-coded colour fails it. No new
   golden PNG. `scenes.rs` gains only the parameter it needs to take a theme. Like the other scenes it renders
   only on macOS under `cargo run`; elsewhere it prints `skipped`.
3. New `crates/fe-config/tests/design_language_doc.rs`, `every_token_group_has_a_section`: reads
   `docs/design-language.md` (from `CARGO_MANIFEST_DIR/../../docs`) and asserts a `### <group>` heading for every
   `kiln_core::theme::TOKEN_GROUPS` entry, and that the six `##` sections exist; so a new token group without
   documentation fails.

Out of scope: TESTING.md (held by T1235; the example's module doc and the design doc describe the probe),
anything under `kiln/` (T1268 reserves `kiln/**`), fixing any gap.
