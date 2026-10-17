---
task_uid: "01m551h14mnfhqqw9egvyzwypz"
task: "T138"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Test fe-project::finder_query_vscode_chars backslashes_wildcards_and_ellipses_read_as_vs_code_reads_them (tests/finder_query_vscode_chars.rs, top level) passes: over src/editor/main.rs, src/lib.rs and docs/main.md, FinderIndex::query with editor\\main, *.md, main*rs and src…lib each returns the file VS Code's quick open finds, with highlights on the normalized word (src\\lib highlights the path's src/lib); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "A word made only of * and … is no word (* alone lists paths as an empty query does); fe-project::finder_query_words space_separated_words_each_match_the_path and the --lib finder unit tests pass as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-project passes with no existing test edited; cargo clippy -p fe-project --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Quick open reads \ in a query as / and ignores * and …, as VS Code's quick open does, so src\lib.rs and *.md find their files instead of nothing (Mac T780)

## Problem

Quick open (`FinderIndex::query` in `crates/fe-project/src/finder.rs`) splits its query on whitespace,
as VS Code does, but otherwise matches each word as typed. VS Code's quick open first normalizes the
query (`normalizeQuery`, src/vs/base/common/fuzzyScorer.ts on microsoft/vscode main): on macOS and Linux
`original.replace(/\\/g, sep)` ("Help macOS/Linux users to search for paths when using backslash"), then
`replace(/[\*…\s"]/g, '')` drops wildcards (`*`) and ellipses (`…`).

Reproduced at bcb3ea60 with `FinderIndex::from_paths(["src/editor/main.rs", "src/lib.rs",
"docs/main.md"])`: `query(r"editor\main", 10)` returns nothing, though `query("editor/main", 10)` returns
`src/editor/main.rs`. `*.md`, `main*rs` and `src…lib` find nothing either, because no path holds a `\`,
`*` or `…`. In VS Code all four find the file. A Windows path pasted from a log or a chat (`src\lib.rs`)
and the habitual `*.md` both come up empty in fastedit.

## Change

In `crates/fe-project/src/finder.rs`, add `fn normalize_word(word: &str) -> Option<String>`, which maps
`\` to `/` and drops `*` and `…` (U+2026), returning `None` when nothing is left. `FinderIndex::query`
runs every whitespace-split word through it before anything else, so a normalized word holding `/` takes
the path-only route in `word_candidates`, highlights are computed for the normalized word (so `src\lib`
highlights the path's `src/lib`), and a word made only of wildcards is no word (`*` alone lists paths as
an empty query does). Quotes and VS Code's exact-match `"…"` queries are out of scope. Nothing changes in
fe-workbench: `quick_open.rs` already passes the query, minus any `:line:col`, to `query`.

Test in a new `crates/fe-project/tests/finder_query_vscode_chars.rs`.
