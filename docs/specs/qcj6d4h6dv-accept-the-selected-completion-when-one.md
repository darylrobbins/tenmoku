---
task_uid: "01m55txkaa18nww6qcj6d4h6dv"
task: "T925"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::completion::commit_characters::commit_characters_come_from_the_item_else_the_server passes: an item's commitCharacters, else the server's allCommitCharacters, else none; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test tests::completion::items_convert_to_byte_edits passes with its pinned completionItem capability changed only by commitCharactersSupport: true (the one existing test this card edits)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui unit test view::completion_tests::commit::typing_a_commit_character_accepts_then_types_it passes: with value (commit .) selected at va, typing . leaves value. with the menu closed and undo passing through the texts completion.accept then typing . gives; with vanilla (none) selected, typing . leaves va.; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp -p fe-editor -p fe-gpui -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp -p fe-editor -p fe-gpui -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Accept the selected completion when one of its commit characters is typed, then type the character, as VS Code does, instead of typing over the half-typed word

## Problem

LSP completeness track, item 1 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`:
completion has no "commitCharacters"). A completion item's `commitCharacters` (or, for items without them, the
server's `completionProvider.allCommitCharacters`) are characters that, typed while the item is selected,
accept it and are then typed after it. In VS Code (`editor.acceptSuggestionOnCommitCharacter`, on by default)
typing `.` or `(` on a selected TypeScript or Python completion completes the name and types the `.` or `(`.
fastedit types the character over the half-typed word and the menu closes, so the user must accept with
Enter or Tab first.

Today fe-lsp neither advertises `completionItem.commitCharactersSupport` (`client_capabilities`,
`crates/fe-lsp/src/server.rs:2036-2045`) nor reads the field (`convert`, `crates/fe-lsp/src/completion.rs:213`),
fe-editor's `CompletionItem` (`crates/fe-editor/src/completion.rs:39-58`) has no field for it, and
`EditorView::type_text` (`crates/fe-gpui/src/commands.rs:709-720`) types first and only then tells the menu.

## Change

1. `crates/fe-lsp/src/completion.rs`: `CompletionItem` gains `pub commit_characters: Vec<String>`: the item's
   `commitCharacters`, else the server's `allCommitCharacters` (from its capabilities when the list is
   converted), else empty. `itemDefaults` stays unadvertised and unread.
2. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `completionItem.commitCharactersSupport: true`.
3. `crates/fe-editor/src/completion.rs`: `CompletionItem` gains `pub commit_characters: Vec<String>`
   (default empty); the struct's docs say what it is for.
4. `crates/fe-workbench/src/lsp.rs` `completion_item`: copies it.
5. `crates/fe-gpui/src/completion_menu.rs`: new `pub(crate) fn commit_completion(&mut self, text: &str, cx) -> bool`.
   When the menu shows with a live session and a selected item, and `text` is one character among that
   item's `commit_characters`, it accepts the item exactly as `completion.accept` does and returns true.
   `EditorView::type_text` (`crates/fe-gpui/src/commands.rs`) calls it before inserting, then types `text`
   as today, so the character lands after the accepted text, and undo steps back exactly as after accepting
   with `completion.accept` and then typing the character. Nothing else changes: other typing narrows the menu as
   today, and replace mode, pastes and multi-character input never commit.
6. Tests (new files, each declared from an existing test file with one mod line):
   - `crates/fe-lsp/src/tests/completion/commit_characters.rs`, declared by `mod commit_characters;` in
     `crates/fe-lsp/src/tests/completion.rs`. `commit_characters_come_from_the_item_else_the_server`: with a fake
     whose `completionProvider.allCommitCharacters` is `["."]`, an item with `commitCharacters: ["(", ";"]` has
     those and one without has `["."]`; with no `allCommitCharacters`, an item without them has none.
   - `crates/fe-lsp/src/tests/completion.rs` `items_convert_to_byte_edits`: its pinned `completionItem`
     object gains `"commitCharactersSupport": true`; the only change to an existing test.
   - `crates/fe-gpui/src/view/completion_commit_tests.rs`, declared at the end of
     `crates/fe-gpui/src/view/completion_tests.rs` as `#[path = "completion_commit_tests.rs"] mod commit;`, with
     a provider of its own offering `value` with `commit_characters: ["."]` and `vanilla` with none, at `va`:
     `typing_a_commit_character_accepts_then_types_it`: with `value` selected, typing `.` leaves `value.`,
     the caret after the `.` and the menu closed, and `editor.undo` until the text stops changing ends at
     `va`, passing through the same texts as `completion.accept` then typing `.` does; with `vanilla`
     selected, typing `.` leaves `va.`, as at the base.

Order: T913 and the snippet-guard card edit the same fe-lsp files first; T884 (ready) edits
`completion_menu.rs` and `completion_tests.rs`. T856/T857 add fields to the same `CompletionItem`s later and
must keep this one. Linux-OK.
