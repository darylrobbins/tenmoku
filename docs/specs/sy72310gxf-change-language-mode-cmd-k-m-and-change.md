---
task_uid: "01m5wbffdfprzqbpsy72310gxf"
task: "T361"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-keymap picker_chord_keys: on macOS cmd-k m resolves to language.select and cmd-k cmd-t to theme.select in the workbench context, and in an editor context (editor stacked on workbench); on Linux ctrl-k m and ctrl-k ctrl-t resolve to the same two commands; a lone m or t in an editor still resolves to typing (the chord prefix does not swallow later keys)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-keymap picker_chord_keys::existing_chords_unchanged: cmd-k z, cmd-k v, cmd-k cmd-w, cmd-k cmd-0 and cmd-k cmd-q still resolve to their present commands on both platforms"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench default_keymaps: every command named by the two new bindings is a registered typed command (language.select, theme.select); cargo nextest run -p fe-keymap -p fe-workbench and clippy pass"
    required: true
    manual: false
---

# Change Language Mode (cmd-k m) and Change Color Theme (cmd-k cmd-t) keys, as VS Code's workbench.action.editor.changeLanguageMode and workbench.action.selectTheme (Mac T1190)

Both commands exist and are reachable only from the palette and the status bar: language.select ("Change Language Mode…", workbench_commands.rs, LanguagePicker) and theme.select ("Change Color Theme…", ThemePicker). Neither has a default key (rg of fastedit.toml finds neither id). Add the VS Code chords: Change Language Mode is cmd-k m (ctrl-k m elsewhere); Select Color Theme is cmd-k cmd-t (ctrl-k ctrl-t elsewhere). Use the secondary-k / secondary-m / secondary-t spelling the other chords use ("secondary-k secondary-1"), context "workbench" like "secondary-k z" (workbench.toggle_zen_mode, ~line 696), and place them next to it with a one-line comment naming the VS Code commands.

No Rust changes outside tests: do not touch workbench_commands.rs or any fe-editor file. Check the chord prefix interplay: "secondary-k" is already a prefix for ~25 chords, so adding "m" and "secondary-t" must not change how any existing chord, a vim <C-k> or a lone key resolves; the new test covers that with the keymap-resolution helpers used by tests/trim_whitespace_key.rs and tests/last_edit_key.rs (copy their helper shape; new tests go in the new file tests/picker_chord_keys.rs only).

Add the two commands to the macos/non-macos expectations in fe-workbench tests/default_keymaps.rs only if that file enumerates bindings by platform; otherwise leave it untouched and drop it from scope when finishing.

Lent from Mac T1190 (y75). Linux-OK, headless tests. Serialized behind T360 on shared keymap/catalog lists.
