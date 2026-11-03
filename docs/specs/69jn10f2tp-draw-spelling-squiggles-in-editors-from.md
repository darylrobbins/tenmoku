---
task_uid: "01m6ezvh4s947wnw69jn10f2tp"
task: "T422"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test spell_squiggles_tests::misspelled_words_get_an_underline (cargo nextest run -p fe-workbench) passes: with a fake Speller, a rust comment 'teh quick' gets one Underline decoration over 'teh' in the Information tint and none over code; fixing the word removes it after the next check"
    required: true
    manual: false
  - id: "C2"
    text: "Test spell_squiggles_tests::checks_are_debounced_and_off_main_thread (cargo nextest run -p fe-workbench) passes: ten edits within the 400 ms debounce (paused clock) cause one check; the speller is called from cx.background_spawn; a result for an outdated buffer version is dropped"
    required: true
    manual: false
  - id: "C3"
    text: "Test spell_squiggles_tests::only_the_viewport_plus_margin_is_checked (cargo nextest run -p fe-workbench) passes: in a 5,000 row buffer only the visible rows plus 50 either side are checked, and scrolling checks the newly exposed rows once (results cached by row range until edited)"
    required: true
    manual: false
  - id: "C4"
    text: "Test spell_squiggles_tests::setting_toggles_live (cargo nextest run -p fe-workbench) passes: editor.spell_check = false removes every squiggle and stops checks without reopening files; true brings them back; removing a scope from editor.spell_check_scopes drops its squiggles"
    required: true
    manual: false
  - id: "C5"
    text: "Test spell_squiggles_tests::dictionary_changes_recheck (cargo nextest run -p fe-workbench) passes: adding a word to the dictionary or ignore set removes its squiggles in all open editors"
    required: true
    manual: false
  - id: "C6"
    text: "Test spell_squiggles_tests::no_speller_means_no_squiggles (cargo nextest run -p fe-workbench) passes: with NoSpeller the provider adds no decorations and costs no background tasks"
    required: true
    manual: false
  - id: "C7"
    text: "Spelling squiggles never appear in the Problems panel or the status-bar problem counts (asserted in the same file), and the existing diagnostics squiggle tests pass unchanged"
    required: true
    manual: false
---

# Draw spelling squiggles in editors from a debounced background check, behind editor.spell_check, using a decoration provider (Mac T1334)

## Problem
f9a/f9b can find misspellings; nothing shows them.

## Change
New crates/fe-workbench/src/spell_squiggles.rs: a `DecorationProvider` (crates/fe-editor/src/decorations.rs; installed with `EditorModel::add_decoration_provider` like other providers) per editor, holding the cached misspellings and a debounced check task. Underline kind and a distinct tint from diagnostics (`Tint::Information`; if the underline is not wavy for that tint, use the wavy `Underline` variant already used for diagnostics). The Workbench owns an `Rc<dyn Speller>` (default NoSpeller; the app sets the real one, f9e) and the `Dictionary`, and attaches the provider when an editor opens (find where `lsp_lightbulb`/`lsp_colors` attach theirs in workbench.rs and mirror it). Changes to settings follow `SettingsEvent` as `format_on_save` does in workbench_root.rs (read, do not edit that file; if it must change, block with the finding).

## Scope decision (Mac lead, option A)
Add crates/fe-workbench/src/workbench_root.rs to scope for one hook in `reload_settings` (the only place a SettingsEvent reaches a workbench): `self.reload_spell_squiggles(&event.settings.editor, cx)`, shaped like `reload_auto_save`. The speller is held as `Arc<dyn Speller>` instead of `Rc<dyn Speller>`, because `background_spawn` needs Send.

## Implementation notes

- `Workbench::set_speller` is generic so that passing `NoSpeller` holds no speller at all and costs nothing; the trait can't say so itself. The speller is an `Arc<dyn Speller>`, as the scope decision says.
- `Workbench::add_spell_word`, `ignore_spell_word`, `misspellings`, `speller`, `spell_language` and `spell_dictionary` are the seams T423 needs. The dictionary starts empty and writes nothing, because reading the user's blocks on its file: the app reads `Dictionary::user()` off the main thread and calls `set_spell_dictionary`, along with setting the real speller (f9e).
- Only the editor each pane is showing gets checked; a hidden tab keeps its squiggles, and is checked when shown. Dictionary changes reach hidden tabs at once. Diff view sides have no squiggles, as they aren't in `open_editors`.
- Rows whose highlights don't finish are looked at again up to 8 times, with any language found missing compiled first, then count as having no words until the next edit.
- Review n6y3a0 F4 added `crates/fe-workbench/src/spell_ranges.rs` and its tests to scope: `checked_spellable_ranges` reports whether the queries it really made finished, which a separate probe of the rows can't, as they reach past the rows. `spellable_ranges` is unchanged.
- Every open editor gets its provider, hidden ones too, though only shown ones are checked: a split's copied provider then can't outlive its original.
- The rows in view come from what the last frame laid out (`EditorView::shows_buffer_row`), so blocks in the view, a view starting partway down a wrapped row, and rows in a fold don't change what's checked: the rows shown, with 50 more above the first and below the last. A row partly in view counts as shown.
