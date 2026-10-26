---
task_uid: "01m5qp1tc9ch4j97d9y4gzhgpf"
task: "T1156"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "preserve_case_tests::rules: apply('Foo','bar')=Bar, ('FOO','bar')=BAR, ('foo','BAR')=bar, ('foo_bar','baz_qux')=baz_qux, ('Foo_Bar','baz_qux')=Baz_Qux, ('fOo','bar')=bar (exact table checked against VS Code and written in the test)"
    required: true
    manual: false
  - id: "C2"
    text: "find_preserve_case_tests::replace_all: with the toggle on, replacing /foo/i with 'bar' over 'foo Foo FOO' gives 'bar Bar BAR' as one undo step; with it off, 'bar bar bar'; Replace (single) follows the same rule and capture groups still expand first"
    required: true
    manual: false
  - id: "C3"
    text: "search_preserve_case_tests::project_replace: with the toggle on, Replace All across two files produces the same cased results and the counts shown first are unchanged; toggle off leaves existing search_panel tests passing unedited"
    required: true
    manual: false
  - id: "C4"
    text: "fe-keymap preserve_case_keys: alt-cmd-p (macos) / alt-p resolve to find.toggle_preserve_case in find_bar and search.toggle_preserve_case in search_panel; the toggle button is present with its on/off state"
    required: true
    manual: false
  - id: "C5"
    text: "Catalog tests updated; cargo nextest run -p fe-workbench -p fe-keymap and clippy pass"
    required: true
    manual: false
---

# Preserve Case toggle (alt-cmd-p) in find-replace and project replace, as VS Code's

## Problem
The user asked on 2026-10-24 "What are we missing in the editor?". VS Code's Preserve Case toggle (the "AB" button in the find/replace widget and in the Search view, alt-cmd-p on macOS and alt-p elsewhere) is missing: replacing `foo` with `bar` cannot turn `Foo` into `Bar` and `FOO` into `BAR`. Verified: the find bar has only Aa, whole word and regex toggles (`crates/fe-workbench/src/find_bar.rs:75`, `:542`) and the search panel only `SearchOption::{Regex, MatchCase, WholeWord}` (`crates/fe-workbench/src/search_panel.rs:119`).

## Current state
- The find bar builds a `Replacement` from the replace input (`find_bar.rs:243`, `Replacement::parse(text, regex)` from fe-vim's search module) and applies it per match in `replace_next` / `replace_all` (`find_bar.rs:256`, `:329`).
- Project replace builds replacements in `search_panel.rs` (`confirm_replace_all_with` :1003, `replace_in_file` :1020, `replace_in_folder` :1028).
- Bindings: find bar toggles at `crates/fe-keymap/keymaps/fastedit.toml:1313-1326`, search panel at `:646-660`; `alt-p` / `alt-cmd-p` are unused.

## Change
- New pure function module `crates/fe-workbench/src/preserve_case.rs`: `apply(matched: &str, replacement: &str) -> String` implementing VS Code's `buildReplaceStringWithCasePreserved`: matched all upper-case (and has a letter) -> replacement upper-cased; all lower-case -> lower-cased; otherwise, if the matched text's first letter is upper-case, upper-case the replacement's first letter, and when the matched text has `-` or `_` separated parts with matching case pattern per part and the replacement has the same separator, apply the case per part; anything else leaves the replacement as written. Confirm the exact rules against VS Code's source and list the cases in the tests.
- Find bar: a "preserve case" toggle (button labelled `AB`, selector like the other toggles), command `find.toggle_preserve_case` ("Toggle Preserve Case"), key `alt-cmd-p` on macos and `alt-p` elsewhere, context `find_bar`. When on, Replace and Replace All apply `preserve_case::apply` to each match's replacement text after capture expansion. One undo step for Replace All as today.
- Search panel: the same toggle, `search.toggle_preserve_case`, same keys with context `search_panel`; honoured by Replace in file / folder / all and by the preview of replacements if the panel shows one.
- Off by default and not persisted, as the other toggles.

## Tests
New `crates/fe-workbench/src/preserve_case_tests.rs` (pure), `crates/fe-workbench/src/find_preserve_case_tests.rs`, `crates/fe-workbench/src/search_preserve_case_tests.rs`, `crates/fe-keymap/tests/preserve_case_keys.rs`; catalog tests updated.
