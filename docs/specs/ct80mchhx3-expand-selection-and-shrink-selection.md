---
task_uid: "01m55ef4fzampmntct80mchhx3"
task: "T150"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor integration tests expand_grows_each_selection_and_shrink_steps_back, expanded_selections_that_meet_merge and an_edit_or_a_click_forgets_the_history (crates/fe-editor/tests/expand_selection.rs, top level) pass, dispatching editor.expand_selection and editor.shrink_selection through commands::registry(); they fail at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui unit test view::language_tests::expand_selection::expand_follows_the_syntax_tree passes: on a parsed Rust file each of three editor.expand_selection steps selects exactly fe_syntax::larger_syntax_node of the previous selection and editor.shrink_selection returns to the caret; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "fe-keymap integration test expand_and_shrink_selection_keys_match_vs_code (crates/fe-keymap/tests/expand_selection_keys.rs, top level) passes: ctrl-shift-cmd-right/left on macOS and shift-alt-right/left on Linux in the editor stack, none of them bound with vim on; it fails at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-editor -p fe-gpui -p fe-keymap passes; the only existing tests edited are the four that pin the command and binding lists, each gaining just the two new commands or their bindings: fe-editor src/tests/commands.rs every_editor_command_is_registered, fe-editor tests/viewer.rs refuses_edits, fe-gpui command_tests.rs table_is_the_only_binding_source and keymap_tests.rs default_keymap_matches_the_old_table (219 -\u003e 221 bindings), and language_tests.rs only gains the #[path] mod line; cargo clippy -p fe-editor -p fe-gpui -p fe-keymap --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Expand Selection and Shrink Selection grow every selection to its enclosing syntax node and step back, on VS Code's keys (Mac T855)

## Problem

VS Code's Expand Selection and Shrink Selection (`editor.action.smartSelect.expand` / `.shrink`: ctrl-shift-cmd-right
/ left on macOS, shift-alt-right / left elsewhere) grow every selection to the next enclosing syntax node and step
back through the same ranges. fastedit has the engine but no command: `fe_syntax::larger_syntax_node`
(`crates/fe-syntax/src/select.rs:25`, tested by `crates/fe-syntax/tests/larger_node.rs`) is called by nothing.
(VS Code asks the language server's `selectionRange` first; tree-sitter nodes are what it falls back to and what
fastedit has for every language with a grammar, so this card uses them; an LSP source can come later.) Gap audit
2026-10-17 (user-approved).

## Change

1. `crates/fe-editor/src/commands.rs`: `EditorHost::larger_syntax_node(&self, range: Range<usize>) ->
   Option<Range<usize>>`, defaulting to `None` (as `comment_tokens` defaults), and two commands registered with
   `add` next to the fold commands, neither mutating the buffer: `expand_selection` "Expand Selection" and
   `shrink_selection` "Shrink Selection" (IDs `editor.expand_selection`, `editor.shrink_selection`).
2. New module `crates/fe-editor/src/smart_select.rs` (declared in `crates/fe-editor/src/lib.rs`), holding the
   model's state and the two operations:
   - **Expand** replaces each selection by `host.larger_syntax_node(selection)` (a selection with none, as at the
     root, stays), keeping the selections' order and merging any that now overlap, as the multi-cursor code merges;
     the selections before the step are pushed on the model's expand history. Nothing changes (and nothing is
     pushed) if no selection grew: `Effect::Nothing`.
   - **Shrink** pops the history and restores those selections exactly; with an empty history it does nothing.
   - The history is dropped by any other change of the selections or the text (keep the snapshot version and
     the selections it last set, and compare at the next step), so a shrink after a click or an edit does nothing.
3. `crates/fe-gpui/src/commands.rs`: `ViewHost` implements `larger_syntax_node` with
   `fe_syntax::larger_syntax_node` on its caught-up syntax, as it implements `fold_range_at`.
4. `crates/fe-keymap/keymaps/fastedit.toml`: `editor.expand_selection` on `{ macos = "ctrl-shift-cmd-right",
   default = "shift-alt-right" }` and `editor.shrink_selection` on `{ macos = "ctrl-shift-cmd-left", default =
   "shift-alt-left" }`, context `"editor && !vim.enabled"` (vim keeps its own keys; it's in the palette there).
5. Tests:
   - New integration test `crates/fe-editor/tests/expand_selection.rs`, top-level fns, dispatching through
     `fe_editor::commands::registry()` and `EditorTarget::new` as `crates/fe-editor/tests/viewer.rs` does, with a
     test host whose `larger_syntax_node` grows a range to the next of a fixed list of nested ranges:
     `expand_grows_each_selection_and_shrink_steps_back` (two carets grow independently through three levels; three
     shrinks restore each earlier set exactly; a fourth does nothing),
     `expanded_selections_that_meet_merge`, and
     `an_edit_or_a_click_forgets_the_history` (after an expand, typing a char or setting the caret, a shrink is
     `Effect::Nothing` and leaves the selections).
   - New unit-test file `crates/fe-gpui/src/view/expand_selection_tests.rs`, declared by
     `#[path = "expand_selection_tests.rs"] mod expand_selection;` at the end of the existing
     `crates/fe-gpui/src/view/language_tests.rs`, with its `editor` helper on a Rust file,
     `expand_follows_the_syntax_tree`: the caret inside an
     argument of `fn main() { foo(1, 2); }`, `editor.expand_selection` run three times gives, each time, exactly
     `fe_syntax::larger_syntax_node` of the previous selection, and `editor.shrink_selection` returns to the caret.
   - New integration test `crates/fe-keymap/tests/expand_selection_keys.rs`, top-level fn
     `expand_and_shrink_selection_keys_match_vs_code`, resolving keys as `crates/fe-keymap/tests/lsp_bindings.rs`
     does (copy its helpers): the four keys above on `Os::MacOs` and `Os::Linux` in the plain editor stack, and none
     of them bound in a vim normal-mode stack.
