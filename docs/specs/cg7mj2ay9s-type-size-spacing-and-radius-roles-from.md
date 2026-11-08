---
task_uid: "01m6w2wesz6s1zz4cg7mj2ay9s"
task: "T319"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test tests::roles::values_match_the_design_tables (cargo nextest run -p kiln-ui) passes: every TextRole, space, radius, height and width constant equals its row in docs/design/design-system.md#type-spacing-and-sizes"
    required: true
    manual: false
  - id: "C2"
    text: "FontFamilies::default() is today's families (system UI font, kiln_platform mono family); no golden changes"
    required: true
    manual: false
---

# Type, size, spacing and radius roles from the design as named constants in kiln-ui (roles.rs), adopted by nothing yet (Mac T1510)

# Type, size, spacing and radius roles as named constants in kiln-ui

Spec: `docs/design/design-system.md#type-spacing-and-sizes`. Replaces the held
audit card P9's values (`docs/decisions/25w38k498b-ui-polish-audit.md`,
Cross-cutting › Spacing scale and Type ramp) with B3's.

Constants only, adopted by nothing yet: later cards switch each widget over, so
this card moves no pixel.

## Change

New `kiln/crates/kiln-ui/src/roles.rs`, exported from kiln-ui's `lib.rs`:

- `TextRole` { family slot (`Ui`, `Code`, `Title`), size, line height, weight }
  with one constant per row of the design's type table: `PAGE_TITLE` 30/36 400
  (Title), `SECTION_HEADING` 15/20 600, `UI_HEADING` 13/18 600, `UI_BODY`
  13/18 400, `UI_LABEL` 12/16 500, `UI_SMALL` 11/14 500 (Ui), `CODE` 13/20 400,
  `CODE_SMALL` 12/18 400 (Code).
- `FontFamilies { ui, code, title }`: the host sets it as a GPUI global;
  `FontFamilies::default()` is today's (system UI font, `mono_family`, system
  UI font), so kiln apps that don't set it see no change.
- `space::{S2, S4, S6, S8, S12, S16, S24, S40}`, `radius::{FILL 2, CONTROL 4,
  MENU 6, FLOAT 10}`, `height::{TITLE_BAR 38, TAB_STRIP 34, ROW 24, MENU_ROW 28,
  STATUS_BAR 24, CONTROL 26, CONTROL_SMALL 22, RAIL_ITEM 32}`,
  `width::{RAIL 42, SIDEBAR_MIN 232, SIDEBAR_DEFAULT 248}`,
  `BOTTOM_PANEL_DEFAULT 250`, `SHADOW_FLOAT` (0 12 32, 55% / 14% black) and
  `FOCUS_RING` (2 px, offset 1 px).
- A doc comment per constant naming the design row it comes from.

Platform: **Linux-OK**.
