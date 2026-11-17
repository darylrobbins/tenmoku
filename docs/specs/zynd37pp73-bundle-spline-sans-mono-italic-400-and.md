---
task_uid: "01m6w0arv705ncm5zynd37pp73"
task: "T1599"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test each_file_names_its_family_and_weight (cargo nextest run -p fe-fonts --test fonts) passes with the new italic files"
    required: true
    manual: false
  - id: "C2"
    text: "On macOS, test bundled_families_resolve_at_each_weight (cargo nextest run -p fe-workbench --test bundled_fonts) passes with italic 400 and 600 resolving to the bundled faces, not a fallback"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fastedit --test notice passes with the new rows"
    required: true
    manual: false
---

# Bundle Spline Sans Mono Italic (400) and Bold Italic faces

# Bundle the italic and bold faces of Spline Sans Mono

Spec: Syntax (italic comments, bold definitions) needs faces to draw them in.
T1511 bundles Spline Sans Mono at 400, 500 and 600 upright only; synthesizing
italic by slanting would not match the design.

## Change

- `crates/fe-fonts`: add Spline Sans Mono Italic at 400 and 600 Italic
  (OFL-1.1), registered with the others; licence rows in `THIRD_PARTY.md`
  and `NOTICE` as the upright faces have.
- `crates/fe-fonts/tests/fonts.rs` (the name-table test) lists the new files;
  the `bundled_fonts` test in fe-workbench resolves italic 400 and 600 to the
  bundled face, not a fallback.

## Not in this card

Using them (the shaping card), Instrument Sans italics.

Platform: **Linux-OK** for the name-table test; the resolution test is macOS.
