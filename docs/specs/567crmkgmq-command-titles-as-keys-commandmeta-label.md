---
task_uid: "01m6vvsc6ddj9zc3567crmkgmq"
task: "T1523"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The registry test passes: every command of a migrated crate has a command-\u003cid\u003e key and every command-\u003cid\u003e key has a command (cargo nextest run -p kiln-command)"
    required: true
    manual: false
  - id: "C2"
    text: "A fake-locale test shows the palette sorting and matching on label() (cargo nextest run -p kiln-shell command_palette)"
    required: true
    manual: false
  - id: "C3"
    text: "Every existing call site of Picker::new and Button::new compiles unchanged (the transitional From impls); cargo nextest run -p kiln -p kiln-shell -p kiln-ui passes"
    required: true
    manual: false
---

# Command titles as keys (CommandMeta label/detail) and the label constructors take Text: Picker::new and Button::new (i18n I3)

# Command titles as keys, and the label constructors take Text (I3)

Spec: card **I3** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for I3 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- The plan's "not alongside P18" no longer applies (the palette is still to
  design in `docs/design/design-system.md#still-to-design`; P18 is not filed).
- `kiln/crates/kiln-ui/src/controls.rs` was just restyled by the B3 controls
  card; change only `Button::new`'s parameter type and its callers inside kiln.
- `TypedCommand::label()` (§ 8 table) is in this card: `kiln/crates/kiln-platform/src/command_actions.rs`
  is in scope. Test translated resolution and the silent fallback through it in that file's unit tests
  (review F4).
- **Locale changes are not in this card** (I2's `ActiveLocale`, T1522, has not landed): the menu bar
  re-resolving on a locale change (`resolve_again` on `ActiveLocale`, review F2) and an open picker's
  placeholder following a locale change (`Picker` keeping its `Text`, which needs
  `kiln-ui/src/text_input.rs`, review F3) moved to **T1620**, which depends on T1522 and this card.
  Here labels resolve when a menu is built or a picker or palette opens, and tests set the locale
  before building the app. Note the deferral in the decision file.

Platform: **Linux-OK**.

## Implementation notes

- **The resolver** is `kiln/crates/kiln-command/src/l10n.rs`: `command_key`,
  `command_label`, `command_detail` and `derive_label`, re-exported at
  kiln-command's root, with `CommandMeta::label()` and `detail()`. A miss is
  silent (`try_format`, never `tr!`). kiln-command owns
  `kiln-command-toggle = Toggle { $title }`, in a `## public` group, as kiln
  uses it.
- **The registry check** is `kiln_command::l10n::check_command_keys`, behind
  kiln-command's `lint` feature (dev-dependency only, like kiln-l10n's).
  kiln-command's own test (`tests/l10n.rs`) pins it on a fixture. kiln-shell
  (`tests/command_palette.rs`) and kiln (`tests/l10n_catalogue.rs`) run it on
  their real commands: the typed ones their `register` adds, plus the
  picker's registry.
- **Keys moved:** kiln-shell's 17 command titles (the picker's and its
  input's 15, `menu_bar.open`, `command_palette.toggle`); kiln's 7 typed app
  and window commands; the toggle message. The picker's registry lost its
  title literals and registers each command with `command_label(id, id)`.
- **`app.hide` is `command-app-hide = Hide { $app }`** (review F1). Command
  titles take process-wide label arguments (`kiln_command::set_label_arg`).
  kiln sets `app` to the app's name in `shipped`, before it resolves any menu
  label. A title that doesn't format, such as one using `$app` before the
  app sets it, falls back to its registered title. So the palette and
  menus show `Hide <app>`, as kiln-platform's registry title already did, and
  kiln's registry test checks every kiln command, with no exclusion.
- **Scope gaps from the plan's I3 row.** This card's scope has no kiln-platform
  and no `kiln/tests/l10n_command_labels.rs`, and I2 hasn't landed:
  - `TypedCommand::label()` is added in rev 3 (review F4), with a unit test
    in `command_actions.rs` (`label_tests`). The palette and the menu table
    read it.
  - kiln doesn't yet re-resolve its menus on a locale change. Rev 3 moves
    that to T1620, as `docs/decisions/567crmkgmq-command-title-keys.md`
    records.
  - The menu fake-locale checks (`place_items`, the menu table, a keyless
    command) live in `kiln/tests/l10n_catalogue.rs`. The palette's check
    lives in kiln-shell's `tests/command_palette.rs`.
- **`Picker::new`** resolves its `Text` when the picker is made. Its
  `TextInput` (kiln-ui `text_input.rs`, outside this scope) stores a
  `SharedString`, so an open picker keeps its placeholder across a locale
  change. **`Button::new`** resolves at construction; buttons are rebuilt
  every render. Every existing caller compiles unchanged, and
  `cargo clippy --workspace --all-targets` is clean.
- **kiln-ui** gains only the `kiln-l10n` dependency. It has no strings of its
  own here, so it has no `l10n.rs` yet. No crate's literal lint is switched on
  (K2), so no `l10n-allow.toml` is added.
- **Carried T1518 F2** (Tab traversal into controls) isn't fixed. This card
  may change only `Button::new`'s parameter in `controls.rs` (Changes from the
  plan), and the fix belongs in the host's focus infrastructure, outside this
  scope.

### Review round 1 (codex 3qrvn6)

- F1 fixed (above).
- F2 (re-resolve menus on a locale change) needs I2's `ActiveLocale`. I2 is
  T1522, which is blocked (lent to helium), and kiln-platform isn't in this
  scope.
- F3 (an open picker following a locale change) needs `TextInput` to keep or
  take a `Text`: `kiln/crates/kiln-ui/src/text_input.rs`, which isn't in
  scope. It also needs a locale signal (F2).
- F4 (`TypedCommand::label()`) is in `kiln/crates/kiln-platform/src/command_actions.rs`,
  which this card's scope left out, though the plan's I3 row had it.

### Rev 3

- F4: `TypedCommand::label()` is done.
- F2 and F3 move to T1620 (`docs/decisions/567crmkgmq-command-title-keys.md`).
- F1's fix is kept.

### Rev 3 review round 1 (codex sqhcrx)

- Prior F1 and F4 were verdicted fixed.
- F2 and F3, re-raised, are disputed (`rgt review dispute`): rev 3 defers
  both to T1620, as the decision file records.
