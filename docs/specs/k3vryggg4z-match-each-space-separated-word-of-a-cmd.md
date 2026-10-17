---
task_uid: "01m54cv2sexs9439k3vryggg4z"
task: "T129"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-project test finder_query_words::space_separated_words_each_match_the_path passes: over a project of src/editor/main.rs, src/lib.rs and docs/main.md, \"main rs\", \"rs main\", \"editor main\" and \"src main rs\" return src/editor/main.rs and not src/lib.rs or docs/main.md, \"main md\" returns docs/main.md only, and \"main zzz\" returns nothing; \"main \", \" main\" and \"main  \" return the same paths in the same order as \"main\"; \"   \" returns what \"\" returns; every result's indices are ascending, without duplicates, and include for each word chars of the path that spell it; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-project -p fe-workbench passes with no existing test edited (one-word queries rank and highlight as before); cargo clippy -p fe-project --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Match each space-separated word of a Cmd-P query on its own, as VS Code's quick open does, instead of matching nothing once the query holds a space (Mac T750)

## Problem

Cmd-P matches nothing as soon as the query holds a space. `FinderIndex::query`
(crates/fe-project/src/finder.rs) hands the whole text to one frizbee `Matcher`, and its doc says
"every character of `text` must appear in the path, in order", spaces included. The workbench's quick
open (crates/fe-workbench/src/quick_open.rs) passes the typed query through `split_location` only, so
the space reaches the matcher.

Reproduced at Mac main 80e6c76d with a project holding `src/editor/main.rs` and `src/lib.rs`:
`"main rs"`, `"editor main"`, `"src main"`, and even `"main "` or `" main"` all return no paths, while
`"main.rs"` finds `src/editor/main.rs`.

VS Code's quick open treats spaces as separating search words (src/vs/base/common/fuzzyScorer.ts,
`prepareQuery`: the query splits on `' '`, empty pieces are dropped, whitespace is removed from each
piece, and `scoreItemFuzzy` needs every piece to match the item, adding up their scores). So
`main rs` opens `main.rs`, and a stray trailing space changes nothing.

## Change

In crates/fe-project/src/finder.rs, make `FinderIndex::query` split `text` on whitespace into words,
dropping empty ones:

- No word (empty or whitespace-only text): as an empty `text` today, the first `limit` paths in path
  order.
- One word: exactly today's matching, ranking and indices for that word (so `"main "` gives what
  `"main"` gives).
- Several words: a path matches only when every word matches it, each word matched as a single word
  is today (in the file name, with `NAME_TIER`, when the word has no `/` and matches there, else
  across the path). Its score is the saturating sum of the words' scores; ranking is otherwise as
  today (score, then shorter path, then path order). Its `indices` are the union of the words'
  indices, ascending and without duplicates.

Update `query`'s doc comment to say so. The file-name shortcut ("with enough file name matches,
nothing else can make the cut") may only be kept for the one-word case. No change outside
crates/fe-project: quick_open.rs already shows `indices` as highlights whatever their count.

The regression test is a new integration test, crates/fe-project/tests/finder_query_words.rs, whose
test fn sits at the top level of the file; it builds its fixture with `tempfile` and
`FinderIndex::walk` as crates/fe-project/tests/finder_dir_replaced_by_file.rs does.
