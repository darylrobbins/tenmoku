---
task_uid: "01m72y4h4wjt0gg835wferc539"
task: "T1720"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "On the Mac, cargo run -p fe-workbench --example screenshots -- audit --pseudo target/ui-audit-pseudo writes every audit scene in en-XA (recorded in the decision file, with any English text found listed as findings)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo run -p fe-workbench --example screenshots -- audit without --pseudo writes the same scenes as before, in en-US"
    required: true
    manual: false
---

# Pseudo-locale audit capture: screenshots audit --pseudo writes every audit scene in en-XA, run on the Mac (split from T1564)

# Pseudo-locale audit capture: screenshots audit --pseudo (split from T1564)

Spec: `docs/decisions/xte5e7hqsw-i18n-design.md` §§ 6 and 8. T1564 adds the built-in pseudo-locale `en-XA`
(selected by `TENMOKU_PSEUDO_LOCALE=1`); this card is its Mac half, split out so T1564 could run on Linux.

## Change

- The audit capture takes `--pseudo`: `cargo run -p fe-workbench --example screenshots -- audit --pseudo
  target/ui-audit-pseudo` writes every audit scene in `en-XA` (through the same selection T1564 added, not a
  second path). Without `--pseudo` the audit is unchanged.
- Run it on the Mac and record in the decision file which scenes still show plain-English text: each is a
  string the catalogue missed, listed as a finding (file a follow-up for them; fixing them is not this card).

Platform: **Mac only** (the audit capture renders through Metal/CoreText).
