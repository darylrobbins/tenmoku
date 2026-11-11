---
task_uid: "01m75ghf4fpfhdkxkygacjhzgv"
task: "T586"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Regular tabs, pinned tabs and close buttons each handle an AccessKit Click with on_a11y_action, calling the same activation or close handler as a mouse click directly with a keyboard ClickEvent (click_count 1), not through GPUI's coordinate fallback. The close button's Click doesn't activate its tab. The close button's wrapper (its Button node and listeners) stays painted at rest, with only its visual content hidden until hover (checked by reading the code: gpui-pre 0.3.9 test windows can't deliver AccessKit actions)"
    required: true
    manual: false
  - id: "C2"
    text: "tests::tabs::an_inactive_clean_tabs_close_button_closes_it_without_hovering in kiln/crates/kiln-ui/src/tests/tabs.rs: with no mouse move first, a click at an inactive clean tab's close box closes that tab and doesn't activate it. It fails at the base and passes"
    required: true
    manual: false
  - id: "C3"
    text: "The existing tab tests still pass unchanged: kiln-ui tests::tabs, kiln-shell tests/tab_icon_close.rs (tab-close/tab-dirty debug bounds drawn only when shown), fe-workbench tests::pane_controls_b3 and pane_pin_tests"
    required: true
    manual: false
  - id: "C4"
    text: "The finish notes state the known limit (no test can deliver an AccessKit action, so the direct handlers and the offscreen-tab case go untested) and list human-check lines for the A11Y-11 VoiceOver pass: VoiceOver's press on 'Close b.txt' for an inactive clean tab closes it; pressing an offscreen tab and an offscreen pinned tab activates each"
    required: true
    manual: false
---

# Tabs: a screen reader's Click reaches the tab or close button it names (Mac follow-up of T1670, T581 F1)

# Tabs: a screen reader's Click reaches the tab or close button it names (Mac follow-up of T1670, T581 F1)

Follow-up to T581 (Mac T1670, A11Y-4 Tabs and project tree, e9038419), from its review finding F1 (medium,
review 01m75cgjc1ayrs4gncsd4n3xb6), kiln/crates/kiln-ui/src/tab_bar.rs:469, :523, :576.

## The problem

T581 made each tab a `Tab` node and each close button a `Button` node ("Close b.txt"), but neither handles
an AccessKit `Click` itself. gpui-pre 0.3.9 then falls back to a synthetic mouse down and up at the node's
centre (`Window::handle_a11y_action`, gpui-pre src/window.rs:7056-7073). That goes wrong in two ways:

1. **An inactive clean tab's close button.** At rest it is `.invisible()` (`hidden_at_rest`, tab_bar.rs:479,
   :493-497), shown only on group hover. Div paint returns early for `Visibility::Hidden`
   (gpui-pre src/elements/div.rs:2535), before it registers mouse listeners or `on_a11y_action` listeners
   (div.rs:2600-2612). The fallback's down/up comes with no mouse move, so the tab's own click handler gets
   it: "Close b.txt" activates b.txt instead of closing it.
2. **An offscreen tab, or its close button.** Its node is exposed, but its centre lies outside the strip's
   visible area, so a click there can't reliably reach it.

## The fix

In kiln/crates/kiln-ui/src/tab_bar.rs:

- Give the regular tab, the pinned tab (`render_pinned`) and the close button an
  `.on_a11y_action(accesskit::Action::Click, …)` that calls the same handler as `on_click` / `on_close`
  directly. Share each handler between the mouse and a11y paths (for example `Rc` instead of the boxed
  `ClickHandler`). Pass a keyboard `ClickEvent` (`ClickEvent::default()`): its `click_count()` is 1, so
  kiln-shell's pane (src/pane.rs:715, double click pins) doesn't pin on a screen reader's Click. The close
  button's a11y Click must not also activate the tab.
- Keep the close button's wrapper (the `Button` node with the listeners) painted at rest, and hide only its
  visual content until group hover. GPUI records a div's own debug bounds before its visibility check
  (div.rs:2526-2535). So put the `invisible()` / `group_hover(visible)` on an intermediate div *above* the
  `tab-close:<label>` / `tab-dirty:<label>` debug-selector div. Then those bounds still appear only when the
  mark is shown, which these existing tests rely on: kiln-shell tests/tab_icon_close.rs
  `only_the_active_or_hovered_tab_draws_its_close_button`, fe-workbench src/tests/pane_controls_b3.rs
  `a_dirty_tab_shows_its_dot_and_a_pinned_tab_its_icon`, and src/pane_pin_tests.rs.
- Clicking the hidden close mark of an inactive clean tab now closes it, as it does today once hovered.
  Pointer users get there by hovering anyway, so the pointer behaviour doesn't change.

## Tests

**Regression test** (assert-style, in the existing kiln/crates/kiln-ui/src/tests/tabs.rs, using only
APIs that exist at the base: the `Host`/`Tabs` fixture, `tab_bounds`, `simulate_click`):
`an_inactive_clean_tabs_close_button_closes_it_without_hovering`. With tab one active, `simulate_click`
at the centre of tab three's close box. Find the centre from `tab_bounds(&host, 2, cx)` as the existing
test does (`right() - px(15.)`), not from `debug_bounds`, which is absent at rest. Send no mouse move
first: `simulate_click` is exactly the down/up pair GPUI's Click fallback dispatches. Assert the events
are `[TabsEvent::CloseRequested(2)]`, with no `Activated(2)`. At the base it gets `[Activated(2)]`.

**What can't be tested here (a known limit; say so in the finish notes).** A test can't deliver an AccessKit
action through gpui-pre 0.3.9. The test window's `a11y_init` is the trait's no-op (gpui-pre
src/platform.rs:1387), so a test window's a11y is never active. Div paint registers `on_a11y_action`
listeners only while it is active (div.rs:2600). `Window::handle_a11y_action` is `pub(crate)`
(window.rs:7034). So the direct handlers, and the offscreen-tab case, are checked by reading the code and by
the A11Y-11 VoiceOver pass. Keep test-only seams out of non-test files: a test in kiln-ui's lib test binary
that uses an API the fix adds won't compile at the base, and the regression check then fails as infra.

Out of scope: kiln/crates/kiln-ui/src/a11y.rs and its recorder (T579, parked).
