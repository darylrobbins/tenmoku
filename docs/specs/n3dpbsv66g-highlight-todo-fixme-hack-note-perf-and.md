---
task_uid: "01m6rmmn8mvnvevqn3dpbsv66g"
task: "T485"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "todo_comments_tests::finds_keyword_colon_in_a_comment, todo_comments_tests::ignores_a_keyword_in_a_string_or_code and todo_comments_tests::requires_the_colon_and_a_word_boundary"
    required: true
    manual: false
  - id: "C2"
    text: "todo_comments_tests::aliases_map_to_their_kind and todo_comments_tests::captures_the_user_in_parentheses"
    required: true
    manual: false
  - id: "C3"
    text: "todo_comments_tests::comment_ranges_come_from_the_highlights_for_rust_and_python and todo_comments_tests::markdown_has_none"
    required: true
    manual: false
  - id: "C4"
    text: "todo_decorations_tests::a_todo_comment_gets_a_decoration_after_the_rest_delay, todo_decorations_tests::setting_off_adds_no_provider, todo_decorations_tests::editing_the_comment_moves_the_decoration and todo_decorations_tests::every_kind_is_a_tinted_background_on_its_tag (the badge is a tinted background on the keyword's tag, Fix Error, Hack/Warn Warning, Todo Information, Note/Test Hint, Perf Accent; bold, a tinted foreground and a distinct Perf tint are Known limits)"
    required: true
    manual: false
  - id: "C5"
    text: "todo_comments_defaults_to_true (flat file crates/fe-config/tests/todo_comments_setting.rs)"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-workbench -p fe-config pass"
    required: true
    manual: false
---

# Highlight TODO, FIXME, HACK, NOTE, PERF and WARN keywords in comments (editor.todo_comments) (Mac T1413)

Lent from Mac T1413 (lend y50, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

# Highlight TODO, FIXME, HACK, NOTE, PERF and WARN keywords in comments (editor.todo_comments)

## Problem
nvim's todo-comments.nvim (LazyVim extra, in the default LazyVim UI) colours `TODO:`, `FIXME:`, `HACK:`, `WARN:`, `PERF:`, `NOTE:` and `TEST:` in comments, and VS Code users install Todo Highlight. Tenmoku draws nothing special for them. This card is the highlighting and the shared scanner; the project list and `]t`/`[t` are the next card.

## Change
- New `crates/fe-workbench/src/todo_comments.rs`, pure: `scan(text: &Text, comment_ranges: &[Range<usize>], keywords: &Keywords) -> Vec<TodoHit>` where `TodoHit { keyword_range, kind: Todo|Fix|Hack|Warn|Perf|Note|Test, tag_range (the keyword and its colon), user: Option<Range> (the `(name)` after it) }`. A hit is one of the keywords in capitals at a word boundary inside a comment, followed by `:` (`TODO:`, `TODO(daryl):`), the todo-comments default; aliases `FIX`, `FIXME`, `BUG`, `FIXIT`, `ISSUE` -> Fix, `XXX` -> Hack, `WARNING` -> Warn, `OPTIMIZE`, `PERFORMANCE` -> Perf, `INFO` -> Note, `TESTING`, `PASSED`, `FAILED` -> Test. Ranges are bytes.
- Comment ranges come from the parse the editor already has: reuse `spell_ranges.rs`'s walk of highlight spans for `Highlight::Comment` tokens (extract the shared part into the new file's `comment_ranges(syntax, text, rows)` rather than editing spell_ranges.rs, duplicating twelve lines at most), restricted to the visible rows plus a margin as `spell_squiggles.rs` does, and Markdown/plain text yield none.
- Drawing: a decoration provider per shown editor (the pattern of `spell_squiggles.rs`, using `fe_editor::decorations`), painting the keyword with its tag as a badge that is a tinted background only (a `DecorationKind::Highlight`, the kind's colour at low alpha); colours come from the theme's palette through `Tint`, so each bundled theme works: Fix Error, Hack and Warn Warning, Todo Information, Note and Test Hint, Perf Accent. Debounced like the squiggles (REST), repaint on edit and theme change.
- Setting `editor.todo_comments` (bool, default true) in `crates/fe-config/src/settings.rs`, `settings.schema.json`, `settings-template.toml`; `editor.todo_keywords` is not added (follow-up if wanted).

## Tests (new files)
- `crates/fe-workbench/src/todo_comments_tests.rs` (`todo_comments_tests::...`): `finds_keyword_colon_in_a_comment`, `ignores_a_keyword_in_a_string_or_code`, `requires_the_colon_and_a_word_boundary`, `aliases_map_to_their_kind`, `captures_the_user_in_parentheses`, `comment_ranges_come_from_the_highlights_for_rust_and_python`, `markdown_has_none`.
- `crates/fe-workbench/src/todo_decorations_tests.rs` (`todo_decorations_tests::...`): `a_todo_comment_gets_a_decoration_after_the_rest_delay`, `setting_off_adds_no_provider`, `editing_the_comment_moves_the_decoration`.
- `crates/fe-config/tests/todo_comments_setting.rs`: `todo_comments_defaults_to_true`.
Mac screenshot: not required (decoration ranges and colours are asserted on the provider's output); no macOS-only criterion. Linux-OK.


## Helium notes
- A setting goes live on helium only through a line in crates/fe-workbench/src/workbench_root.rs `reload_settings` (Pane.settings is private; GlobalSettings misses per-root settings). If the new key must change behaviour without a restart, respec workbench_root.rs into scope before the first finish (`rgt task respec --scope`, which replaces the list).
## Known limits
The badge is a tinted background only. Missing, and left to a follow-up card (badge styling) that the Mac lead files:
- Bold: the fe-gpui text pipeline carries only a colour per span (`StyledSpan<Hsla>`, one font per shaped line), so a weight needs a new span style through line layout and glyph-width measurement (`fe-gpui/src/element.rs`, `element/decorations.rs`, minimap, sticky scroll).
- A foreground in the kind's colour: `DecorationKind::Foreground` takes a syntax highlight name and the theme has none for these kinds, so it needs a tint-carrying foreground kind.
- A Perf tint distinct from the cursor colour: `Tint::Accent` resolves to `theme.cursor`; a distinct hint tint needs a new `Tint` or theme colour.

## Implementation notes

- The provider, `Todos`, lives in `todo_comments.rs` beside the scanner; it is wired in `workbench.rs` (render sync) and `workbench_root.rs` (`reload_settings`), with `spell_squiggles.rs` helpers made `pub(crate)`.
- The decoration model has no bold or per-kind foreground, so a badge is a `Highlight` in the kind's tint (Fix Error; Hack/Warn Warning; Todo Information; Note/Test Hint; Perf Accent).
