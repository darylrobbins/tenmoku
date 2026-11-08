---
task_uid: "01m6w2zb171nt22c8s7y9g3f2s"
task: "T522"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p kiln-l10n passes, including the negative fixture: a translated message referencing an en-US-only term in an unselected branch fails the key check"
    required: true
    manual: false
  - id: "C2"
    text: "The behaviour matches the option T1506 chose in docs/decisions/xte5e7hqsw-i18n-design.md section 8"
    required: true
    manual: false
---

# Each locale's bundle resolves its own Fluent references (T1506 G-3) (Mac T1521)

# Each locale's bundle resolves its own references (T1506 G-3)

Spec: `docs/decisions/xte5e7hqsw-i18n-design.md` § 8 "Layer 3" and § 11 G-3, as T1506 rewrote them (review
finding F10): the option T1506 chose (complete per-locale bundles validated as
built, or references required to resolve inside the locale's own resources),
implemented in kiln-l10n's catalogue and its `lint` key check, with the
negative fixture of a bad reference in an unselected branch.

Platform: **Linux-OK**.

## Implementation notes

T521 already landed this behaviour: the runtime builds one bundle per locale from that locale's resources only (`Catalogue::from_sources`), and `lint::catalogue` walks every branch and checks each reference against the same locale's definitions. This card adds no source change. The negative fixtures are in `kiln/crates/kiln-l10n/tests/lint_catalogue.rs`: a term and an attribute only en-US defines, in an unselected branch of a `pl` message, fail the walk, and a runtime assertion shows Fluent reports the error. `cargo nextest run -p kiln-l10n` passes (65 tests). No file with carried findings was touched.
