---
task_uid: "01m6wt0wv9j3tbadx0ggfr7ha1"
task: "T329"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test monokai_primary_button_text_is_readable (cargo nextest run -p fe-config --test monokai_on_accent) fails at base and passes with the fix"
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-config/tests/monokai_theme.rs still passes"
    required: true
    manual: false
---

# Monokai theme: primary-button text (text.on_accent) reads #F8F8F2 on its accent, not #272822 (T1418 F1) (Mac T1611)

Review follow-up of T1418, finding F1 (carried by T1509, which may not change pre-existing tokens).

`crates/fe-config/themes/vscode-monokai.toml:42` sets `text.on_accent = "#272822"` on `accent.base = "#75715E"`. That is
dark text on a mid-grey accent: primary-button labels are hard to read. T1418's spec asked for `#F8F8F2`.

Fix: set `text.on_accent = "#F8F8F2"`. Add a regression test in a new file, `crates/fe-config/tests/monokai_on_accent.rs`:
load the bundled Monokai theme and assert that `text.on_accent` resolves to `#F8F8F2`, and that its contrast against
`accent.base` is at least 4.5:1 (WCAG relative luminance, computed in the test).
