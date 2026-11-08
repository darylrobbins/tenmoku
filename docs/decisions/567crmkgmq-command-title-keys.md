# Command titles as keys (i18n I3, T1523)

The decisions T1523 made in building card I3 of
`docs/decisions/xte5e7hqsw-i18n-design.md`, and what it left to T1620.

## Deferred to T1620: following a locale change

Rev 3 of the spec takes these out of this card. They need I2's `ActiveLocale`
(T1522), which hasn't landed:

- **The menu bar rebuilding on a locale change** (review F2): kiln observing
  `ActiveLocale` and calling `resolve_again`.
- **An open picker's placeholder following a locale change** (review F3):
  `Picker` keeping its `Text`, which needs `kiln-ui/src/text_input.rs` to
  store one.

Until then a label resolves when it is built:

- menu items, when kiln builds the menu bar;
- the palette's entries, when it opens;
- a picker's placeholder, when the picker is made;
- a button's label, at each render.

The fake-locale tests set the locale before they build the app or open the
palette.

## `app.hide` takes the app's name

kiln-platform's app registry titles `app.hide` `Hide <app name>`, and the
palette lists that entry. A plain `command-app-hide = Hide` would change the
en-US text, so the key is `Hide { $app }`:

- `kiln_command::set_label_arg` gives every command title the argument;
- kiln sets `app` from `AppInfo` in `contributions::shipped`, which runs
  before any menu label resolves;
- a title that doesn't format, such as one using `$app` before the app sets
  it, falls back to its registered title.

kiln's registry test checks every kiln command, with no exclusion (review F1).

## Smaller choices

- **The registry check** is `kiln_command::l10n::check_command_keys`, behind
  kiln-command's `lint` feature, a dev-dependency only, as kiln-l10n's lint
  is. Each crate with commands runs it on its own commands.
- **kiln-command re-exports the catalogue** as `kiln_command::l10n::catalogue`.
  kiln-platform's unit tests use it to register a fake locale, so kiln-platform
  needs no new dependency.
- **`kiln-command-toggle`** belongs to kiln-command, in a `## public` group,
  because kiln uses it.
- **Fake locales in tests** are `Resources` built by hand, with inline sources,
  submitted through `inventory`. They need no fixture files outside the test,
  and the root catalogue checks, which read from disk, never see them.
