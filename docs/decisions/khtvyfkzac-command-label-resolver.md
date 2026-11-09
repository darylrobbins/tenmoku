# One command-label resolver, rebuilt on a locale change (T1526, T1506 G-1)

The decisions T1526 made in building § 4 "Command palette titles and
categories" of `docs/decisions/xte5e7hqsw-i18n-design.md` for the typed-command
path, and what it found already built.

## Already built by T1523 (I3)

- `kiln-command/src/l10n.rs`: `command_key`, `command_label`, `command_detail`,
  `derive_label`, with the order § 4 gives (key, derived label, fallback) and a
  silent miss.
- `label()` on `CommandMeta`, `TypedCommand` and `CommandSpec`.
- `Toggle {}`: both stored titles are `tr!("kiln-command-toggle", title = …)`,
  and `register_toggles` calls `derive_label(id, text!("kiln-command-toggle",
  …))`, so `label()` reaches the toggle's label through step 2. The palette and
  both menu paths (`table_menu_bar`, `place_items`) already read `label()`.

## What this card adds

- **`TypedCommand` doesn't change** (§ 4, "No struct changes"; review F2 of
  round 1): `title` stays the registered English fallback, and every reader
  resolves through `label()` when it shows the title. Round 1 cached the
  resolved title in `title`, relabelled on `ActiveLocale`, so the keymaps
  picker's unchanged `typed.title` followed; codex rejected that as the bypass
  § 4 removes, and it is gone. kiln-platform's
  `the_store_keeps_the_fallback_and_label_follows_the_locale` pins it.
- **kiln re-resolves on `ActiveLocale`** (`contributions::follow_locale`,
  installed by `start`): `resolve_again`, the path an extension's menu change
  takes, so both menu paths rebuild and `menus::install` the bar. The
  catalogue's generation is recorded when `shipped` resolves the labels (a
  thread-local, as `shipped` only reads the app); `follow_locale` skips a set
  at that generation, so the app's first `set_locale`, for the languages the
  warm-up built, re-resolves nothing, and `start` resolves again if the
  generation moved after `init` built the menus, as when the app's config
  source applies a saved `workbench.locale` (review F1 of round 1;
  `a_locale_set_during_init_titles_the_menus`). This is the menu half of
  T1620; T1620 keeps the picker half.
- **Which-key** (`command_title`) reads `label()` for every registry, at each
  render.
- **The Keyboard Shortcuts editor** builds fe-keymap's `CommandInfo` already
  resolved (`title: typed.label().into()`, `meta.label().into()`), and an open
  tab rebuilds its model on `ActiveLocale` beside its `ChangedFiles` observer.
  fe-keymap doesn't change.
- **The keymaps picker** (`keymaps_picker::titles`) reads `label()` for typed
  commands and registry commands, as § 4's W1 row has it, when it opens. Its
  file was added to the card's scope by a respec; C1 needs it.

## The cross-surface test

`crates/fe-workbench/tests/command_labels_fake_locale.rs`, the only test in
its binary (the scope names this file rather than § 4's
`l10n_command_labels.rs`). It starts the app through `kiln::init` with the
workbench's commands, keymap and menus, adds the binary's
`#[kiln::command(id = "workbench.probe_labels", context = "workbench", menu =
"File")]`, maps `<leader>p` to it, and opens the Keyboard Shortcuts tab before
the switch. The fake locale is a hand-built `Resources` submitted through
`inventory` (no fixture files, outside the scope), as T1523's tests do. Per
surface:

- the palette: `commands_in`;
- the menu: every action item the platform has (`cx.get_menus()`), after the
  switch's `resolve_again`;
- which-key: `EditorView::which_key_rows`, a `#[doc(hidden)]` accessor of the
  popup's rows as drawn (`show_which_key_for` holds `<leader>`), since
  `command_title` is crate-private and `vim.rs`/`lib.rs` are out of scope;
- the keymaps picker, opened after each switch: the test types the title and
  presses Enter, and the probe's action runs only if the `<Space>p` row has
  that title (its row accessors are `pub(crate)` and `cfg(test)`);
- the shortcuts editor, never reopened: `Workbench::shortcuts_editor_rows`, a
  `#[doc(hidden)]` accessor of the open tab's rows.

C2's evidence is kiln's `a_panel_toggle_is_titled_in_the_locale_set`: a
panel's View menu item and its registry label follow a fake
`kiln-command-toggle` and back, the registered title staying English.
