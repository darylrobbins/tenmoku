---
task_uid: "01m72y5e6q7ftdgxh26nm4h83g"
task: "T343"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test pseudo_locale_transforms_text_not_placeables (cargo nextest run -p kiln-l10n) passes: accented, about 35% longer, bracketed; placeables and selectors intact"
    required: true
    manual: false
  - id: "C2"
    text: "Test every_message_formats_in_the_pseudo_locale (cargo nextest run -p fastedit --test l10n_catalogue) passes for every key the app links, with sample arguments"
    required: true
    manual: false
  - id: "C3"
    text: "en-XA is selected only by TENMOKU_PSEUDO_LOCALE=1 and is absent from the shipped-locale list and the schema enum (a test)"
    required: true
    manual: false
---

# A pseudo-locale (en-XA) for finding uncatalogued, clipped and concatenated text, with a pseudo audit capture (Mac T1564)

# A pseudo-locale for finding untranslated, truncated and concatenated text

Spec: `docs/decisions/xte5e7hqsw-i18n-design.md` §§ 3, 6 and 8 (the catalogue, locale selection and the
completeness checks). The design checks completeness statically (the lint and
key checks); this adds the runtime check translators' tools use, so a person can
see what the lint can't: text drawn outside the catalogue, layouts that clip
longer languages, and messages assembled from fragments.

## Change

- kiln-l10n: a built-in pseudo-locale `en-XA`, generated from en-US at
  catalogue build (no `.ftl` files): each message's text accented
  (a→å, e→é, …), padded about 35% longer, and wrapped in `⟦ ⟧`, placeables
  and selectors kept. Never in the shipped-locales list or the schema enum.
- Selected only by `TENMOKU_PSEUDO_LOCALE=1` (read in kiln-platform's
  `locale::preferred`), so users never see it.
- A test that formats every key of every resource linked into the app root
  test (`crates/fastedit/tests/l10n_catalogue.rs`) in `en-XA` with sample
  arguments, and fails on a formatting error (a missing argument, a selector
  with no default).

Platform: **Linux-OK**. The audit capture's `--pseudo` flag and its Mac run were split out
(2026-11-10, user-approved) into a Mac follow-up card that depends on this one.
