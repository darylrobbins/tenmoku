---
task_uid: "01m6ezvf0r4kmh0ewhx9ecf1mf"
task: "T420"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-config --test spell_check_setting passes: editor.spell_check defaults to true, editor.spell_check_scopes defaults to [\"comments\", \"strings\", \"text\"], editor.spell_check_language defaults to \"auto\"; scopes accept only comments, strings and text, and a bad scope is a settings diagnostic that keeps the valid ones"
    required: true
    manual: false
  - id: "C2"
    text: "Test spell_tests::tokenizer_splits_identifiers (cargo nextest run -p fe-workbench) passes: 'parseHttpResponse_body' yields parse, Http, Response and body with their byte ranges; 'HTTPServer' yields HTTP and Server; apostrophes stay inside a word (don't); non-ASCII letters are kept"
    required: true
    manual: false
  - id: "C3"
    text: "Test spell_tests::tokenizer_skips_non_words (cargo nextest run -p fe-workbench) passes: URLs, emails, file paths, hex literals (0xdeadbeef), words with digits (utf8, h264), words under 3 chars, ALL-CAPS words of 2..=5 letters and words after a '$' or '@' sigil yield no tokens"
    required: true
    manual: false
  - id: "C4"
    text: "Test spell_tests::misspellings_use_the_speller_and_the_dictionary (cargo nextest run -p fe-workbench) passes: with FakeSpeller knowing only 'hello', check_text returns the ranges of other words, drops any word in the user dictionary or session-ignore set (case-insensitively), and asks the speller once per distinct word"
    required: true
    manual: false
  - id: "C5"
    text: "Test spell_tests::dictionary_file_roundtrips (cargo nextest run -p fe-workbench) passes: add_word appends one lowercase word per line to the dictionary file (created with its parent directory, mode 600 not required), duplicates are not written twice, comments and blanks are ignored on load, and a missing or unreadable file is an empty dictionary, not an error"
    required: true
    manual: false
  - id: "C6"
    text: "Test spell_tests::suggestions_pass_through_the_speller (cargo nextest run -p fe-workbench) passes: suggest(word) returns FakeSpeller's list capped at 8, case-matched to the original (Teh gives The, TEH gives THE)"
    required: true
    manual: false
---

# Add the spelling core: editor.spell_check settings, a Speller trait with a test fake, a word tokenizer that splits camelCase and snake_case and skips URLs and code-like words, and a user dictionary file (Mac T1332)

## Problem
User approved spell checking 2026-11-02 as a day-one gap: comments and strings in code (tree-sitter aware) and plain text/Markdown, squiggles, suggestions, add to dictionary, preferring macOS's NSSpellChecker. The first release is macOS-only but cards build on Linux, so the checker sits behind a seam.

## Change
- Settings (EditorSettings): `spell_check: bool = true`, `spell_check_scopes: Vec<SpellScope>` (comments | strings | text; `text` = plain text and Markdown prose), `spell_check_language: String = "auto"` (auto = the system's language list; otherwise a BCP-47 tag). Schema regenerated; template documents all three.
- New crates/fe-workbench/src/spell.rs, no GPUI: `trait Speller: Send + Sync { fn is_misspelled(&self, word: &str, language: &str) -> bool; fn suggest(&self, word: &str, language: &str) -> Vec<String>; }`, `NoSpeller` (everything is correct; used off macOS and as the pre-wiring default), `Dictionary` (user file `<config dir>/dictionary.txt` via fe_config::paths, plus a session-ignore HashSet), `tokenize(text) -> Vec<Word{range, text}>`, `check_text(speller, dict, text, language) -> Vec<Misspelling>`, `suggest`. `FakeSpeller` lives in the test file. Workspace-local dictionaries are out of scope.
- Declared in lib.rs; no other file in the workbench changes here.

## Implementation notes

- A bad scope is dropped by a layer filter keyed on `editor.spell_check_scopes` alone: a filter on `editor` would take the place in the file away from every other editor key's error. `LspLayer::read` runs the same filter, or a warning in the store would cost a trusted root its `lsp` table.
- C2 and C3 meet at the all-capitals rule: it applies to a whole word or `_` segment (`HTTP`, `HTTP_URL`), not to a part of a camelCase one (`HTTPServer` gives `HTTP`). An acronym's plural (`URLs`) is left out in both places.
- The tokenizer prefers a missed word to a false squiggle: anything with a `/` is a path (`and/or` too), and so is a dotted or `::` name. Words of Chinese or Japanese are kept as runs, per C2; the speller wiring may want to look at that.
- A backslash is a path's when what follows can't be a string's escape (`assets\wrod`, `assets\_wrod`, `assets\\wrod`). A known limit, pinned by a test: where every backslash could be an escape, `src\test` can't be told from `first\nsecond`, and it's read as the escape, so that words in strings still get checked.
- Dictionary keys fold each letter through lowercase, capitals and lowercase again, leaving `ı` alone, with no new dependency. Over every code point that gives the same classes as Python's `str.casefold()`, Unicode's default full case folding, but for the case pairs Unicode 17 added, which Python's Unicode 16 doesn't have, and the deliberate `’` as `'`. Canonically equivalent spellings (precomposed and decomposed `é`) aren't folded; that would need unicode-normalization in the crate.
