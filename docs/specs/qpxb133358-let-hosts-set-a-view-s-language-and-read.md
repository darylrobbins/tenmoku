---
task_uid: "01m417z4g28atd5aqpxb133358"
task: "T26"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui: set_language on an untitled view highlights it, and set_language(None) returns it to plain text (test view::language_tests::set_language_highlights_an_untitled_buffer)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui: switching language while a parse is in flight never installs the old language's tree (test view::language_tests::switching_language_mid_parse_discards_the_old_parse)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui: an extensionless file starting with #!/usr/bin/env python3 opens highlighted as Python (test view::language_tests::a_shebang_picks_the_language_at_open)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui: EditorView::syntax() returns a tree and version matching the model's snapshot once parsing settles (test view::language_tests::hosts_read_the_parsed_tree)"
    required: true
    manual: false
---

# Let hosts set a view's language and read its syntax tree

Source: Paddock T-13 (`b2c2ea4f-4f64-4e02-953d-20843cb10b79`), P3, draft (needs the user's approval).
Split from T-13: this is the stateful `EditorView` half; detection is T-13a.

## Problem

`EditorView::new` (`crates/fe-gpui/src/view.rs`) picks the language from the path once, and
`EditorView::syntax` is `pub(crate)`. The AI host needs to:

- set or override the language, for untitled buffers, AI scratch buffers and unusual extensions;
- detect it from content when the path says nothing (T-13a);
- read the current syntax tree for structure-aware edits and context.

## Change

After T-05, `EditorView.syntax` is an `Option<SyntaxDriver<gpui::Task<()>>>` and `language_task`
holds a language load:

- `EditorView::new` uses `fe_syntax::detect_language_name(path, first ~512 bytes)` instead of
  `language_name_for_path`.
- `pub fn set_language(&mut self, name: Option<&str>, cx)`: `None` turns highlighting off. It drops
  the current driver (cancelling its parse) and any `language_task` before installing the new one, loading
  the language in the background as `new` does when it isn't loaded yet. A parse started for the old
  language must never be installed into the new one: `Syntax::finish` checks buffer and version, not
  language.
- `pub fn syntax(&self) -> Option<&Syntax>` for hosts, through `SyntaxDriver::syntax()`. `Syntax::tree()` is edited to match
  `Syntax::version()` but may not be reparsed yet; `needs_parse()` says which. Document that on the
  accessor.
- GPUI tests in a new module declared from `view.rs` as `#[cfg(test)] mod language_tests;`
  (`crates/fe-gpui/src/view/language_tests.rs`), on the pattern of `tests::highlighting_follows_edits`
  in `crates/fe-gpui/src/tests.rs`.

## Open questions

These need the user's answer before this card is readied:

- **Where `Syntax` lives long term.** Today it's on `EditorView`; after T-05 the driver is still owned
  by the view. The plan mentions a `DecorationProvider` in fe-editor. If syntax moves to the model,
  `set_language` and `syntax()` belong on `EditorModel` instead and this card's scope changes. This card
  assumes it stays on the view for now.
- Should an explicit `set_language` stick when the path later changes (Save As), or re-detect?
  This card assumes it sticks until `set_language(None)` or another call.
- Outlines via tree-sitter-tags (in the plan) are a later card, not this one.

## Implementation notes

- `EditorView::new` now detects through `detect_language_name(path, first 512 bytes)` and then
  calls `set_language`, so opening and overriding share one code path.
- `set_language` clears the driver and `language_task` (dropping cancels both) before installing
  anything. A name that isn't compiled in is treated as `None`.
- As a second guard, `ParseSpawner` carries the language it parses for, and its completion only
  calls `finish` when the view's current driver has that same language.
- An explicit language sticks: nothing re-detects when the path changes. That matches the card's
  stated assumption, and the open questions still need the user's answer.
