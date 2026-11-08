---
task_uid: "01m6vvs1cftkf0v677mbge01pe"
task: "T1516"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test tests::icon_stroke::every_bundled_icon_strokes_at_1_75 (cargo nextest run -p kiln-ui) passes"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-ui passes, including tests::assets::bundled_files_match_the_enum with the new icons; no existing icon is renamed (which-key icon names keep working: cargo nextest run -p fe-gpui which_key_icons)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fastedit --test notice passes with the Icons rows' file lists updated"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (every example's goldens, kiln's gallery included: cargo run -p kiln --example gallery-screenshots matches too)"
    required: true
    manual: false
---

# Line icons per the design: Lucide at 1.75 stroke, plus the icons the B3 surfaces need (Runs, Git log, find toggles, spinner, signals)

# Line icons per the design: Lucide at 1.75 stroke, and the icons B3 needs

Spec: `docs/design/design-system.md#workbench-anatomy` (Icons: 16 px line icons
on a 24 grid, 1.7–1.8 stroke, in the text colour of their state; Agents a small
robot head; Runs a play triangle with list lines; Changes a page with +/−).

## Change

- Every SVG in `kiln/crates/kiln-ui/assets/icons/` (Lucide 1.52.0) drawn at
  `stroke-width="1.75"`. Record the modification in the Icons row of
  `THIRD_PARTY.md` and `kiln/THIRD_PARTY.md` (ISC permits it).
- Add the Lucide icons the design and its surface cards need that the set
  lacks (check `Icon::ALL` first): `list-video` (Runs), `square-terminal`,
  `git-commit-vertical` (Git log), `case-sensitive`, `whole-word`, `regex`,
  `text-select` (find toggles), `loader-circle` (spinner), `circle-check`,
  `circle-x`, `triangle-alert`, `chevron-up`, `pin`, `square-plus`,
  `square-minus`, `bell`, `play`, `square` (stop). Same Lucide tag; each gets an
  `Icon` variant in `kiln/crates/kiln-ui/src/file_icons.rs` and the THIRD_PARTY
  file list.
- Icon names are user-visible (which-key `icon=` in vimrc and settings): rename
  nothing.
- Regenerate every golden that draws an icon on the Mac (all five example sets:
  fe-gpui, fe-workbench, fe-markdown-view, fe-chat-view, kiln gallery).

Platform: **Mac-only** (goldens).
