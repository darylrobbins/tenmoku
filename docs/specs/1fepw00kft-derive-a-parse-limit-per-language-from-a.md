---
task_uid: "01m41yfw27w8az4d1fepw00kft"
task: "T42"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax: TREE_BUDGET_BYTES is 300 MiB and, for every name in language_names(), Language::parse_limit() equals TREE_BUDGET_BYTES / its ratio from the test's table (rust 40, json 30, markdown 85, any other 40), with rust 7_864_320, json 10_485_760 and markdown 3_700_856 bytes, each gated on its feature (test tests/parse_limit.rs::limits_follow_the_tree_budget)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax: for rust, json and markdown, text of exactly parse_limit() bytes has needs_parse() true and window_job None; at parse_limit()+1 bytes needs_parse() is false and window_job is Some; inserting one byte into the at-limit buffer and calling Syntax::edit flips it. No parse is run (test tests/parse_limit.rs::one_byte_over_the_limit_is_not_parsed)"
    required: true
    manual: false
  - id: "C3"
    text: "MAX_PARSE_BYTES is gone (rg -n MAX_PARSE_BYTES crates finds nothing). Above a language's limit the existing 32 KB viewport-window path is unchanged, and its tests (src/syntax/window_tests.rs, src/driver/window_tests.rs, fe-gpui src/view/window_tests.rs) pass when sized from the Rust limit. cargo test -p fe-syntax and cargo test -p fe-gpui pass, and cargo bench -p fe-syntax --no-run builds"
    required: true
    manual: false
  - id: "C4"
    text: "Docs: PERF.md's M2 memory target covers the text only (≤ 1.5× file size) plus a ≤ 300 MB tree cap. Its 'Tree memory' section gives a per-language limit table and says the constants in crates/fe-syntax/src/parse_limit.rs come from that section's ratios. The 16 MB footprint rows are labelled as measured at the old 16 MB limit (no fastedit --perf re-measurement). docs/decisions/mmap-read-only.md:62 no longer cites MAX_PARSE_BYTES"
    required: true
    manual: false
---

# Derive a parse limit per language from a 300 MB syntax-tree budget, replacing MAX_PARSE_BYTES

Follow-up to T3 (decided by the user, 2026-10-03). Source: Paddock T-26; also settles T-27's
"consider a lower parse limit for Markdown".

## Decision being implemented

- The ≤ 1.5× file-size memory target covers the text only. The syntax tree gets an absolute cap
  of **300 MB** (`300 * 1024 * 1024`, the same MiB convention `MAX_PARSE_BYTES` uses today).
- Each `Language` gets its own parse limit, `floor(TREE_BUDGET_BYTES / tree_ratio)`. This replaces the
  global `MAX_PARSE_BYTES` (8 MiB, `crates/fe-syntax/src/syntax.rs:26`).
- Above a language's limit, nothing changes from today except the threshold. **Today** means the
  T22/T24 path (finalized): no full tree, and a 32 KB window around the viewport is parsed
  (`Syntax::window_job`, `SyntaxDriver::sync_window`). Keep that path as it is. Don't remove it and
  don't make it plain text.

## Ratios: hard-coded, cited

Put the ratios in source as constants, with a doc comment that cites PERF.md, "Tree memory" (malloc
growth across a parse, printed by `cargo bench -p fe-syntax`). Don't measure them at runtime. Use the
largest typical ratio for each language, which gives the numbers T3 quoted:

| Language | Ratio | Source row in PERF.md "Tree memory" | Limit (bytes) |
|---|---|---|---|
| rust | 40 | Rust, generated items | 7,864,320 (7.5 MiB) |
| json | 30 | JSON, pretty-printed objects | 10,485,760 (10 MiB) |
| markdown | 85 | Markdown, short paragraphs | 3,700,856 (~3.5 MiB) |
| every other language | 40 | not measured; uses the code ratio (Rust) | 7,864,320 |

A JSON array of single digits (96×) still reaches ~960 MB at 10 MiB. T3 accepted this: per-language
limits bound the typical file, not the worst. Say so in the doc comment.

## Where

- **New `crates/fe-syntax/src/parse_limit.rs`.** It holds `pub const TREE_BUDGET_BYTES`, a private
  `(name, ratio)` table with a 40× default, and `impl Language { pub fn parse_limit(&self) -> usize }`,
  which is keyed on `self.name()`. The table lives here, not as a field on `Def` in `language.rs`,
  because `language.rs` is in T18's scope (running) and in the scopes of T19, T20 and T34. A grammar
  added later gets the default until someone measures it.
- **`crates/fe-syntax/src/lib.rs`:** add `mod parse_limit;`, export `TREE_BUDGET_BYTES`, and stop
  exporting `MAX_PARSE_BYTES`.
- **`crates/fe-syntax/src/syntax.rs`:** delete `MAX_PARSE_BYTES` and use
  `self.language.parse_limit()` at each of its uses (`edit`, `needs_parse`, `window_job`, `finish`).
  Move the memory explanation from its doc comment to `parse_limit.rs`.
- **`crates/fe-syntax/src/driver.rs`:** the `sync` check (around line 103) and the doc comments.
- **`crates/fe-gpui/src/view.rs:403`:** decide whether to defer the window sync from the driver's
  language limit. With no syntax there's no window to ask for.
- **Existing fixtures:** size them from the language's limit instead of the constant.
  - `crates/fe-syntax/src/syntax/window_tests.rs` and `crates/fe-syntax/src/driver/window_tests.rs`
    are Rust.
  - `crates/fe-gpui/src/view/window_tests.rs` is Rust, `main.rs`.
  - In `crates/fe-syntax/benches/syntax.rs` and `crates/fe-syntax/benches/flat.rs`, each corpus fills
    to its own language's limit minus 1 KB.
- **`docs/decisions/mmap-read-only.md:62`:** "`MAX_PARSE_BYTES` is 8 MiB" becomes "every language's
  parse limit is at most 10 MiB". The point still holds against the 512 MiB mapping threshold.

## Tests (new file, deterministic, no parse)

Write `crates/fe-syntax/tests/parse_limit.rs`. It mustn't parse a file at the limit: a debug-build
parse of MBs is slow, and a counting-allocator check isn't deterministic here. Tree-sitter allocates
through C malloc, the benches' `malloc_zone_statistics` is process-wide, and tests run in parallel.
The benches already print tree memory at each limit.

1. `limits_follow_the_tree_budget`:
   - `TREE_BUDGET_BYTES == 300 * 1024 * 1024`.
   - For every name in `language_names()`, `parse_limit()` equals `TREE_BUDGET_BYTES / ratio`, with
     the ratio taken from the test's own table: rust 40, json 30, markdown 85, any other 40.
   - The literal values 7_864_320, 10_485_760 and 3_700_856 hold for rust, json and markdown. Gate
     each on `#[cfg(feature = "...")]`, as `tests/yaml_long_list.rs` does.
2. `one_byte_over_the_limit_is_not_parsed`, for rust, json and markdown:
   - With text of exactly `parse_limit()` bytes, `Syntax::needs_parse()` is true and
     `window_job(0..len)` is `None`.
   - With text of `parse_limit() + 1` bytes, `needs_parse()` is false and `window_job` is `Some`.
   - Inserting one byte into the at-limit buffer and calling `Syntax::edit` flips it from the first
     case to the second.
   - Use a cheap repeated line for the text. Build each buffer once.

## Docs (PERF.md, no re-measurement)

- In "M2 large-file targets", the memory row covers the text only (≤ 1.5× file size) and adds the
  tree cap (≤ 300 MB under each language's parse limit). Update the paragraph below the table
  (line 71: "`MAX_PARSE_BYTES` (16 MB)") to name per-language limits.
- In "Tree memory", replace the final paragraph about the 8 MB limit with the per-language limit
  table above. Say the constants in `crates/fe-syntax/src/parse_limit.rs` come from this section's
  ratios.
- In "Memory (physical footprint)", label the 16 MB rows as measured at the old 16 MB global limit.
  Don't re-measure footprint with `fastedit --perf` here.
- In "Flat files", note that the Markdown row was filled to the old 8 MB limit, so the bench now fills
  to ~3.5 MiB. Leave its numbers as they are.

## Out of scope

- Re-measuring with `fastedit --perf`.
- Ratios for unmeasured languages.
- Viewport-only highlighting changes.
- The Markdown keystroke reparse (T33).
