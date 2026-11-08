---
task_uid: "01m6vvrtev575gxk3s2jas9prx"
task: "T1511"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test each_file_names_its_family_and_weight (cargo nextest run -p fe-fonts --test fonts) passes: every bundled file's name table gives Instrument Sans, Spline Sans Mono or Instrument Serif and the weight it is listed under"
    required: true
    manual: false
  - id: "C2"
    text: "On macOS, test bundled_families_resolve_at_each_weight (cargo nextest run -p fe-workbench --test bundled_fonts) passes: after fe_workbench::init, the text system resolves each family at 400, 500 and 600 (Serif 400) to the bundled face, not a fallback, and the three weights of Instrument Sans give different advances for the same string"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fastedit --test notice passes with the new Fonts table checked (OFL-1.1 rows, OFL.txt beside each family); a fixture row whose OFL.txt is missing fails"
    required: true
    manual: false
  - id: "C4"
    text: "scripts/check-no-gpui.sh passes (fe-fonts has no gpui dependency); no golden changes"
    required: true
    manual: false
---

# Bundle Instrument Sans, Spline Sans Mono and Instrument Serif (OFL-1.1) in a GPUI-free fe-fonts crate, registered at init, with licence rows; not yet the defaults

# Bundle Instrument Sans, Spline Sans Mono and Instrument Serif

Spec: `docs/design/design-system.md#type-spacing-and-sizes` ("Three families").
Today no font is bundled (`THIRD_PARTY.md` "Fonts: none bundled") and text
renders in system fonts. This card ships the three families and registers
them; it does **not** make them the defaults (a later card does), so no golden
moves.

## Change

- New GPUI-free crate `crates/fe-fonts` (workspace member; no gpui
  dependency, so `scripts/check-no-gpui.sh` needs no change):
  `pub fn all() -> Vec<Cow<'static, [u8]>>` over `include_bytes!` of the
  files in `crates/fe-fonts/fonts/`, plus `pub const UI`, `CODE`, `TITLE`
  family names.
- Files: Instrument Sans (400, 500, 600, and their italics if one file),
  Spline Sans Mono (400, 500, 600, 400 italic), Instrument Serif (400),
  from google/fonts at a pinned commit (record it). They ship as variable fonts
  upstream: check that GPUI's text system picks the right weight from them on
  macOS; if it does not, commit static instances made with fontTools'
  `instancer` and record the command in `crates/fe-fonts/fonts/README.md`.
- Each family's `OFL.txt` beside its files.
- Registration: `crates/fe-workbench/src/lib.rs` `init` calls
  `cx.text_system().add_fonts(fe_fonts::all())` (an added line; lib.rs is
  shared append-only). The screenshot examples register them when the
  defaults card switches to them, not here.
- Licences: a `## Fonts` table in `THIRD_PARTY.md` (Our path | Source |
  Commit | License | Files), the "Fonts: none bundled" audit bullet updated,
  a NOTICE block per family, and `crates/fastedit/tests/notice.rs` taught the
  Fonts table and `OFL-1.1` (licence text beside the row's path, as Icons rows
  do). OFL-1.1 permits bundling in commercial software; cargo-deny only checks
  crates, so `deny.toml` does not change.

Platform: **Mac-only** (the resolution test runs on CoreText; the parse test
runs anywhere).

## Implementation notes

- GPUI's macOS text system does not pick weights from the variable fonts: it
  registers an in-memory file as one face at the default instance, so with
  the variable Instrument Sans the 400, 500 and 600 requests set the same
  string at the same width (the C2 test fails). The committed files are
  static instances (fontTools 4.60.1 `varLib.instancer --update-name-table`);
  `crates/fe-fonts/fonts/README.md` has the commands, the google/fonts commit
  (`86aced91de05cd4f96a06838124cca45be337b2b`) and the upstream SHA-256s.
- Instrument Sans's italics are a separate upstream file, so only the three
  upright weights ship; Spline Sans Mono has 400, 500, 600 and 400 italic.
- `fe_fonts::FACES` lists each file with its family, weight and style;
  `all()` maps it. The C2 test tells a bundled face from a fallback by
  comparing each probe character's glyph id and advance with the bundled
  file's (ttf-parser, a dev-dependency).
