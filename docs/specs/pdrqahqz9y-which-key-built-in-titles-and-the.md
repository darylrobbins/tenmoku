---
task_uid: "01m6w2zdmq9phm2epdrqahqz9y"
task: "T525"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The preset test passes: every bundled fegroup has a vim-group-* key; user groups stay as written"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim --test l10n passes with fe-vim's lint on for the whole crate"
    required: true
    manual: false
---

# Which-key built-in titles and the bundled LazyVim preset's group names as keys (i18n V2) (Mac T1525)

# Which-key built-ins and the LazyVim preset's groups as keys (V2)

Spec: card **V2** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for V2 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- The plan waits for T1396; T1396 was abandoned, so don't wait.

Platform: **Linux-OK**.
