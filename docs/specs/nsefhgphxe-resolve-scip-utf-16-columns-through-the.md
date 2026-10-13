---
task_uid: "01m4synt16hf56mdnsefhgphxe"
task: "T95"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "The new crates/fe-nav/tests/scip_long_line.rs has a test named exactly utf16_occurrences_on_a_long_row_resolve_quickly (no other name): 20,000 UTF-16 occurrences on one row, occurrence_at at the last finds it in under 5 s; it fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "A test named exactly utf16_columns_that_split_a_surrogate_pair_are_refused passes as the body describes; UTF-8 and UTF-32 handling are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Every existing fe-nav test passes unchanged"
    required: true
    manual: false
---

# Resolve SCIP UTF-16 columns through the rope's UTF-16 metric instead of walking the row per occurrence, so a long generated row doesn't hang go-to-definition (Mac T627)

{
  "mode": "inline",
  "body": "## Problem\n\n`to_offset` in crates/fe-nav/src/scip.rs (around line 576) turns a SCIP `(line, column)` into a byte\noffset. For UTF-16 columns, which are also what an index gets when it names no encoding, and for UTF-32,\nit walks the row's chars from the row start up to the column on every call. `ScipIndex::occurrence_at`\n(around line 309) runs `byte_range` on every occurrence on the caret's row, and `ScipSite::byte_range`\ndoes the same for each definition and reference site. k occurrences on one row of length L therefore cost\nO(k·L), with no cancel check in between. A minified or generated JS bundle indexed by scip-typescript\n(UTF-16) is a realistic trigger: go-to-definition or find-references on that row hangs.\n\nA probe on a scratch copy of main (e6988aba, debug build) used one row `\"a;\"` × n with an occurrence\n`[0, 2i, 2i+1]` per `a` and called `occurrence_at` at the row's end. UTF-16 took 0.33 s at n=2k, 1.5 s at\n4k, 10.5 s at 8k and 24.6 s at 16k. UTF-8 stayed under 9 ms. With the change below, n=16k took 27 ms.\n`git log` shows no fix on main: scip.rs has only its original commits.\n\n## Change\n\nIn `to_offset`, convert UTF-16 columns through the rope's UTF-16 metric that `fe_text::Text` already\nexposes (`offset_to_utf16`, `utf16_to_offset`, both O(log n)) instead of walking chars:\n\n- `target = text.offset_to_utf16(line_start) + column`;\n- `None` if `target > text.offset_to_utf16(line_end)` (past the row's end, as now);\n- `offset = text.utf16_to_offset(target)`, and `None` unless `text.offset_to_utf16(offset) == target`,\n  so a column that splits a surrogate pair is still refused, as today.\n\nUTF-8 handling is unchanged. UTF-32 columns (rare in SCIP indexes) keep the char walk: fe_text::Text has no\npublic char-to-offset conversion, and adding one is out of this card's scope.\n\nRegression test: a new integration test file crates/fe-nav/tests/scip_long_line.rs, public API only\n(`ScipIndex::from_bytes`, `occurrence_at`, `fe_text::Text`; build the index with the `scip`/`protobuf`\ncrates as crates/fe-nav/tests/scip_index.rs does). It must contain a test named exactly\n`utf16_occurrences_on_a_long_row_resolve_quickly`: one document with UTF-16 position encoding, text\n`\"a;\".repeat(20_000)`, an occurrence `[0, 2i, 2i + 1]` for each i, then `occurrence_at` at the last `a`\nfinds it in under 5 s (seconds at most after the fix even under heavy load; tens of seconds at base). A\nsecond test named exactly `utf16_columns_that_split_a_surrogate_pair_are_refused`: on the row `\"😀x\"` with\nUTF-16 occurrences `[0, 0, 2]` (symbol `s1`) and `[0, 1, 3]` (symbol `s2`), `occurrence_at` at byte 0 gives\n`s1`, and no offset in the row gives `s2`, as at base.",
  "ref": "",
  "ref_blob": ""
}
