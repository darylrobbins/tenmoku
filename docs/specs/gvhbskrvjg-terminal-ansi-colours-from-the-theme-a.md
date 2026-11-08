---
task_uid: "01m6vvrxhsm08pn4gvhbskrvjg"
task: "T1513"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test a_theme_without_the_table_keeps_todays_sets (cargo nextest run -p fe-config --test terminal_ansi_theme) passes for every bundled theme but Yohen and Hakuji"
    required: true
    manual: false
  - id: "C2"
    text: "Test yohen_and_hakuji_ansi_clear_4_5_on_the_editor (same binary) passes for ansi_1..ansi_7 and ansi_9..ansi_15"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-terminal-view -p fe-workbench terminal passes"
    required: true
    manual: false
---

# Terminal ANSI colours from the theme: a [terminal] table, today's two sets as the defaults, Yohen and Hakuji from their palette

# Terminal ANSI colours from the theme

Spec: `docs/design/design-system.md#surfaces` (terminal on the editor surface)
and `#terminal`. The held audit card P23 / finding TE1.

Today the 16 ANSI colours are two fixed sets picked by the background's
lightness (`crates/fe-terminal-view/src/style.rs` `ansi_dark`, `ansi_light`,
`ansi_for`).

## Change

- A `[terminal]` table in fe-config themes (`ansi_0` … `ansi_15`, plus
  `foreground`, `background`, `cursor`), read in `crates/fe-config/src/theme.rs`
  (added to `EXTENSIONS`), with today's two sets as the bases' defaults, so every
  other theme draws as now.
- Yohen and Hakuji set the table from their palette (red shinsha, green
  celadon, yellow kiseto, blue ruri, magenta jun, cyan seiji, white ink,
  bright variants one step lighter), each ≥ 4.5:1 on the editor surface
  except black/bright black.
- `crates/fe-workbench/src/terminal_panel.rs` passes the theme's table to the
  view instead of calling `ansi_for` (one call site).

Platform: **Linux-OK**.
