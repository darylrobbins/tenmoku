---
task_uid: "01m6vvtaqfy5cd7b8yacwmzjbq"
task: "T1536"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Each of the twelve crates' l10n tests passes with its lint on; language and file names and git stderr matchers are allow-listed with reasons"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git still matches git's English stderr (LC_ALL=C kept); cargo nextest run for the twelve crates passes"
    required: true
    manual: false
---

# The remaining crates' strings: fe-lsp, fe-markdown-view, fe-chat, fe-chat-anthropic, fe-net, fe-nav, fe-syntax, fe-git, fe-project, fe-fileview, fe-buffer, fe-text (i18n X1)

# The remaining crates: fe-lsp, fe-markdown-view, fe-chat, fe-net, fe-nav, fe-syntax, fe-git and others (X1)

Spec: card **X1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for X1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

Platform: **Linux-OK**.

## Implementation notes

- Each of the twelve crates has `tests/l10n.rs` (literal lint with `all = true`); crates with user-facing text also have `src/l10n.rs`, `l10n/en-US/*.ftl` and a keys test. Non-user-facing text (diagnostics, external stderr matchers, file/language names, fixtures) is allow-listed with reasons. fe-git keeps `LC_ALL=C`.
- Carried findings not fixed: T1176 F2 (needs a new `BufferEvent` variant, own card), T1254 F1, T1381 F1/F2, T1392 F1/F2 (files untouched). T1176 F3 fixed.
