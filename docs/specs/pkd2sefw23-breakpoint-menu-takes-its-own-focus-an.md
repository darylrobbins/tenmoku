---
task_uid: "01m7kthmb2y2eqpypkd2sefw23"
task: "T670"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "While the breakpoint menu is open its own FocusHandle (tracked by the menu panel, with its own key context carrying HOLDS_KEYS and its own on_key_down) has focus, the editor's focus handle does not, and no input handler is registered for the menu"
    required: true
    manual: false
  - id: "C2"
    text: "Opening the menu ends any composition explicitly at open (marked_text_range None, buffer text unchanged), not left to on_blur"
    required: true
    manual: false
  - id: "C3"
    text: "Escape, choosing an item (focus restored before the item's deferred run), a mouse-down outside and a stale close give focus back to the editor; a close without a window gives it back at the next render; the existing breakpoints_tests menu tests pass"
    required: true
    manual: false
  - id: "C4"
    text: "opening_the_breakpoint_menu_mid_composition_settles_it_and_the_menu_gets_the_keys (crates/fe-gpui/tests/breakpoint_menu_ime.rs, base APIs only, modelling AppKit's composing key routing) fails at base and passes with the fix"
    required: true
    manual: false
---

# Breakpoint menu takes its own focus: an input method composing in the editor can't commit text or take the arrows while it's open (T1881 F7) (Mac T1888)

Review follow-up of Mac T1881 (helium batch e265bc83d, "Debug gutter follow-ups"), finding F7 (medium, macOS only).

**F7** crates/fe-gpui/src/view/breakpoints.rs, `EditorView::on_right_down` (~line 410): opening the breakpoint menu calls `window.focus(&view.focus, cx)`, so the editor keeps focus while the menu is open. The editor element registers its input handler for that focus (crates/fe-gpui/src/element.rs, `window.handle_input(&focus, ElementInputHandler::new(..))`, gated only on `accepts_text`), so the window's input handler stays the editor's. The menu reads its keys from the editor's own `on_key_down` (crates/fe-gpui/src/view.rs, the `breakpoint_menu_key` call), with `kiln_platform::keys::HOLDS_KEYS` added to the editor's key context while it's open.

On macOS that doesn't cover an active composition. In gpui-pre-macos 0.3.9 `handle_key_down` (src/window.rs), when the window's input handler reports marked text (`is_composing`), and for any key with no `key_char` (arrows, Escape), the key goes to `[inputContext handleEvent:]` before GPUI's KeyDown. So with a composition open when the menu opens, Enter commits the marked text into the file, the arrows move the IME's candidates instead of the menu's item, and neither `HOLDS_KEYS` nor `breakpoint_menu_key` ever sees them.

Fix, in crates/fe-gpui/src/view/breakpoints.rs and crates/fe-gpui/src/view.rs:
- **Own focus target.** The menu gets its own `FocusHandle` (made on first open and kept in `Breakpoints`, as kiln-shell's dock ⋯ menu does in kiln/crates/kiln-shell/src/dock.rs, `DockState::open_menu` / `close_menu` with `menu_focus` and `menu_return`). The menu panel in `breakpoint_menu_layer` tracks it, carries its own key context with `HOLDS_KEYS` (a context no binding names, so kiln's interceptor doesn't match editor bindings first: the swapped-context pattern of crates/fe-workbench/src/project_panel_menu.rs), and reads keys in its own `on_key_down` through `breakpoint_menu_key`. Nothing calls `handle_input` for it, so while it has focus the window has no input handler and AppKit's `hasMarkedText` / `markedRange` answer none. The editor's `HOLDS_KEYS` flag and its `on_key_down` call to `breakpoint_menu_key` go, unless still needed for a path the menu's focus doesn't cover.
- **Settle the composition when it opens.** Opening the menu ends any composition the way the editor's blur already does (`cancel_composition` in crates/fe-gpui/src/view/ime.rs: dropped, buffer untouched), explicitly at open, not left to `on_blur`, which runs only during a draw of an active window.
- **Give focus back on dismissal.** Escape, choosing an item, a mouse-down outside, and the menu going stale (an edit, a Save As, new marks or handlers) close it and return focus to the editor if the menu's handle had it. Choosing an item restores focus before its deferred `run`, so an item that moves focus (a later condition field, T1861) keeps that move. A close with no window at hand (`set_breakpoints`, `set_breakpoint_handlers`) gives focus back at the view's next render while the menu's handle still has it. Focus leaving the menu for anything other than the editor closes it, leaving focus where it went.
- Doc comments that say focus stays on the editor while the menu is open (`breakpoint_menu_key`, `breakpoint_menu_open`) get updated to match.

The in-crate menu tests in crates/fe-gpui/src/view/breakpoints_tests.rs (`escape_closes_the_breakpoint_menu` asserts the editor has focus again; `the_breakpoint_menu_takes_arrows_and_enter`; `typed_keys_leave_the_open_breakpoint_menu_and_the_text_be`; `vim_normal_mode_keys_do_not_reach_the_editor_behind_the_menu`) keep passing. Adjust them only where they assumed the editor itself is focused while the menu is open.

**Regression test.** A new integration test binary crates/fe-gpui/tests/breakpoint_menu_ime.rs (fe-gpui's tests/*.rs are each their own binary, auto-discovered; nothing to declare). It uses base APIs only: `EditorView::new`, `set_breakpoint_handlers` with `BreakpointHandlers` / `BreakpointMenuItem`, `breakpoint_menu()`, `drawn_gutter()`, `caret_bounds_at`, `model()`, `gpui::EntityInputHandler` and `Focusable`, with the editor/draw/right-click helpers copied from crates/fe-gpui/tests/breakpoint_gutter.rs and crates/fe-gpui/src/view/breakpoints_tests.rs (`open_menu`, a two-item host menu that records chosen items).

GPUI's test platform can't reproduce AppKit's ordering: `Window::dispatch_keystroke` always runs KeyDown first and offers only unhandled keys to the platform input handler, and tests can't reach the `TestWindow`'s input handler. A composition itself is headless-simulable through `EntityInputHandler::replace_and_mark_text_in_range` on the view (as crates/fe-gpui/tests/non_latin_text.rs does). So the test models the macOS routing in a small helper, `mac_key(view, key, cx)`, citing gpui-pre-macos `handle_key_down`: if the editor's focus handle is focused (the only case where the editor's input handler is the window's) and `marked_text_range` is `Some`, the IME takes the key. Enter commits through `replace_text_in_range(None, <marked text>)`, and Up/Down change the candidate through `replace_and_mark_text_in_range`, with no KeyDown. Otherwise the key goes through `cx.simulate_keystrokes`.

`opening_the_breakpoint_menu_mid_composition_settles_it_and_the_menu_gets_the_keys`:
1. A focused editor with breakpoint handlers. Start a composition (`replace_and_mark_text_in_range(None, "にほ", None, ..)`) and assert it's marked.
2. Right-click row 3's number. The menu opens (`breakpoint_menu().is_some()`), `marked_text_range` is `None`, the text equals the text before the composition, and the editor's focus handle is not focused.
3. `mac_key("down")` then `mac_key("enter")`. Item 1 ran once, the text is unchanged (no "にほ", no newline), the menu is closed, and the editor's focus handle is focused again.
4. Reopen, `mac_key("escape")`. The menu closes, the editor is focused, and a fresh composition then committed through `replace_text_in_range` inserts its text (the editor accepts input again).

At base, step 2's `marked_text_range` check fails (the editor keeps its composition and focus). The failing check must be an `assert_eq!` or a plain `assert!` so the base-run panic text starts with "assertion" (not `assert!(c, "msg")`).

Not in scope: telling AppKit's input context to discard its own marked state. GPUI 0.3.9 exposes no `discardMarkedText`, and GPUI isn't vendored. With no input handler, AppKit's NSTextInputClient queries see no marked text, which is what the menu relies on. A running-app check with a Japanese IME belongs with T436.

**Helium filing note.** rgt finish overlays the candidate's test files (`*_tests.rs`, `tests/**`) onto base to run the regression test, so every edit to crates/fe-gpui/src/view/breakpoints_tests.rs and the new tests/breakpoint_menu_ime.rs must compile against base APIs (no new accessors), or the base build fails and finish ends as needs_human:infra. Starts after T668 (completion as you type) lands, which also edits view.rs; keep view.rs edits minimal.
