---
task_uid: "01m7202akrx4h3h83awfe9hayr"
task: "T553"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-editor --test l10n passes with fe-editor's lint on for the whole crate"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-editor passes unchanged apart from tests that built removed literals"
    required: true
    manual: false
---

# fe-editor command titles and messages; lint on for fe-editor (i18n E1) (Mac T1529)

# fe-editor command titles and messages (E1)

Spec: card **E1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for E1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- Scope is the files that carry text today (listed), not `src/**`, so the
  editor's other ready cards (T1357, the RTL caret cards) are not blocked. If
  the lint reports another file, ask for it to be added.
- Waits for T1502 (`column_select.rs`, `multi_cursor.rs`).

Platform: **Linux-OK**.

## Implementation notes

- **Titles and descriptions.** `l10n/en-US/fe-editor.ftl` holds a
  `command-<id>` message for each of the registry's 104 commands, and
  `editor.insert`'s and `editor.move`'s descriptions as `.description`.
  `commands::add` and the two `register_with_args` calls register with the
  title and description resolved from the catalogue then
  (`l10n::command_title`, `l10n::command_description`), as fe-gpui's G1
  does; `label()` and `detail()` resolve them again where they're shown. The
  title tuples lost their title column. A dump of every command's id, title
  and description before and after the change is byte-identical.
- **Messages.** The undo, redo and save failures, `editor.move`'s count
  limit, the save errors (`SaveError::ChangedOnDisk` and the io errors of
  `disk.rs`), `ChangesetError` and `CopyError` are `editor-*` messages with
  named arguments; `CopyError`'s byte count is a plural selector.
- **Scope added.** The lint also reported `lib.rs` (which declares
  `mod l10n`), `changeset.rs` and `viewer.rs` (user-facing errors), added by
  respec; plus `Cargo.lock` for the new dependency. `auto_indent.rs` and
  `scripted_ai.rs` are only allow-listed, not edited: the first's literal is
  a line break and indentation, the second is the scripted fake AI's
  developer script language (verbs, parse errors, step log).
- **Allow-list.** `all = true`, with the command id format, the joins edits
  write into the buffer (comment tokens, line breaks and indentation) and an
  `.expect` inside `thread_local!`, which the lint reads as a bare literal.
- **Tests.** `tests/l10n.rs` has the literal and key lints, the registry
  check (`every_command_has_its_key`, via `check_command_keys`) and
  `messages_read_as_before`, which checks each moved message's en-US text
  against the English it replaced.
