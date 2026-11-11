---
task_uid: "01m7595a082421sbkgvg8x110y"
task: "T582"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The command palette, kiln-shell pickers and modals, the tab switcher and the find bar expose the roles section 6 gives (a dialog with a search input, a list of options with the highlighted row as active descendant, labelled find bar fields and buttons)"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in kiln/crates/kiln-ui/src/tests/list.rs and crates/fe-workbench/tests/a11y_pickers.rs assert those roles and labels and that the active descendant follows the highlighted row as the query refilters and the selection moves; both pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-6 Palette, pickers and find bar: dialog, search input and option list with the highlighted row as active descendant (T294 design) (Mac T1672)

# A11Y-6 Palette, pickers and find bar

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-6 Palette, pickers and find bar** row of section 11 "Proposed card breakdown", and section 6 "Chrome semantics".
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Implementation notes

- **Active descendant, where the design allows it.** Section 6 of the design says focus in a
  picker is in the query, a sibling of the rows, so GPUI can't make a row the active
  descendant (it honours `aria_active_descendant` only under the focused node), and the
  picker announces the selected row instead (Q2 stays open). The card's "design wins" clause
  applies, so:
  - kiln-ui `List` holds focus itself: it is a `ListBox` (named by
    `ListDelegate::accessible_name`), its rows `ListBoxOption`s (named by
    `ListDelegate::row_name`) with selected, position and size, and the selected row is the
    active descendant while the list has focus. `RowState::option` is the shared helper.
  - `PaletteFrame` is a `Dialog` named by `.name(..)`, its list a `ListBox` of the same name;
    `.track_focus(..)` makes the dialog the focused node for a palette with no query. The tab
    switcher uses it, so its selected tab is the active descendant and follows each press.
  - kiln-shell `Picker`: a `Dialog` named by `PickerDelegate::title` (else the query's
    placeholder), the query a `SearchInput`, rows `ListBoxOption`s named by
    `PickerDelegate::match_label`, selected, with position and size. On each move of the
    selection to another match (refilter or key) it announces that match's label politely.
    It never marks a row as active descendant, since GPUI would ignore it.
  - Review round 1 (F1): `match_label` is a required method, never defaulting to
    `match_id` (an index, entity id or empty string in many pickers). Every delegate
    returns the text its row shows, its secondary text joined with ", " where it tells
    rows apart (a diagnostic's file and line, a symbol's place, "current").
  - Review round 1 (F2): the picker announces as it draws, and only while its query has
    focus in the active window, so a dismissed picker kept for reuse (quick open) says
    nothing as late matches arrive, and nothing reaches another window. `show` resets what
    was last said, so a picker shown again says its selection.
  - Gpui-pre 0.3.9 gives no public way to set AccessKit's `active_descendant` relation on
    the query's node to a sibling option (node ids of real elements are `pub(crate)`), and
    `text_input.rs` is A11Y-5's; doing so is Q2's restructure, not this card.
- **Find bar:** a `Search` landmark named "Find" (the query's placeholder message), the query
  a `SearchInput`, the replacement a `TextInput`; toggles and buttons keep A11Y-3's names.
  There is no close button in the bar to label.
- **T557 F6 (carried):** the Vim command is now focusable: a persistent tab-stop focus handle,
  Enter or Space copies it while focused, the focus ring shows when focused from the
  keyboard, and a click still leaves focus where it was
  (`find_bar::vim_command_keys::the_keyboard_reaches_and_presses_the_vim_command`).
- `modal.rs` needed no change: the dialog role lives on `PaletteFrame`, which every modal
  (pickers and the tab switcher) draws.
- Scope gained `kiln/crates/kiln-shell/src/command_palette.rs` (respec), so palette options
  read command titles rather than ids, then every `PickerDelegate` implementer (respec for
  F1). The carried mediums that came with those files (T480 F2/F4, T491 F4/F5, T493 F1,
  T548 F1, T559 F1) are unrelated to screen-reader names and are left to their own cards;
  the buffer picker's label uses the catalogued `workbench-setup-current`, not its row's
  hard-coded "current" (T559 F1's row is unchanged).

### Section 12 human checks for A11Y-11

- **H8. Pickers.** Open the command palette and type: the query is echoed. Down arrow reads
  the selected command. Escape returns to the editor, and VoiceOver says so. The same for
  quick open.
- **H1′ (last line).** Every button has a title (the find bar's toggles, steps, replace
  buttons and the Vim command).
- **H11′ (second line).** Escape closes, and Return confirms, in every in-window modal,
  including the palette, quick open and the tab switcher.
- **H11a′ (first line).** List rows take focus and show the focus ring.
