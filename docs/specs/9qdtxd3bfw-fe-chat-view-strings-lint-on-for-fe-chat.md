---
task_uid: "01m6yhvdwhdteprb9qdtxd3bfw"
task: "T332"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-chat-view --test l10n passes; the chat goldens still match on the Mac"
    required: true
    manual: false
---

# fe-chat-view strings; lint on for fe-chat-view (i18n CV1) (Mac T1534)

# fe-chat-view strings (CV1)

Spec: card **CV1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for CV1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

Platform: **Linux-OK**.
