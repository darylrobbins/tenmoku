---
task_uid: "01m4saf4db71gkkd7s99hr9pev"
task: "T69"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test pairs::tests::open_and_type_over: typing ( before a space gives (|) and typing ) then moves over it; before a word char it inserts only (; typing ) where a ) was already in the text inserts one; with two carets each pairs independently"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test pairs::tests::quotes_wrap_and_backspace: typing ' after a word char (don't) inserts one; \" pairs at a space; backspace in an untouched (|) deletes both; typing [ with foo selected gives [foo] with foo still selected"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test pairs::tests::off_and_other_inserts: with set_auto_close(false), or for a paste or a multi-char insert, typing inserts exactly what's typed; one undo removes a typed pair with its typing"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test commands::pairs_host_tests::per_language: in a Rust view ' is not paired and ( is; in a Python view ' is paired; a fe-workbench settings reload with editor.auto_close_pairs = false turns pairing off in open editors and new ones"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-editor -p fe-gpui -p fe-workbench and cargo clippy on those crates --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Auto-close brackets and quotes as you type, with type-over, backspace of an empty pair and wrapping a selection, per language and off by a setting (Mac T477)

Filed on helium from Mac T477 (~/coordination/helium-cards/T477.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zf.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Held in helium's inbox until helium syncs to mac/main 3eec614a (Mac T496, 6ea5a601, lists auto_pairs for every language). Ready it after that sync.

## Problem

The user (2026-10-12) asked for auto-closing brackets and quotes, with type-over, per-language pairs and a setting to
turn it off. Typing `(` inserts only `(` today (`EditorModel::insert`, `crates/fe-editor/src/lib.rs` :856). T449 adds
`fe_syntax::auto_pairs(language)` and T466 the `editor.auto_close_pairs` setting.

## Change

fe-editor, new module `crates/fe-editor/src/pairs.rs` (`mod pairs;` in lib.rs):

- `EditorHost` (commands.rs :27) gets `fn auto_pairs(&self) -> &[(char, char)] { &[] }`; `EditorModel` gets
  `set_auto_close(bool)` (default true). `editor.insert` of exactly one char goes through
  `insert_typed(ch, pairs)`; pastes, multi-char inserts, completion, commands and vim are untouched.
- **Open**: typing an opener at a caret inserts the pair and leaves the caret between, when the next char is the
  row's end, whitespace, or one of `;:.,=}])>`. A quote that is its own closer also needs the previous char not to
  be a word char (so `don't` stays as typed), and is never paired when the caret is right after the same quote.
- **Wrap**: typing an opener with a non-empty selection surrounds it (`(sel)`), keeping the inner text selected.
- **Type-over**: typing a closer when the next char is a closer this model inserted automatically, still untouched
  and on the caret's row, moves over it instead of inserting. Auto-inserted closers are tracked as anchors and
  forgotten when the caret leaves their row or text is edited between them and their opener's caret position.
- **Backspace** between an auto-inserted, still-empty pair deletes both.
- Every caret does this independently, as one edit, inside the same undo grouping as other typing. With
  `set_auto_close(false)` or no pairs, typing is exactly as before.

fe-gpui: `ViewHost` (`crates/fe-gpui/src/commands.rs` :178) answers `auto_pairs` with
`fe_syntax::auto_pairs(language)` for the view's language (none while it loads). fe-workbench applies
`editor.auto_close_pairs` to each editor's model in `Pane::new_editor` (`crates/fe-workbench/src/pane.rs` :531) and
again in `Pane::reload_settings` (:514) after each editor's own reload, so `view.rs` is untouched.

Not in this card: vim's insert mode (vim types through fe-vim, not `editor.insert`).

## Tests

New files `crates/fe-editor/src/pairs_tests.rs` (declared from pairs.rs with `#[path]`, using
`crate::tests::{parse, render}`) and `crates/fe-gpui/src/pairs_host_tests.rs` (declared from commands.rs with
`#[path]`).

## Implementation notes

- `EditorModel::insert_typed(ch, pairs)` is the spec's name, so the crate-private `insert_typed(text, typed)` it
  would clash with is now `insert_grouped` (its one other caller is `ghost_text.rs`). Typing builds its edits and
  final selections per selection and lands them through a new `apply_selecting`, which `apply_typed` now ends in,
  so a pair, a wrap or a type-over joins the run of typing like any keystroke; a pure type-over edits nothing and
  only moves the caret.
- Tracked pairs anchor both halves (start biased right, end left, so each stays on its char as text goes in either
  side). A pair is forgotten, on each catch-up (`sync_display`), when either half is no longer its char, they are no
  longer on one row, or no selection's head is on their row; and once typed over. Typing inside a pair keeps it:
  that's the `(foo)` case type-over exists for. At most 64 are kept.
- fe-gpui: `ViewHost` gets `language`, the loaded language's name from the syntax driver whether or not its tree
  matches the text yet. `auto_pairs` and `comment_tokens` both read it, which fixes T67 F1 (a second toggle before
  the next frame did nothing); `comment_host_tests::toggles_back_before_the_next_frame` covers it, and fails on the
  old code. With no language loaded (still loading, or plain text) nothing is paired. `vim.rs` builds a `ViewHost`
  too, so it fills the new field.
- fe-workbench: the setting is applied in `Pane::new_editor` and after each editor's reload. Editors restored from
  a saved layout are built in `fe-gpui/src/pane_item.rs` from the restore style's settings, so that sets it too.
- C4's settings half can't run in fe-gpui: fe-workbench is only a dev-dependency there, built against fe-gpui's lib,
  so its `Workbench` takes the lib's `EditorTheme`, a different type from the one the test build sees. The fe-gpui
  test covers the model switch in a view; `pane::auto_close_tests::reload_reaches_open_and_new_editors` in
  fe-workbench covers the reload reaching an open editor and a file opened after it (and fails without the
  `new_editor` line).
