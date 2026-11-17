---
task_uid: "01m7kpwycn9wssnk3czq8q85kc"
task: "T373"
rev: 1
type: "feature"
complexity: "m"
criteria: []
---

# Detect language for unknown extensions by first line and content, add <?php and html markers, and remember Change Language Mode's choice per file (Auto Detect clears it) (Mac T1916)

User asked: "Do we handle file type detection by content yet? Detecting XML/JSON or comments specifying the file type - despite the extension?"

Today `fe_syntax::detect_language` (crates/fe-syntax/src/detect.rs) runs: modeline > path (`language_name_for_path`) > shebang > first-line markers (`language_name_for_first_line`) > content sniff (crates/fe-syntax/src/sniff.rs `language_name_by_content`). The sniff only runs for a path with NO extension, and first-line markers only when the path names nothing. Change Language Mode (`language.select`, crates/fe-workbench/src/language_picker.rs) sets the language through `EditorView::set_language` (crates/fe-gpui/src/view.rs, sets `language_chosen`) but never persists it.

## Do
1. Unknown extension: `detect_language` runs the first-line markers and the XML/JSON sniff also when the path has an extension that NO language maps (`language_name_for_path` is None). A known extension still wins, unless a modeline names a language. Update `language_name_by_content`'s no-extension guard in sniff.rs accordingly (and the doc comments in detect.rs).
2. New first-line markers in `language_name_for_first_line`: `<?php` -> php; `<!DOCTYPE html` (ASCII case-insensitive) and `<html` -> html. Only where those languages are compiled in / registered (same rule as the existing markers).
3. Remembered choice: a language picked in Change Language Mode is remembered per file (absolute path) and restored when the file is opened again, in any window of that project. Store it in a new small per-project state file in fe-config modelled on crates/fe-config/src/harpoon.rs (`<state_dir>/languages/<project_key>.toml`, `project_key(root)` from harpoon.rs, advisory-locked read-modify-write, unparsable file reads as empty). Rootless windows use the empty-path key as `window_layouts` does (crates/fe-workbench/src/workbench.rs). Prefer this over the window layout store (kiln LayoutStore), which only restores on a window restore.
4. Picker: new first row "Auto Detect" (l10n key, fe-workbench ftl) clears the remembered choice for the file and runs `EditorView::detect_language` (pane_item.rs:817, which resets `language_chosen`). Picking Plain Text or a language stores it. A remembered name no longer known (grammar gone) falls back to detection, no error.
5. Apply the remembered choice where an editor for a path opens: `detect_language` call sites crates/fe-workbench/src/workbench_files.rs (:328, :629) and crates/fe-gpui/src/pane_item.rs (:771, :1879). Design the hook so card 2 can insert configured associations without touching these again (e.g. a `LanguageOverride`/resolver passed in or set on the EditorView; a plain `fn detect_language_with(path, start, end, extra_rules)` in fe-syntax is fine).

## Precedence after this card
remembered choice > modeline > built-in path mapping > shebang > first-line > sniff (sniff/first-line also for unknown extensions).

## Notes
- Scope (suggested): crates/fe-syntax/src/{detect,sniff,language}.rs, crates/fe-syntax/tests/**, crates/fe-config/src/{lib.rs,language_choices.rs}, crates/fe-config/tests/**, crates/fe-gpui/src/{pane_item,view}.rs, crates/fe-workbench/src/{language_picker,workbench_files,tests/mod}.rs + new tests, crates/fe-workbench/l10n/en-US/*.ftl, l10n-allow.toml.
- Tests in NEW files: crates/fe-syntax/tests/unknown_extension_detect.rs (e.g. `notes.zz` with `<?xml` / `{"a":1}` / `<?php` / `<!doctype HTML>`; `a.json` containing `<?xml` stays json; modeline beats known extension), crates/fe-config/tests/language_choices.rs (round-trip, lock, corrupt file), crates/fe-workbench/src/tests/language_remembered.rs declared from src/tests/mod.rs (shared append-only): pick, close, reopen restores; Auto Detect clears.
- Overlap: T1485 holds grammar_prompt.rs and grammar_install.rs, which call `detect_language` after an install (grammar_prompt.rs:615, grammar_install.rs:771); not in this scope, keep their signatures. T1901/T1902 (grammars) only touch extensions/grammars/**, no conflict. Keep `detect_language(path, start, end)` as is (add new fns instead) so other callers (commit_diff.rs, diff_view.rs, refactor_preview_view.rs) don't change.
- Linux-ok; no Mac golden expected (a picker row is not in a golden; if one is, say so).

## Implementation notes

- fe-syntax: first-line markers already ran for an unknown extension (the path names nothing); only the sniff's extension guard went. `<?php` and `<!DOCTYPE html`/`<html` (ASCII case-insensitive, after leading whitespace, the tag name ending there) count where `language_names()` has the language, as the other markers do. `content_sniff.rs`'s "unknown extension is never sniffed" and its HTML cases changed to match.
- Hook for card 2: `EditorView::set_language_resolver(cx, |path, cx| …)` sets one app-wide resolver (`Some(Some(name))` a language, `Some(None)` Plain Text, `None` no say). `EditorView::resolve_language` asks it and else runs `detect_language`. The constructor, Save As (pane_item), `detect_language_from` (splits and revisions) and the workbench's `editor_moved`/`follow_moves` call `resolve_language`. `detect_language` stays pure detection, so Auto Detect and the grammar install callers keep their behaviour. Card 2 composes its associations into the resolver that `language_picker::init` installs.
- Store (`fe_config::language_choices`): the resolver runs when the editor is built, before the workbench can tell it its window's root. So a lookup needs no root: it takes the canonical file and walks up to the nearest folder whose list names the file, and failing that uses the rootless (empty-path key) list. The walk does one `read_dir` of `<state>/languages` and reads only lists that exist. A pick goes in the window root's list when the file is under that root, and in the rootless list otherwise. Auto Detect forgets the file in every list a lookup would find it in. Writes run off the UI thread, chained in pick order.
- Outside the suggested scope: `fe-workbench/src/lib.rs` (one `language_picker::init(cx)` line) and `workbench_palettes.rs` (passes the window's root to `LanguagePicker::opened`). `tests/status_bar.rs` and `tests/a11y_pickers.rs` changed because Plain Text is now the second row. `fe-config/l10n-allow.toml` lists the store's two file-name format strings, as it does harpoon's. `grammar_prompt_tests.rs` and `tests/grammar_prompt_packs.rs` (beside T1485's `grammar_prompt.rs`) wrote `{"a": 1}` into files with an uninstalled fixture's extension and expected plain text until the install. The sniff now takes that for JSON, so the tests write `"a"` instead: still JSON for the fixture's grammar, and not something the sniff claims.
