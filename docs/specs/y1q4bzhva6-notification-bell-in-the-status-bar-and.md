---
task_uid: "01m6w09wwknr1d5gy1q4bzhva6"
task: "T1583"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::notification_inbox_view (cargo nextest run -p fe-workbench) pass: the_bell_dot_shows_only_with_needs_you_items, the_popup_groups_rows_and_runs_a_rows_action, escape_closes_and_arrows_move, the_accessible_name_counts_with_plurals, clear_earlier_empties_that_group"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln-ui passes with the bell icon (tests::assets::bundled_files_match_the_enum) and cargo nextest run -p kiln-shell passes with the status item's dot"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac, the audit scene named in the body renders in both themes and its probes pass (recorded in the decision file) (audit/inbox.rs)"
    required: true
    manual: false
---

# Notification bell in the status bar and the inbox popup (Needs you, Earlier)

# Notification bell and inbox popup

Spec: `docs/design/design-system.md#notifications-and-agents` (Inbox: "opened
from the bell at the right end of the status bar (a rust dot when something
needs you)") and the status bar row ("Right: ... notification bell").

## Change

- The bell: a host `StatusItem` at the right end of the status bar
  (`crates/fe-workbench/src/status_bar.rs`) with a `bell` icon
  (`kiln/crates/kiln-ui/assets/icons/bell.svg`, Lucide, 1.75 stroke, and its
  `Icon` row in `kiln/crates/kiln-ui/src/file_icons.rs`), a 6 px accent dot
  while `NotificationInbox` (T1582) has anything that needs you, and an
  accessible name with the count (a plural message). A small dot field on
  `kiln_shell::StatusItem` (`kiln/crates/kiln-shell/src/status.rs`).
- Click or `notifications.show` (palette) opens the inbox as an overlay above
  the bell: two groups, Needs you (rows on the `selected` surface, the source's icon in its
  colour) and Earlier. A row: the
  source icon in its colour, title, one detail line, age, one action button
  (the notification's action), a dismiss. A "Clear earlier" button. Esc closes;
  arrows move; Enter runs the row's action.
- The agent-source rows use the agent colour; nothing is drawn in the accent
  except the dot and focus.
- An audit scene `audit/inbox.rs`, both themes.

## Not in this card

Focus mode and the level setting (next card), the toast look (T1556).

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-inbox-…` in
`crates/fe-workbench/l10n/en-US/inbox.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**; the audit scene is recorded on the Mac.
