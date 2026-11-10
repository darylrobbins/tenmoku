---
task_uid: "01m71sg90g64vfkbqj0zzs5ds3"
task: "T340"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "A test switches a running app to a fake locale and back and checks the installed menu bar after each switch, through both the contributed menu table and the extension menu path (cargo nextest run -p kiln l10n_locale_change)"
    required: true
    manual: false
  - id: "C2"
    text: "A test keeps one picker open while switching to a fake locale and back and checks its placeholder and accessible name after each switch (cargo nextest run -p kiln-shell picker_locale_change)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p kiln -p kiln-shell -p kiln-ui passes, and en-US output is byte-equal to before (no golden changes)"
    required: true
    manual: false
---

# Menus and open pickers follow a locale change: resolve_again on ActiveLocale, Picker keeps its Text placeholder (I3 follow-up, T1523 F2 F3) (Mac T1620)

# Menus and open pickers follow a locale change (I3 follow-up: T1523 F2, F3)

Spec: the locale-change rows of card **I3** in `docs/decisions/xte5e7hqsw-i18n-design.md`
(§ 4 and the § 8 table, as amended by T1506), which T1523 left out because I2's
`ActiveLocale` (T1522) had not landed. T1523 built the label resolution; this card wires it to
locale changes.

## What to build

- **Menus (T1523 review F2).** kiln observes I2's `ActiveLocale` and calls `resolve_again`
  in `kiln/crates/kiln/src/contributions.rs`, so both contributed-menu paths (the menu table and
  the extension menu path) rebuild the bar and `menus::install` it in the new language.
- **Open pickers (T1523 review F3).** `Picker::new` keeps the keyed `Text` it is given, and the
  input resolves it when it renders (or is updated from it on a locale change), so an open
  picker's placeholder and its fallback accessible name follow the locale. This needs
  `kiln/crates/kiln-ui/src/text_input.rs` to hold a `Text` placeholder.

en-US text stays byte-equal to today's (§ 9): no golden changes.

Platform: **Linux-OK**.
