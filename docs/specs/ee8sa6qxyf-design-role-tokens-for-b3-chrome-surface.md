---
task_uid: "01m6vvrq10ew0en2ee8sa6qxyf"
task: "T1509"
rev: 5
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test new_roles_resolve_to_todays_colours (cargo nextest run -p fe-config --test design_role_tokens) passes: for Kiln Dark, Kiln Light and every theme crates/fe-config bundles, each new token resolves by its rule or base value, and every pre-existing token resolves exactly as before this change. The test lives in crates/fe-config/tests/design_role_tokens.rs (kiln must not read fastedit files), and kiln/crates/kiln-core/tests/design_role_tokens.rs is deleted"
    required: true
    manual: false
  - id: "C2"
    text: "Test agent_never_resolves_to_the_accent (same binary) passes for every bundled theme"
    required: true
    manual: false
  - id: "C3"
    text: "Test tenmoku_palettes_set_the_new_editor_keys_to_todays_colours (cargo nextest run -p fe-config --test design_role_keys) passes, and editor_palette still passes"
    required: true
    manual: false
  - id: "C4"
    text: "crates/fe-config/tests/design_language_doc.rs passes with ### agent and ### vcs headings; no golden changes"
    required: true
    manual: false
---

# Design-role tokens for B3 (chrome surface, faint text, control border, modified and agent signals with soft grounds, VCS tokens, find-match and identifier-highlight keys), every existing theme unchanged

# Design-role tokens for B3, derived so every existing theme looks the same

Spec: `docs/design/design-system.md#colour` (Signals, Surfaces, the three text
steps) and `#editor` (find match, identifier highlight, agent lane). Audit
finding PT1/GP2 (VCS colours) from `docs/decisions/25w38k498b-ui-polish-audit.md`.

This is plumbing only: new token slots and editor keys, each with a rule or a
base value that reproduces today's colour, so **no pixel changes** and no
golden moves. Yohen and Hakuji (a later card) fill them with the design's values;
the surface cards read them.

## Change

kiln-core (`kiln/crates/kiln-core/src/theme/tokens.rs`, the `tokens!` table and
`RULES`):

- `surface.chrome` (title bar, rail, tab strip, status bar; rule: `surface.panel`).
  Today's `surface.panel` is the design's "tool window"; `surface.background`
  is "editor"; `surface.raised` is "overlay" (today's `surface.overlay` stays the
  modal scrim: say so in the doc comment, since the names cross).
- `text.faint` (rule: `text.muted` 40% toward the background).
- `border.control` (inputs, secondary buttons; rule: `border.default`).
- `status.modified` (Ruri; root in the base TOMLs at today's `accent.base`
  value so nothing moves) and a soft ground for each signal:
  `status.success_soft`, `error_soft`, `warning_soft`, `modified_soft`
  (rule: the signal at alpha 0x26).
- A new `agent` group: `agent.base` (root, set in `themes/dark.toml` and
  `themes/light.toml`), `agent.soft` (rule: base at alpha 0x26). Nothing reads
  it yet. It must never resolve equal to `accent.base` (test).
- A new `vcs` group: `added`, `modified`, `deleted`, `untracked`, `ignored`,
  `conflict`, with rules from `status.success`, `status.modified`,
  `status.error`, `status.success`, `text.muted`, `status.warning`.
- `UiTheme` does **not** gain the fields (lead ruling, rev 3): `Theme.colors`
  (`Tokens<Hsla>`) carries every new token through the `tokens!` macro, and
  `UiTheme` is the legacy flat view the doc lists as a gap.

fe-config editor keys (`crates/fe-config/src/theme.rs`, `EditorColors`):
`find_match`, `find_match_other`, `identifier_highlight`, `agent_lane`,
`fold_hover`; `crates/fe-gpui/src/theme.rs` exposes them on `EditorTheme`.
The bundled Tenmoku Dark and Light set each to the colour fe-gpui computes
today (`element.rs:search_match_color` foreground at 0.22,
`decorations.rs:tint_color` at `HIGHLIGHT_ALPHA`), because
`crates/fe-config/tests/editor_palette.rs` requires every key set; Tenmoku Ember
(`crates/fe-config/themes/tenmoku-ember.toml`) sets them too, for the same reason. Readers stay
on the old computation in this card.

`docs/design-language.md`: a `### agent` and `### vcs` heading with their
tokens (`crates/fe-config/tests/design_language_doc.rs` requires one per group),
and the new tokens under their groups.

`kiln/crates/kiln/tests/extension_themes.rs`
(`the_user_theme_lays_over_the_selected_theme`): tokens whose rule reads
`status.error` now follow a user's `status.error`, so the test skips them.

## Not in this card

Reading the new tokens anywhere, the Yohen and Hakuji values, font styles.

Platform: **Linux-OK** (no golden changes; token resolution is pure data).
