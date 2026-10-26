---
task_uid: "01m5vxsc9xqgm2sazwnv8mxgyh"
task: "T1186"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "move_to_match_tests::moves_the_last_selection: with carets on two words and the second (bottommost) word \"foo\" selected, editor.move_last_selection_to_next_find_match drops that selection and selects the next \"foo\" after it, keeping the first selection; without a wrap the new selection is the primary (bottommost) selection, which the view follows"
    required: true
    manual: false
  - id: "C2"
    text: "move_to_match_tests::empty_selection_uses_the_word: with only a caret inside \"foo\" the word at the caret is selected first (as select_next_occurrence does) and a second call moves that selection to the next whole-word \"foo\""
    required: true
    manual: false
  - id: "C3"
    text: "move_to_match_tests::wraps_and_skips: after the last match it wraps to the first (the moved selection, landing above another selection, is then not the primary and is not separately revealed: the documented limit, with no unconsumed reveal-request state left in the code); it skips a match already selected by another caret; with no other match, or a word over the occurrence length limit, nothing changes"
    required: true
    manual: false
  - id: "C4"
    text: "move_to_match_tests::undo: one call is one Cursor Undo (editor.cursor_undo) step back to the previous selections, edits no text and pushes no text-undo entry (normal Undo after it changes nothing the command did); fe-keymap move_to_match_key: secondary-k secondary-d resolves to it in the editor context, not in vim normal mode; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-gpui keymap_tests, fe-workbench default_keymaps); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Move Last Selection to Next Find Match (cmd-k cmd-d), as VS Code's editor.action.moveSelectionToNextFindMatch

fastedit has Add Selection to Next Find Match (select_next_occurrence, cmd-d) and Select All Occurrences but not VS Code's cmd-k cmd-d, which moves instead of adds: the most recently added selection is replaced by the next match of the same text, the other selections stay. New module move_to_match.rs with editor.move_last_selection_to_next_find_match ("Move Last Selection to Next Find Match"), reusing multi_cursor.rs's matching (each_match, is_whole_word, the "word at an empty caret" rule and its MAX_WORD_BYTES-style limit) so cmd-d and cmd-k cmd-d agree on what a match is: same case and whole-word rules as select_next_occurrence today. Do not edit multi_cursor.rs; if a helper there needs pub(crate), widen only its visibility. If it looks like an edit to multi_cursor.rs is unavoidable, say so in a review note instead.

VS Code detail: the "last" selection is the most recently added one, i.e. fastedit's primary. A search starts after its end and wraps once. No find-widget state is involved here (VS Code uses the find widget's case/word options only when it is open; fastedit follows select_next_occurrence's rules, which a later card can change for both together).

Key: "secondary-k secondary-d" in context "editor && !vim.enabled" like the cmd-d binding (check the actual context of editor.select_next_occurrence's binding and copy it). Chord prefix secondary-k is already used by many bindings; verify no binding of the form secondary-k secondary-d exists. Headless tests via the case_transform_tests.rs dispatch helper.

Rev 2 (lead decision after review findings F1/F2 recurred): fastedit has no primary identity apart from position. EditorModel::primary_selection is the bottommost selection (lib.rs), and set_selections sorts. So "the last selection" here is the bottommost one, the same selection select_next_occurrence searches from. When the search wraps and the moved selection lands above other selections, it is no longer bottommost and so not primary. That is the known limit of this card: do not add a primary-identity concept to SelectionSet/lib.rs, which stays out of scope (a later card can add one for cmd-d and cmd-k cmd-d together). Either way, the command reveals the moved selection (the new match), not the primary. Undo: fastedit's normal Undo (cmd-z) only reverts text edits, and pure selection changes, cmd-d included, go through Cursor Undo (cmd-u, cursor_history.rs). VS Code's normal Undo doesn't revert cmd-k cmd-d either. So the command must be one Cursor Undo step that edits no text and pushes no text-undo entry, and that includes the word seeding at an empty caret if it happens in the same call.

Rev 3 (lead decision after review r? F1): the editor view scrolls to follow the primary (bottommost) selection, and no view-side mechanism reveals a non-primary selection. Building one belongs outside this card's scope (fe-gpui/fe-workbench viewport). So after a wrap the view follows the primary as it does for every multi-selection command, and the moved selection is not revealed when it lands above the primary. That is the documented limit, together with primary identity. Remove any reveal-request state that nothing consumes (no dead metadata). Without a wrap, the moved selection is bottommost and so primary, and the normal follow-primary scrolling reveals it. A later card adds primary identity plus revealing it for cmd-d and cmd-k cmd-d together.

## Implementation notes

- multi_cursor.rs: only visibility widened (`seed_carets`, `needle`, `overlaps` to pub(crate)).
- Primary stays positional (bottommost), and the view follows it. Without a wrap the moved selection is bottommost, so the usual scrolling reveals it. After a wrap it can land above another selection: it is then not primary and not separately revealed. That is the documented limit (asserted in `wraps_and_skips`); a later card adds primary identity and revealing for this and cmd-d together. No reveal-request state exists.
- Undo is Cursor Undo: one `set_selections` call, so one cursor-history step (seeding included), no text edit, no text-undo entry; tested in `move_to_match::tests::undo`.
- The seed-or-move choice follows the primary: a caret primary seeds every caret; a selected primary moves and leaves other carets alone. A word over MAX_WORD_BYTES is checked before seeding.
