---
task_uid: "01m7dn7487wrd73xc2w6ycxxw1"
task: "T368"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test the_level_parses_and_defaults_to_all (cargo nextest run -p fe-config --test notifications_setting) passes"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in tests::notification_policy (cargo nextest run -p fe-workbench) pass: needs_you_sends_toasts_without_actions_to_the_inbox, off_sends_everything_to_the_inbox_but_errors_still_show_once, focus_mode_holds_non_urgent_toasts_until_20_seconds_idle (fake clock), urgent_toasts_show_at_once"
    required: true
    manual: false
  - id: "C3"
    text: "UPDATE_GOLDEN=1 regenerates the settings schema and the generated settings.ftl and settings-template.ftl; cargo nextest run -p fe-config passes with them"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Focus mode and workbench.notifications (all, needs_you, off): hold non-urgent toasts until the user is idle (Mac T1584)

# Focus mode and the notification level setting

Spec: `docs/design/design-system.md#notifications-and-agents` ("A Focus mode
holds non-urgent items until you are idle") and Settings ("... agent command
permissions and notification level").

## Change

- `workbench.notifications` = `"all" | "needs_you" | "off"` (default `"all"`),
  user setting only: `all` shows every toast as today; `needs_you` shows toasts
  as toasts only when the notification needs you, the rest go straight to the
  inbox; `off` sends everything to the inbox (errors still show once).
  `crates/fe-config/src/settings.rs`, `settings.schema.json`,
  `settings-template.toml`, and the generated catalogue files regenerated
  (`UPDATE_GOLDEN=1`); the Settings page lists it by itself.
- Focus mode: a toggle in the inbox popup footer and the command
  `notifications.toggle_focus` (palette): while on, non-urgent toasts (no
  action, not an error) are held in the inbox and shown as toasts only once
  there has been no key or mouse input for 20 seconds; urgent ones show at
  once. The bell shows a muted slash while it is on. State is per window and
  not persisted.
- `crates/fe-workbench/src/notification_inbox.rs` (T1582) gets the policy;
  `toast.rs` consults it.

## Not in this card

Per-source levels, quiet hours, persisting focus mode, the agent permission
setting (a design card).

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-inbox-…` in
`crates/fe-workbench/l10n/en-US/inbox.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**

## Implementation notes

- The bell is slashed (`bell-off`, added to kiln-ui's lucide set) while Focus mode is on, and its name and tooltip say so (`workbench-inbox-bell-focus`).
- The popup footer toggle is a `ToggleButton` and is not in the popup's custom tab order; `notifications.toggle_focus` is the keyboard route.
- Held toasts are recorded in the inbox at once and shown when the user is idle. `ToastLayer::push` returns a distinct id for a toast that didn't show: it dismisses the notification, or after release the queue's toast, even if the inbox has since trimmed the notification.
- With `off`, a question stays in the inbox and the layer keeps its reply, so the inbox row answers it; an action toast keeps its `action_args` in the notification.
- Held payloads, kept replies and handles are pruned as their notifications go; a level change applies at release.
- Idle time: key down and mouse down (capture), a pointer that actually moved (gpui sends a synthetic move after a redraw), and scrolling through a window-wide capture listener.
