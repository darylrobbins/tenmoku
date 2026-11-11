---
task_uid: "01m7595aedfq24n49xy7bpe2n5"
task: "T583"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Each editor and chrome gap in section 7's table is reachable by keyboard as it specifies: toast focus, the prompt's Tab order, the code lens picker, editor.apply_inlay_hint, the breadcrumb pickers, tab reorder and the view-stack section commands"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in crates/fe-workbench/tests/a11y_reach_chrome.rs, kiln/crates/kiln-shell/tests/view_stack_sections.rs, kiln/crates/kiln-ui/src/tests/prompt.rs and crates/fe-keymap/tests/shortcuts_model.rs drive each by keys and pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-16a Keyboard reach, editor and chrome: toasts, prompt Tab order, code lenses, inlay hints, breadcrumbs, tab reorder and view-stack sections (T294 design) (Mac T1678)

# A11Y-16a Keyboard reach: editor and chrome

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-16a Keyboard reach: editor and chrome** row of section 11 "Proposed card breakdown", and section 7 "Keyboard reachability" (the editor and chrome rows).
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Implementation notes

Each editor and chrome gap of section 7's decision table, as built:

| Gap | Built | Tests |
|---|---|---|
| Toast choice buttons, action button, close × | `toast.focus` ("Toast: Focus", palette-listed, unbound) focuses the newest toast's first button. The focused toast's card is key context `toast`, where `tab`, `shift-tab` and `escape` are bound to `toast.next_button`, `toast.previous_button` and `toast.dismiss` (palette = false). The buttons are kiln `Button`/`IconButton`s given the layer's focus handles: Enter or Space presses the focused one (GPUI's keyboard click), and it draws kiln's focus ring. Tab order: the answers or the action, then ×. The toast's timer holds while it has focus; when it goes (pressed, dismissed, closed) focus returns to where it was before `toast.focus`, and an action runs there | `a11y_reach_chrome.rs` `toasts::*`; keys in `shortcuts_model.rs` |
| In-window prompt, third button | Tab and Shift-Tab move focus between the prompt's buttons, wrapping, from the default one; Enter or Space presses the focused one, which draws a ring; Escape still cancels. Carried T550 F3: the cancel button is resolved once, when the prompt opens | `kiln-ui/src/tests/prompt.rs` |
| Code lens items | `lsp.run_lens` is palette-listed as "Code Lens: Run…": run with no `lens` arg it opens a picker over the lenses on the caret's row (`EditorView::lens_items_at_caret`); choosing one runs it from the editor as a click does | `a11y_reach_chrome.rs` `lenses::*` |
| Inlay hint double-click | `editor.apply_inlay_hint` ("Apply Inlay Hint", a workbench command in the editor's context) applies the hint at or nearest the caret on its line, as a double-click does: at once if resolved or there's no host, else once resolved. A resolved hint with no edits is passed over | `a11y_reach_chrome.rs` `inlay_hints::*` |
| Breadcrumb path and symbol crumbs | `workbench.pick_breadcrumb_path` ("Breadcrumbs: Pick from Folder") opens the file crumb's picker, its folder's entries with the file selected; `workbench.pick_breadcrumb_symbol` ("Breadcrumbs: Pick Symbol") the symbol crumbs' quick outline. Both work with the breadcrumbs hidden | `a11y_reach_chrome.rs` `breadcrumbs::*` |
| Tab reorder by drag | `pane.reorder_tab_left` / `pane.reorder_tab_right` ("Reorder Tab Left/Right") move the active tab one place within its strip (`PaneView::reorder_active_item`), staying on its side of the pinned boundary, no wrap | `a11y_reach_chrome.rs` `reorder::*` |
| View-stack header collapse | kiln-shell typed commands in key context `view_stack`: `view_stack.toggle_section` toggles the section holding focus (header or view), leaving focus on its header; `view_stack.focus_next_section` / `focus_previous_section` focus the next/previous header, wrapping, past the headerless lead. Each section's element handles them, so they reach the section on focus's path. Registered by `kiln_shell::register` and in `kiln_shell::catalog` | `kiln-shell/tests/view_stack_sections.rs` |

All new commands are unbound but for the toast's keys, which are in its own
context (section 7: bound only where a sibling surface binds one; the prompt
and pickers bind Tab, Enter and Escape the same way). The view-stack divider
stays deferred, as section 7 decides.

Scope widened by respec for `typed_command_ids_tests.rs` (the "Code Lens:
Run…" title), `kiln-shell/src/lib.rs` (register and catalog the view-stack
commands) and `kiln-shell.ftl` (their titles).

### Review round 1 (3b84dp)

- F1: the toast's focus session lasts until the toast goes, not until focus first leaves it: a picker opened from a focused toast (Go to File) and cancelled gives focus back to the same button, which is still drawn with the layer's handle. The timer holds only while focus is in the toast (`on_focus_in`/`on_focus_out` on its card). When the toast goes, focus left on it goes back to where it was before `toast.focus`; focus that moved on stays.
- F2: `accept_inlay_hint`, shared by the double-click, the command and a resolution landing late, refuses in a read-only editor (vim's E21 with vim on), and the command refuses up front.
- F3: a focused toast's card forwards `command_palette.toggle` and `menu_bar.open`, which the shell beside the toasts handles, after giving focus back.
- F4: the pointer's hover and keyboard focus each hold a toast's timer; it runs only when neither does.
- F5: action-toast tests in `toast.rs` (`keyboard_tests`): Tab order, labels, Enter running the action in the editor (with `Source::Keyboard`), Space on ×.

### Review round 2 (4ky94p)

- F1: the layer keeps the last `toast.focus`'s restore target past its toast, and listens for focus lost: when focus lands on the toast's undrawn card or buttons (Go to File, cancelled after the toast expired, gives focus back to the button it saved), it goes to that target. Regression: a timed toast expiring under Go to File.
- F6: a press on the action gives focus back only while focus is still in the toast (from the keyboard); clicked after focus moved on, the action runs where focus is. Regression: focus moved to the project panel, then the action clicked.

### Section 12 human checks for A11Y-11

- **H11′. Command reach, keyboard only, VoiceOver off.** For every row of section 7's decision table owned by A11Y-16a or A11Y-16b, run its command from the palette (or its key) and confirm it does what the mouse control does. Deferred rows are skipped. Escape closes, and Return confirms, in every in-window modal in section 7's modal table, including the git log and shortcuts menus. (This card's rows: Toast: Focus, the prompt's Tab order, Code Lens: Run…, Apply Inlay Hint, Breadcrumbs: Pick from Folder / Pick Symbol, Reorder Tab Left/Right, Toggle Section and Focus Next/Previous Section.)
- **H11a′. Focus and focus ring, keyboard only.** Only the surfaces that take focus are checked: a toast after "Toast: Focus" (Tab between its buttons, Space and Return press), list and tree rows, text fields, and the view-stack headers. Each shows the focus ring. Other buttons take no focus in the first slice (Q3), so no ring is expected on them. Repeat with Full Keyboard Access on (as H11a): Space presses a focused toast button, and Space still types in the editor and text fields.
