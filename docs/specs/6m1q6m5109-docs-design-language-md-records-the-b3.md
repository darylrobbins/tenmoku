---
task_uid: "01m6vvw6t4esb4e26m1q6m5109"
task: "T1566"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-config --test design_language_doc passes"
    required: true
    manual: false
  - id: "C2"
    text: "Every Gaps entry a B3 card closed is struck with the card's handle; only docs/design-language.md changes"
    required: true
    manual: false
---

# docs/design-language.md records the B3 look: role tokens, Yohen and Hakuji, roles.rs, fonts, icons, and the gaps the B3 cards closed

# docs/design-language.md records the B3 look

Spec: `docs/design/design-system.md` (the should) and the held audit card P27.
`docs/design-language.md` records what the code does; once the B3 cards land it
must say so.

## Change (`docs/design-language.md` only)

- Colour tokens: the B3 role tokens (`surface.chrome`, `text.faint`,
  `border.control`, `status.modified`, the soft grounds, `agent`, `vcs`,
  `file_icon`) and the editor keys added; Yohen and Hakuji as the defaults.
- Spacing and Type: kiln-ui's `roles.rs` constants replace the measured
  literals; the three bundled families.
- Icons: 1.75 stroke, the rail set, the tinted file icons.
- Gaps: strike each gap a B3 card closed (Hard-coded colours that moved into
  themes: git graph, ANSI, proposal tints; Two colours for one role; Spacing;
  Type; Icons), and add any the cards left.
- A pointer to `docs/design/design-system.md` at the top.
- `crates/fe-config/tests/design_language_doc.rs` still passes.

Platform: **Linux-OK**.
