---
task_uid: "01m71sgcmhe3vtjzq5xaphjwtt"
task: "T548"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list (W1's files)"
    required: true
    manual: false
  - id: "C2"
    text: "Every fe-workbench command has a command-\u003cid\u003e key (the registry test); GitCommand::title() reads the same key; en-US titles byte-equal"
    required: true
    manual: false
  - id: "C3"
    text: "keymaps_picker reads label(); cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# fe-workbench command titles (workbench, find, spell, task and git commands) and fe-workbench's catalogue and lint (i18n W1) (Mac T1537)

# fe-workbench command titles, and fe-workbench's catalogue and lint (W1)

Spec: card **W1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for W1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- Creates fe-workbench's `src/l10n.rs`, `l10n-allow.toml` (with the
  append-only `migrated` list of I1, listing W1's files), `l10n/en-US/` and
  `tests/l10n.rs` (`ui_text_is_in_the_catalogue`, `keys_used_here_exist`).
  Every later fe-workbench card adds its own `l10n/en-US/<area>.ftl`, one line
  in `src/l10n.rs` and its files' lines in `migrated`.
- Lands after the command-label resolver, so macro-declared commands' keys are
  read.

Platform: **Linux-OK**.

## Implementation notes

- `l10n/en-US/commands.ftl` keys every fe-workbench command, 346 of them: the
  `#[kiln::command]`s of every file (keys come from ids, so files outside this
  card's scope needed no change), and its registries' (search, commit box,
  project panel, status bar). `tests/l10n.rs`'s `every_command_has_its_key`
  checks both ways with `check_command_keys`, and that each key's en-US text is
  the declared title byte for byte. Later fe-workbench cards add no command keys.
- The same file holds the migrated files' own messages (`workbench-spell-*`,
  `workbench-task-*`, `workbench-keymaps-*`, `workbench-git-ex-not-supported`,
  `workbench-plugins-none-installed`), so the card adds one `.ftl`.
- `GitCommand::id()` names each command's typed id; `title()` is
  `command_label(id)`, the palette's resolver, and the literal table is gone.
  `git_commands_tests::each_title_is_its_typed_commands` checks it against the
  declared titles.
- `keymaps_picker.rs` already read `label()` (Z1's cross-surface test,
  `tests/command_labels_fake_locale.rs`, covers it), as did the shortcuts
  editor with its `ActiveLocale` observer, so neither needed the change here.
- The spell and task constants the tests imported became functions; their tests
  (added to the scope) assert the English literally, so en-US stays checked.
- Carried T491 F1: a key map's row keeps its `Remap`; confirming replays the
  keys only while vim is on in the editor and the same map is still bound at
  those keys, else it gives focus back and types nothing
  (`keymaps_picker_tests::a_stale_map_of_keys_types_nothing`).
