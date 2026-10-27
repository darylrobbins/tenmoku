---
task_uid: "01m5w9avvbngj1m94nn1fggh79"
task: "T360"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "save_commands_tests::save_all_writes_every_dirty_file: with a.txt and b.txt dirty in two panes and c.txt clean, workbench.save_all writes a and b to disk and not c, runs format and trim on save for each as an explicit save, saves a buffer shown in two tabs once, and skips untitled buffers and read-only viewers without error"
    required: true
    manual: false
  - id: "C2"
    text: "save_commands_tests::save_all_reports_failures: a file changed on disk keeps its conflict state and does not stop the others from saving; the command returns after all saves finish"
    required: true
    manual: false
  - id: "C3"
    text: "save_commands_tests::save_without_formatting: with editor.format_on_save, trim_trailing_whitespace and insert_final_newline all on, workbench.save_without_formatting writes the buffer text exactly as it is (no format, trim, final newline or code actions on save) and the buffer is clean afterwards; a plain editor.save on the same buffer does apply them"
    required: true
    manual: false
  - id: "C4"
    text: "fe_editor::auto_save::save_reason: SaveReason::ExplicitRaw has edits_allowed false in every vim state yet still prompts on a changed-on-disk file as Explicit does (fe-gpui save_reason_tests extended, same file)"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap save_all_key: alt-cmd-s resolves to workbench.save_all and cmd-k s to workbench.save_without_formatting on macOS in the workbench context; the Linux keymap binds ctrl-k s to save_all only if VS Code's Linux default does (confirm; else palette only); Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-gpui keymap_tests, fe-workbench default_keymaps); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Save All (alt-cmd-s) and Save without Formatting (cmd-k s), as VS Code's workbench.action.files.saveAll and saveWithoutFormatting (Mac T1187)

Neither command exists (grep for save_all finds nothing; only editor.save and workbench.save_as). Typed commands in a new module save_commands.rs with its own "#[kiln::command] impl Workbench" block, as find_commands.rs does, so workbench_commands.rs (148 commands, a merge-conflict magnet) is not edited; lib.rs gains the mod line.

Save All: every editor of a file buffer in every pane of the window that is_dirty, once per BufferId, each through EditorView::save_with(SaveReason::Explicit) so format on save, save fixes and the conflict prompt behave as for cmd-s. Untitled buffers are skipped (VS Code would open a Save As for each; a follow-up can add that). Join the tasks and wait for all.

Save without Formatting: saves the active editor with a new SaveReason::ExplicitRaw. VS Code skips all save participants for it (format on save, trim trailing whitespace, insert final newline, code actions on save), so edits_allowed is false; but unlike AutoDelay it is an explicit save: pane_item.rs save_with currently derives ask = (reason == Explicit), change it to a SaveReason::asks() helper true for both. Keep the change to those two fe-editor and fe-gpui files minimal; do not restructure save_with.

Keys per VS Code's default macOS keymap: alt-cmd-s Save All, cmd-k s Save Without Formatting. Check neither is bound (grep secondary-alt-s and "secondary-k s" in fastedit.toml found nothing at drafting time). Linux: VS Code uses ctrl-k s for Save All; confirm via context7/VS Code docs before binding; no Linux key for Save Without Formatting if unsure. Add menu entries only if the File menu already lists Save As (menus/fastedit.toml); otherwise leave menus alone.

Tests: workbench harness from auto_save_tests.rs / editorconfig_save_tests.rs (temp dir, headless).

Lent from Mac T1187 (y74). Linux-OK: headless workbench tests; keys use secondary-*. Queued behind T359 (Mac T1188) on shared lib.rs/catalog/keymap lists.
