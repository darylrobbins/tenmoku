---
task_uid: "01m4ssydppxg3fpqbbnp4csxxp"
task: "T93"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell test tooltip_text_follows_replacement_while_waiting: an item hovered for 200 ms and then replaced with the same id, tooltip '2 errors' changed to '1 error', shows the '1 error' tooltip and not '2 errors' after the delay"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-shell test tooltip_text_follows_replacement_while_shown: replacing the item while '2 errors' is shown, without moving the pointer, shows '1 error' and not '2 errors'; once the pointer leaves, neither is shown"
    required: true
    manual: false
  - id: "C3"
    text: "The workspace builds; cargo test -p kiln-ui -p kiln-shell -p fe-workbench passes (status_style included); cargo clippy --workspace --all-targets -- -D warnings is clean; no screenshot golden changes"
    required: true
    manual: false
---

# Show a status item's new tooltip text when it is replaced while its tooltip is pending or shown in kiln-ui (T75 review F1) (Mac T598 follow-up)

Follow-up to T75 (landed on main as 2d7f544, "Let status bar items show an icon, a tone, a filled segment and a
tooltip in kiln-shell and kiln-ui (Mac T598)"), review finding F1 (codex, medium).

## Problem

Review of T75, F1 at `kiln/crates/kiln-ui/src/status_bar.rs:243` (the `.when_some(self.tooltip, …)` arm of
`impl RenderOnce for StatusEntry`):

> Replacing an item with the same ID but different tooltip text preserves GPUI's tooltip state. A pending show
> retains the old builder, and a visible tooltip retains the old view, so updating "2 errors" to "1 error" can
> continue showing "2 errors" until the pointer leaves. The new visual test only exercises a static item's tooltip.

Why: the entry's element id is `ElementId::Name(id)`, so GPUI keeps its `active_tooltip` element state across
frames. In gpui-pre 0.3.7 (`src/elements/div.rs`, `handle_tooltip_mouse_move` / `show_tooltip`), the delayed-show
task captures the `build_tooltip` closure of the frame it was scheduled in, and `ActiveTooltip::Visible` holds the
`AnyView` that builder made. `StatusEntry`'s builder creates a new `StatusTooltip` (and a new `Tooltip`) from the
text it captured, so a re-render with new text changes neither: the pending show builds the old text, and a
visible tooltip keeps drawing the old view. T76 (Mac T599) makes the error and warning counts' tooltips change as
diagnostics arrive, exactly this case.

## Change

`kiln/crates/kiln-ui/src/status_bar.rs` only. The tooltip view becomes one entity per entry that the builder
returns and each render updates, so whatever GPUI holds (a pending builder or a visible view) shows the current
text:

- In `StatusEntry::render`, when the entry has a tooltip, get its `StatusTooltip` with
  `window.use_keyed_state(ElementId::NamedChild(Arc::new(ElementId::Name(id.clone())), "tooltip".into()), cx, …)`
  (render takes `window` instead of `_`), created from the text on first use. The keyed state observes the
  entity, so its notify re-renders the bar.
- `StatusTooltip` keeps its `id` and `text` (not a prebuilt selector) and its `tooltip: Entity<Tooltip>`. When the
  render's text differs from the entity's, update the entity: set `text`, replace `tooltip` with
  `Tooltip::new(text, None::<SharedString>, &ui_theme(cx))`, and `cx.notify()`. Equal text changes nothing (no
  notify, so no render loop).
- `StatusTooltip::render` builds the debug selector `status-tooltip:{id}:{text}` from the current fields.
- The `.tooltip(…)` builder returns a clone of that entity (`.into()`), never a new one.

An entry whose tooltip is unchanged draws and behaves exactly as now: same show delay, same dismissal when the
pointer leaves. No change to kiln-shell, `StatusItem`, the element id, or anything visual; no screenshot golden
changes.

## Tests

New integration test file `kiln/crates/kiln-shell/tests/status_tooltip_replace.rs` (no mod.rs edit), with a bench
view like `status_style.rs`'s `Bench` whose items a test can replace (`view.update(cx, |b, cx| { b.items = …;
cx.notify() })`), and an item `StatusItem::new("errors", StatusSide::Left, 0, "2").icon(Icon::CircleX)
.tone(StatusTone::Error).tooltip("2 errors")` replaced by the same id with text "1" and tooltip "1 error".
Query with `cx.debug_bounds` on `status-tooltip:errors:<text>` as `status_style::icon_and_tooltip_draw` does.

- `tooltip_text_follows_replacement_while_waiting`: hover the item's centre, advance the clock 200 ms (under the
  delay; no tooltip yet), replace the item, `run_until_parked`, advance 600 ms, `run_until_parked`: the
  "1 error" tooltip is shown and the "2 errors" one is not.
- `tooltip_text_follows_replacement_while_shown`: hover, advance 600 ms until "2 errors" is shown, replace the
  item without moving the pointer, `run_until_parked`: "1 error" is shown and "2 errors" is not. Then move the
  pointer off the bar's items and advance 600 ms: neither is shown.

Both fail at the base (the old text stays) and pass with the fix.
