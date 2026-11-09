---
task_uid: "01m6vvsjxbn6zemggtrfm2g4st"
task: "T1527"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The fake-locale test passes: the bundled Services item and a keyed command item come out in the fake locale in the built GPUI menus and in the drawn bar"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-core --test l10n passes with kiln-core's lint on; en-US menus are byte-equal to today's"
    required: true
    manual: false
---

# kiln-core and kiln menus: CommandItem/SystemItem title_key and label(), menu ids, $app, Linux key-name labels (i18n K1)

# kiln-core and kiln menus: title_key, label(), menu ids, Linux key names (K1)

Spec: card **K1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for K1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- The plan waits for the audit's P4 and P5 (macOS menu bar per the HIG).
  They are not filed; do not wait for them.

Platform: **Linux-OK**.

## Implementation notes

- **Model.** `CommandItem`, `SystemItem` and `Menu` gain `title_key:
  Option<String>` and `label() -> Text`; `Menu` gains `id`. A table writes
  `title_key` in place of an item's `title` or a menu's `name` (both is an
  error); the parser fills `title`/`name` with the key's en-US text from an
  en-US-only catalogue, so every reader of `title` keeps working. A key en-US
  lacks is a warning, titled as the key. A Services item with no title is
  `kiln-core-menu-services`.
- **Menu ids.** `id` is the menu's en-US name, not a slug such as `file`,
  and a table can't set one (an `id` field is unknown): `MenuBar::menu`,
  layers (`MenuBar::extend`), extensions (`extend_menus`),
  `#[kiln::command(menu = "File")]` and `MenuCommand` go by `id`, so every
  existing name-based lookup, app, user and extension layer finds the same
  menu as before, in every locale, and no file can merge into a menu it
  isn't named for (review F1). kiln's app menu takes the app's name as both
  id and title (`name_app_menu`).
- **Scope expansion (review F1).** `kiln/src/extensions.rs`: the check that
  an extension can't replace an app's item goes by the menu's `id`, and its
  warning finds a keyed menu's item by `title_key`;
  `kiln/tests/extension_keys_menus.rs` holds the two regression tests
  (`a_keyed_extension_menu_cannot_replace_the_apps_items`,
  `an_extension_item_over_a_menu_table_names_the_app`).
- **The macOS Window menu (review F2).** gpui-pre-macos makes a menu the
  app's Windows menu only when its title is `Window`. `menus::refresh` finds
  the menu whose id is `Window` and, when its title in the active locale
  isn't, registers it with AppKit itself (`setWindowsMenu:`, main thread,
  after `set_menus`), at startup and after every locale switch;
  `menus::windows_menu` is the choice, unit-tested in a fake locale.
- **`$app`.** `kiln_core::menu::set_label_arg` holds process-wide label
  arguments, as kiln-command's does for command titles;
  `Config::name_the_app_in_menus` sets `app`, from `Config::menu_source` and
  at the top of `shipped`, so it is set before any menu file is parsed,
  including extensions' over a contributed menu table (review F3). It
  replaces the `{app}` text substitution. Labels resolve with the arguments
  current when shown.
- **kiln's menu keys are kiln-core's.** `kiln/l10n/**` is outside this card's
  scope, so `menus.toml`'s keys are `kiln-core-menu-*` in kiln-core's
  catalogue, in its `## public` group; kiln's root test already counts
  `menus.toml`'s `title_key`s as used.
- **The drawn bar** reads `label()` for menus, command items and system
  items. A system item was drawn as a separator; it now shows its title and
  runs nothing (never highlighted), so the Services title comes out in the bar
  (C1). No table in the repo has a Services item on Linux (fastedit's is
  macOS-only), so no drawn menu changes.
- **Lint on for all of kiln-core.** The literal lint reports about 150 more
  literals than the plan's estimate of 51 (it counted capitalised text only):
  keymap, keystroke, menu, settings-layer, theme, write, trust, secrets,
  extension and SPDX diagnostics, now in `kiln-core.ftl` and `extension.ftl`.
  Each diagnostic resolves in the locale active when it is made. The
  allow-list keeps only non-UI text: platform directory names, layout file
  names, TOML the writers emit, temporary file names, key paths, SPDX ids and
  prefixes, the default keymap and menus' pseudo-paths, the compiler-style
  `file:line:column: error:` prefix, and the doc comments `tokens!` turns into
  string tokens. The trust and acknowledged-licences files' header comments
  are messages too.
- **Tests.** C1's fake-locale test is
  `kiln-shell` `menu_bar::tests::bundled_titles_follow_the_locale` (inline,
  as `kiln-shell/src/menu_bar.rs` is the scope's file there): GPUI's built
  menus and the drawn bar, through `locale::set_locale`, and back. C2's
  `kiln-core/tests/l10n.rs` runs the lint and key checks and pins en-US byte
  for byte: kiln's bundled menus on each platform, the Services default, the
  Linux key names and the trust file. kiln's `the_bundled_menus_are_keyed`
  checks `menus.toml` has no literal title.
