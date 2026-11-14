# T1583: notification bell and inbox popup

Spec: `docs/specs/y1q4bzhva6-notification-bell-in-the-status-bar-and.md`.

## What it does

- **kiln-shell `StatusItem`** gains `dot: bool` (a 6 px `accent.base` dot
  over the icon's top right, debug selector `status-dot:{id}`) and
  `name: Option<SharedString>` (an accessible name, as a button when the
  item has a command). An item with neither draws exactly as before, with no
  wrapper element, so no other item's layout or element ids move.
- **The bell** is `StatusItem::NOTIFICATIONS`, priority 100 000, last on the
  right, an icon with no text. `StatusBar` holds only the needs-you count
  (`set_bell`); the item, its name and tooltip
  (`workbench-inbox-bell`, plural with a `[0]` case) are built as the bar
  renders, so a locale switch re-words them.
- **The popup** (`notification_inbox_view.rs`) is an entity beside the
  toast layer. It observes the layer, which notifies whenever the inbox
  changes, and keeps the bell's count in step. Open, it is drawn at the
  window's lower right, 8 px in, 4 px above the status bar, and the toast
  column is not drawn: the toasts are its rows. It takes focus in key
  context `notification_inbox`; Escape, Up, Down and Enter are handled in
  its `on_key_down` (no default-keymap binding claims them in the
  workbench's context, and Escape/arrows/Enter are left to the popup as the
  tab switcher and quickfix panel do). Focus leaving it closes it.
- Rows: Needs you then Earlier, newest first. Needs-you rows are on
  `state.selected`, an agent's on `agent.soft`; the icon is the agent's
  robot in `agent.base`, else the source's icon (Git branch, task run,
  language server, or the level's own glyph for the app) in the level's
  colour. Only Needs-you rows have the action button. The keyboard row
  draws `state.focus_ring`. No other accent.
- Acting on a row closes the popup, gives focus back, and runs the command
  from there (`toast::run_action`, as a toast's action does), then resolves
  the notification `Acted`; its toast goes if still shown. Dismiss on a
  Needs-you row resolves it `Dismissed` (to Earlier); on an Earlier row it
  removes it. Clear earlier removes every Earlier one.
- `notifications.show` (palette, "Show Notifications") toggles the popup;
  the bell's click runs it.

## Scope beyond the card's list

- `notification_inbox.rs` (T1582): `toast_of`, `remove`, `clear_earlier`;
  its stale `allow(dead_code)` removed now the popup reads it.
- `toast.rs`: `ToastLayer::{resolve_notification, remove_notification,
  clear_earlier}`, `run_action` made `pub(crate)`, the dead-code allow on
  `inbox()` removed.
- `workbench.rs`: the `inbox` field, its construction, the overlay, and the
  toasts hidden while it's open; the module declaration.
- `tests/status_bar.rs`'s `drawn` helper skips the bell (its texts and counts
  pin the editor's items); `tests/status_bar_b3.rs`'s right-side order test
  now ends with the bell, which sits against the bar's edge.
- `Workbench::notify_from` (doc-hidden) for the audit scene, which can't
  reach the crate-private `Toast`.
- `examples/screenshots/golden/workbench.png` changes: the bell is in every
  status bar.

## C4: audit scene `inbox`

Run on the Mac, 2026-11-14: `cargo run -p fe-workbench --example screenshots
-- audit <dir>` wrote `inbox_dark.png` and `inbox_light.png` (Yohen,
Hakuji), and both passed all four probes:

| Probe | Token |
| --- | --- |
| (614, 470), the popup's edge beside the Earlier rows | `surface.raised` |
| (621, 310), the agent's Needs-you row | `agent.soft` over `surface.raised` |
| (621, 403), the task's Needs-you row | `state.selected` over `surface.raised` |
| (617.5, 310), the keyboard row's ring | `state.focus_ring` |

```text
ok      inbox_dark probes (4)
ok      inbox_light probes (4)
```

The bell's dot isn't probed: the popup's float shadow (12 px down, 32 px
blur) covers the whole status bar under it, so no pixel of the dot is the
bare token while the popup is open, and toasts shadow it the same way. The
dot is drawn in `cx.theme().colors.accent.base` (the first run, with the
deprecated `ui_theme(cx).accent`, read the same shadowed colour) and shows
in the captures; `status::tests::a_dotted_item_draws_its_dot_on_the_icon`
pins its size and place.

The same audit run reports 22 other captures failing their probes (chrome
and the bundled themes at (600, 45), settings, shortcuts editor, changes,
Git Log and its card, problems and the problems panel, terminal and its
session). None of them probe the status bar, the one place this card
changes outside its own scene, and they failed identically before the
probes here were adjusted. They aren't this card's, though not re-checked
at the base (that needs a cold build).

Goldens: `workbench.png` and `setup_wizard_{dark,light}.png` regenerated
(`UPDATE_SCREENSHOTS=1`): the bell moved the status bar's right-side items
left. Their diffs also showed two small changes from before this card
(the Project header's `+` in `workbench.png`, and line 9's tail beside the
setup sheet), which the new goldens take in. `file_icons_*.png` passed
unchanged and were left as they were. The comparing run then passed all
six.

After rebasing onto `e5664f5` (T1571's Runs panel, which also changed
`workbench.png`), `workbench.png` was regenerated again (its diff was the
bell alone), the other five goldens passed as they were, and the audit's
`inbox_dark` and `inbox_light` passed their four probes again.

Rebased again onto `f538df1` (T1817 regenerated the goldens, T1768 added
the branch menu's audit scene): `setup_wizard_{dark,light}.png` regenerated
on the merged code (the bell, and T1571's rail icons T1817's goldens
predate); all six goldens pass and `inbox_dark`/`inbox_light` pass their
four probes.

## Review round 1 (codex, changes requested)

- **F1, Enter on a focused button ran the selected row.** Up, Down and
  Enter now act only while the popup itself has focus; a focused button
  takes Enter and Space as GPUI clicks it. Before, nothing reached the
  buttons from the keyboard at all (no binding moves focus by Tab here), so
  the popup now does Tab and Shift-Tab itself, through focus handles it
  owns for each button: from the popup to the selected row's first button,
  then in the drawn order (a row's answers, action, dismiss; Clear earlier
  before the Earlier rows), and back to the popup past either end. The row
  of the focused button is selected, so a button and the selection can't
  belong to different rows; arrows don't move the selection while a button
  has focus. A button that a dismiss or Clear earlier takes away gives
  focus back to the popup first, which would otherwise close with nothing
  focused. Test: `a_focused_button_takes_enter_itself`.
- **F2, questions were dismiss-only.** A notification keeps its question's
  answers (`Notification::answers`), and a Needs-you row draws them as its
  buttons, beside its action, on a line under its text. A click answers
  the live toast's reply through `ToastLayer::answer_notification`, as the
  toast's own button does (reply, then resolve `Answered`, toast gone);
  with the toast gone there's nothing to answer and the row is dismissed.
  Enter answers a question of one answer and moves focus to the first of
  several. Test: `a_question_is_answered_from_its_row`.
- **F3, selection by index.** The selection is a `NotificationId`, with its
  last place kept as the fallback when it goes; row and button elements are
  named by the notification (`inbox-row-{n}`, `inbox-action-{n}`,
  `inbox-answer-{n}-{i}`, `inbox-dismiss-{n}`). Test:
  `the_selection_follows_its_notification` (a new one, a merged duplicate,
  removal).
- **F4, Earlier dismissals left their toasts.** Dismissing an Earlier row
  and Clear earlier dismiss its toasts too, shown or waiting, through
  `dismiss_toast` (timers, focus). Test: `dismissing_earlier_takes_its_toasts`.
- **F5, stale ages.** While open, a timer redraws the popup when the first
  age steps on (the next minute, hour or day), rescheduled on each change,
  dropped on close. Ages are measured on the executor's clock from the wall
  clock at opening, so tests drive them. Test: `ages_count_on_while_open`.

After round 1's fixes the Needs-you rows carry their buttons on a second
line, so the popup grew upward and the agent row's two probes moved from
y 350 to y 310 (the table above has the new points); the audit then passed
`inbox_dark` and `inbox_light`, four probes each, with the same 22 other
captures failing as before.
