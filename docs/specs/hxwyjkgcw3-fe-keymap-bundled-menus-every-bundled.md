---
task_uid: "01m71sfx0hy03kn8hxwyjkgcw3"
task: "T318"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test every_bundled_title_resolves_to_todays_english (cargo nextest run -p fe-keymap) passes; the bundled menu test rejects a literal title in a bundled file"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-keymap --test l10n passes"
    required: true
    manual: false
---

# fe-keymap bundled menus: every bundled title as a title_key, per-platform copies included (i18n KM1) (Mac T1533)

# fe-keymap bundled menus: title_key for every bundled title (KM1)

Spec: card **KM1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 (Migration plan), with sections 3–8 as the
design, as amended by T1506 (sections 4, 8 and 11). Read the row for KM1 and the
sections it cites; build exactly that, with the changes below.

Common to every migration card (§ 9): en-US text stays byte-equal to today's,
so no golden and no behaviour test changes except tests that construct removed
literals. Keys are crate-prefixed, whole messages, named arguments, plural
selectors for counts (§ 5).

## Changes from the plan

- The plan waits for the audit's P4/P5; they are not filed.

Platform: **Linux-OK**.

## Implementation notes

- `menus/fastedit.toml` names every menu and item by `title_key`, in
  `crates/fe-keymap/l10n/en-US/keymap.ftl` (`keymap-menu-*`, one key per
  distinct English title, 124 messages). The app menu is
  `keymap-menu-app = { $app }`, and Hide, Quit and Set Up take `$app` too.
  The Services item drops its `title` for kiln-core's default,
  `kiln-core-menu-services` (also "Services").
- `build.rs` is unchanged in behaviour: it copies `title_key` as is, so
  `DEFAULT_MACOS_MENUS` and `DEFAULT_LINUX_MENUS` are keyed too. Its doc
  comment says so.
- `default_menus_for` (and so `default_menus_macos` and `_linux`) sets
  `$app` to Tenmoku's display name (`menu::set_label_arg`) before it parses,
  as kiln's `Config` does. So callers that parse the bundled menus without
  kiln (fe-workbench, fe-gpui and crates/fastedit tests) still see
  "Hide Tenmoku" and "Quit Tenmoku".
- `tests/l10n.rs`: `every_bundled_title_resolves_to_todays_english` parses
  the table and both copies on macOS, Linux and Windows. It rejects any
  menu or item without a `title_key`, checks each label resolves to the
  en-US title, and compares every title with the pre-change English, which
  was dumped from the literal table before the change and is pinned in the
  test. `a_literal_title_in_a_bundled_file_is_rejected` shows the rejection
  on a fixture.
- The literal lint covers `src/l10n.rs`, `src/lib.rs` and
  `src/shortcuts_model.rs` (`migrated` in `l10n-allow.toml`).
  `src/keymap_edit.rs` stays unmigrated: its 22 literals are the keymap
  edit errors and `KEYMAP_HEADER`, a `pub const` that fe-workbench writes
  and compares against. Moving them changes that API outside this card's
  scope, so a follow-up card should do it.
