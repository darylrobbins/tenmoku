---
task_uid: "01m7ac1syv1nxdyvc0xhka4r8m"
task: "T607"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::notification_inbox (cargo nextest run -p fe-workbench) pass: every_pushed_toast_is_recorded, merged_duplicates_merge_in_the_inbox, needs_you_while_it_has_an_action, acting_or_dismissing_moves_it_to_earlier, answering_choices_moves_it, the_cap_of_200_drops_the_oldest_earlier_first"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench toast passes (merging, lifetime, errors staying, choices and dismiss all unchanged)"
    required: true
    manual: false
---

# Notification inbox model: every toast kept, split into Needs you and Earlier (Mac T1582)

# Notification inbox model: every toast kept, split into Needs you and Earlier

Spec: `docs/design/design-system.md#notifications-and-agents` (Inbox: "Two
groups: Needs you (tinted rows, each with one action) and Earlier"). Toasts
today vanish (`crates/fe-workbench/src/toast.rs`: `ToastQueue`, six-second
lifetime, errors stay) and leave no record.

## Change (headless)

- `crates/fe-workbench/src/notification_inbox.rs` (new): `NotificationInbox`,
  owned by the window's `ToastLayer`: every pushed `Toast` becomes a
  `Notification { id, level, source, title, detail, action, at, state }`
  (`source`: App, Lsp, Task, Git, Agent, from the toast's creator via a new
  optional `Toast::source`). Duplicates the queue merges (x N) merge here too.
  A notification **needs you** while it has an action or unanswered choices
  and has not been acted on or dismissed; otherwise it is Earlier. Acting,
  dismissing or answering moves it. Capped at 200 (oldest Earlier dropped
  first). Count of needs-you for the bell.
- `crates/fe-workbench/src/toast.rs`: `ToastLayer::push` records into the
  inbox; nothing else about toast behaviour changes (merging, six seconds,
  errors stay, Choices, dismiss all).

## Not in this card

The bell, the popup, Focus mode, the level setting, persistence across
restarts.

Platform: **Linux-OK**.
