---
task_uid: "01m48tw9k4bbqh0mt3cd9n8g88"
task: "T106"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-nav test tests/references.rs::code_uses_only: over Rust files where helper appears in calls, in a // comment, in a /* block */ comment, in a \"helper\" string, and as helper_two and Helper, only the calls and the definition are reported (whole word, case-sensitive); dropped counts the comment and string hits; the definition hit has is_definition set and the others do not"
    required: true
    manual: false
  - id: "C2"
    text: "fe-nav test tests/references.rs::offsets_index_the_loaded_text: in a CRLF file and a file with multi-byte chars before the hit on the same line, each verified reference range selects exactly the word in the text Buffer::from_reader builds; preview_ranges select it in preview"
    required: true
    manual: false
  - id: "C3"
    text: "fe-nav test tests/references.rs::the_request_file_comes_first_from_unsaved_text: a use present only in NavRequest.text is reported in the first sink batch with offsets into that text, and the disk copy of the request file is not reported"
    required: true
    manual: false
  - id: "C4"
    text: "fe-nav test tests/references.rs::unverified_past_the_budget_or_language: with verify_files 2 over 5 files that each contain a comment hit, 2 files are verified (comment hit dropped) and 3 report it with verified false; hits in a .txt file are reported unverified; files_verified counts 2 plus the request file"
    required: true
    manual: false
  - id: "C5"
    text: "fe-nav test tests/references.rs::streams_and_cancels: each sink batch groups whole files; cancel set inside the first sink call stops the search, sink is not called again and summary.cancelled is set; with max hits exceeded truncated is set"
    required: true
    manual: false
  - id: "C6"
    text: "fe-nav test tests/references.rs::router_routes_references: a Ready fake Exact provider that implements references answers instead of search; a Ready fake that keeps the default body (Err(Unsupported)) falls through to search, which answers"
    required: true
    manual: false
  - id: "C7"
    text: "scripts/check-no-gpui.sh prints ok: fe-nav; no Cargo.toml or Cargo.lock change; no unsafe in crates/fe-nav"
    required: true
    manual: false
---

# Find references by ripgrep word search, dropping hits in comments and strings

# Find references by ripgrep word search, dropping hits in comments and strings

Milestone M4 "Navigate". The plan gives `fe-nav` references by "ripgrep". This card adds
`references` to T105's `NavigationProvider` seam and implements it in `SearchNavigation` with T65's
`fe_project::search`, filtered by syntax. GPUI-free; the UI card shows the results.

## Problem

A plain word search for `parse` also finds `// parse the header` and `"parse error"`. Filtering needs
a parse of every matched file, which is unbounded on a common name, so the filter needs a budget and
results must stream.

## Change

### `crates/fe-nav/src/provider.rs`

- `Reference { location: Location, line_number: u64, preview: String, preview_ranges:
  Vec<Range<usize>>, verified: bool, is_definition: bool }` and `FileReferences { rel, refs }`.
- `NavigationProvider::references(&self, req, cancel, sink: &mut dyn FnMut(Vec<FileReferences>))
  -> Result<ReferencesSummary, NavError>`, with a default body returning `Err(Unsupported)` so a
  provider can omit it. `Navigation::references` routes like `definitions` (T105's rules: `Unsupported` falls through
  to the next `Ready` provider).
- `ReferencesSummary { word, files_searched, files_verified, dropped: usize, truncated, cancelled }`.

### `crates/fe-nav/src/references.rs` (new), used by `SearchNavigation::references`

- The word comes from T105's word rule; no word gives an empty summary.
- **The request's own file first**, from `request.text` (it may be unsaved): every whole-word,
  case-sensitive occurrence, filtered by the request text's parse, emitted as the first batch. Its
  disk copy's hits are skipped.
- **The rest** from `fe_project::search` (literal, case-sensitive, whole word, `max_matches`
  10,000, default threads), streamed: each `FileMatches` batch is processed as it arrives.
- **Verifying a file:** load it as the loader does (`Buffer::from_reader`), parse it if its language
  is supported and it is under its parse limit, and turn each hit's `line_number` and in-line range
  into an offset in the loaded text (row start + in-line range, as T77 does). Then:
  - drop the hit if every byte is under a `comment*` or `string*` highlight (counted in `dropped`);
  - set `is_definition` when it equals the `name_range` of a `symbols()` item of that file.
  Hits in files that can't be verified (unsupported language, too large, or past the budget) are
  kept with `verified: false`.
- **Budget:** at most `ReferenceOptions::verify_files` files are parsed per request (default 1,000;
  tests set it low). Parsing happens on the calling thread as batches arrive.
- `preview` and `preview_ranges` come from T65's `LineMatch` (`preview_start` applied), so the UI
  can highlight without reloading.
- `cancel` stops the search and verification; once set, `sink` isn't called again.
- `truncated` mirrors the search's.

### `crates/fe-nav/src/search.rs` and `lib.rs`

`SearchNavigation::references` delegating to `references.rs`; `mod` and re-export lines.

### Tests (new file)

`crates/fe-nav/tests/references.rs`, over `tempfile` fixtures.

## Out of scope

- Scoping references to the right symbol (same-named locals in other scopes are reported), renames,
  the UI, unsaved buffers other than the request's own.
- Parallel verification; the perf card measures whether it is needed.

## Decided (lead defaults; the user may revisit)

- **Word search plus a syntax filter**, not an index of references: no extra memory, always fresh
  from disk, streamed.
- At most 1,000 files are verified per request; the rest are shown unverified rather than dropped.
- Case-sensitive, whole word, capped at 10,000 hits like project search.
