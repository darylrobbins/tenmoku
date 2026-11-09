---
task_uid: "01m6vvsh8c5y9hmckhtvyfkzac"
task: "T1526"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test macro_command_label_follows_the_locale_everywhere (cargo nextest run -p fe-workbench --test command_labels_fake_locale) passes: a #[kiln::command] command's title is the fake locale's in the palette, a menu item, which-key, the keymaps picker and the shortcuts editor, and returns after switching back"
    required: true
    manual: false
  - id: "C2"
    text: "Toggle {} titles come from kiln-command-toggle; the behaviour matches T1506's text in docs/decisions/xte5e7hqsw-i18n-design.md section 4"
    required: true
    manual: false
---

# One command-label resolver for TypedCommand, CommandSpec and fe-keymap's CommandInfo, rebuilt on a locale change (T1506 G-1)

# One command-label resolver for TypedCommand, CommandSpec and CommandInfo (T1506 G-1)

Spec: `docs/decisions/xte5e7hqsw-i18n-design.md` § 4 "Command palette titles and categories" and § 11 G-1, as
T1506 rewrote them (the typed-command label resolver, review finding F13).
Build what that section specifies:

- the GPUI-free resolver in kiln-command, from a command id and its fallback
  title to a localised title, applied where each representation is stored:
  `TypedCommand` (the `#[kiln::command]` records, kiln-platform's
  `TypedCommands` global), kiln's `CommandSpec` (including
  `Toggle {}` → `kiln-command-toggle = Toggle { $title }`), and fe-keymap's
  `CommandInfo` (built by `shortcuts_editor.rs`);
- each store rebuilt on a locale change (`ActiveLocale`);
- the fake-locale test of a macro-declared command across palette, menu,
  which-key, keymaps picker and shortcuts editor.

If T1506's text differs from this summary, T1506's text wins.

Platform: **Linux-OK**.

## Implementation notes

See `docs/decisions/khtvyfkzac-command-label-resolver.md`. In short:

- The resolver, `label()` on each type and the `kiln-command-toggle` titles were
  already T1523's; this card wires them to `ActiveLocale`.
- `TypedCommand` is unchanged: `title` stays the English fallback and readers
  call `label()` (review F2); the keymaps picker's `titles` moves to `label()`
  (`keymaps_picker.rs`, added to the scope by rev 2).
- kiln calls `resolve_again` on `ActiveLocale` (skipping a set at the generation
  the menus' labels were resolved in, and resolving again at `start` if a config
  source set a locale during `init`, review F1): the menu half of T1620.
- Which-key and the shortcuts editor read `label()`; an open shortcuts tab
  rebuilds its model on `ActiveLocale`. fe-keymap is unchanged.
- C1: `crates/fe-workbench/tests/command_labels_fake_locale.rs`. C2: kiln's
  `contributions::tests::a_panel_toggle_is_titled_in_the_locale_set`.
