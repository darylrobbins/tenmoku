---
task_uid: "01m46r9w7gfxctvjpddv0sm4g1"
task: "T65"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-project test tests::search::literal_search_finds_every_match: a literal search for \"foo.bar(\" (regex metacharacters taken literally) on a nested fixture finds every occurrence with its /-separated rel path, 1-based line_number, line_offset and byte ranges whose text equals the needle; two hits on one line give one LineMatch with two ranges; each file arrives once with its lines in order; the summary counts files_searched, files_matched and matches"
    required: true
    manual: false
  - id: "C2"
    text: "fe-project test tests::search::search_skips_what_the_tree_hides: files matched by the root .gitignore, a nested .gitignore and an .ignore file, and anything under .git, are not searched, also outside a git repo; dotfiles are searched; with a needle present in every file, the matched files equal the paths FinderIndex::walk indexes over the same fixture"
    required: true
    manual: false
  - id: "C3"
    text: "fe-project test tests::search::regex_case_and_whole_word_options: the regex fo+\\d matches foo1 and fooo2; the default case-insensitive search finds FOO and case_sensitive does not; whole_word \"foo\" skips foobar and _foo but matches foo.bar and (foo); an invalid regex returns Err(SearchError::InvalidPattern) with 0 files searched and no sink call"
    required: true
    manual: false
  - id: "C4"
    text: "fe-project test tests::search::binary_and_long_lines_stay_bounded: a file containing a NUL byte is not reported and counts in binary_skipped; in a 1 MB single-line file with a match near its end, preview is at most 512 bytes on char boundaries, contains the match, and preview_start plus the match range in the preview gives the range in the line; invalid UTF-8 in a matched line gives a lossy preview without panicking"
    required: true
    manual: false
  - id: "C5"
    text: "fe-project test tests::search::results_stream_in_bounded_batches: with threads: 1 over 300 files of one match each, the sink is called 5 times with 64, 64, 64, 64 and 44 files; with default threads, every batch has at most 64 files and 1,000 matches, and the union of the batches equals the threads: 1 result with no file repeated"
    required: true
    manual: false
  - id: "C6"
    text: "fe-project test tests::search::match_cap_stops_the_search: over 1,000 files of 100 matching lines each with max_matches 500, exactly 500 match ranges are delivered and truncated is set; with threads: 1, files_searched == 5; with threads: 4, files_searched \u003c= 9"
    required: true
    manual: false
  - id: "C7"
    text: "fe-project test tests::search::cancel_stops_the_search: with cancel already set, 0 files are searched, the sink is never called and cancelled is set; over 300 files of one match each with threads: 1 and cancel set inside the first sink call, the sink is not called again, files_searched == 64 and cancelled is set; with default threads and the same cancel, the sink is not called again"
    required: true
    manual: false
  - id: "C8"
    text: "crates/fe-project/benches/search.rs builds in the gate and measures a rare literal, a capped common literal, a regex, a case-insensitive whole-word literal and time to first batch over a generated corpus of 10,000 files (~40 MB); PERF.md gains a Project search section with one run of these numbers next to the provisional 50 ms first-batch target (recorded, not asserted)"
    required: true
    manual: false
  - id: "C9"
    text: "cargo deny check licenses bans sources passes with grep-searcher, grep-regex and grep-matcher added; Cargo.lock gains no crates besides those three, encoding_rs, encoding_rs_io and (if not already present) bstr; scripts/check-no-gpui.sh prints ok: fe-project; crates/fe-project/src/search.rs and crates/fe-project/benches/search.rs contain no unsafe, and this task adds no unsafe anywhere in crates/fe-project (listing.rs's existing libc unsafe from T61 is out of scope)"
    required: true
    manual: false
---

# Search the project with ripgrep's grep crates, streaming capped and cancellable results

# Search the project with ripgrep's grep crates, streaming capped and cancellable results

Milestone M3 "Browse". The plan's Architecture table gives `fe-project` "ripgrep project search"
(GPUI-free) and `fe-workbench` the search panel. This card is the search engine only. The panel
is a later card on top of this one and T63.

It depends on T61 (the crate, `listing.rs`'s ignore rules, and the `tempfile` dev-dependency) and
on T62. T62 is listed only so that the `Cargo.toml`, `Cargo.lock` and `crates/fe-project` edits
land one after another.

## Problem

M3 is done when "finder and search budgets are met", and nothing searches file contents yet.
Project search has to:

- skip what the tree hides
- stream results while it walks, so the panel fills progressively
- stop when cancelled or when it has enough matches
- stay bounded on binary files, minified one-line files, and repos of 100k files

## Licensing (checked 2026-10-05, crates.io)

| Crate | Version | License |
|---|---|---|
| `grep-searcher` | 0.1.17 | Unlicense OR MIT |
| `grep-regex` | 0.1.14 | Unlicense OR MIT |
| `grep-matcher` | 0.1.9 | Unlicense OR MIT |
| `encoding_rs` | 0.8.x | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| `encoding_rs_io` | 0.1.7 | MIT OR Apache-2.0 |
| `bstr` | 1.x | MIT OR Apache-2.0 |

- `encoding_rs`, `encoding_rs_io` and `bstr` come in through `grep-searcher`. `bstr` is probably
  already in the lock by then, through T61's `ignore` → `globset`.
- Their other runtime dependencies are already in `Cargo.lock`: `memchr`, `memmap2`, `log`,
  `regex-automata`, `regex-syntax`, `aho-corasick` and `cfg-if`.
- Every license is on `deny.toml`'s allow list, so no policy change and no MPL.
- Don't use the `grep` facade crate. It also pulls in `grep-printer` and `grep-cli`, which we
  don't need.
- New crates in `Cargo.lock` need a person's approval at merge (`human_paths`), which is
  expected.

## Change

- Workspace `Cargo.toml`: `grep-searcher = "0.1"`, `grep-regex = "0.1"` and `grep-matcher = "0.1"`
  in `[workspace.dependencies]`. `crates/fe-project/Cargo.toml` uses them and adds
  `[[bench]] name = "search"`.
- `crates/fe-project/src/listing.rs`: the walk uses the **same ignore settings** as `FileTree` with
  `show_ignored: false`, and as T62's `FinderIndex`, including always excluding `.git`. If T62
  already shares a builder (e.g. a `pub(crate) fn walk_builder(root)`), use it. Otherwise extract
  one there; that's the only change to this file.
- New `crates/fe-project/src/search.rs`, declared and re-exported from `lib.rs`:
  - The query:
    - `SearchQuery { pattern: String, regex: bool, case_sensitive: bool, whole_word: bool }`.
      `regex: false` means a literal (`fixed_strings` or an escaped pattern).
    - `whole_word` uses `grep-regex`'s `word(true)`.
    - Searches are line-oriented: a pattern never matches across a line break.
  - `SearchOptions { max_matches: usize, threads: usize }`, with defaults of 10,000 and 0 (0 means
    the walker's default parallelism).
  - `search(root, &SearchQuery, &SearchOptions, cancel: Arc<AtomicBool>, sink: impl FnMut(Vec<FileMatches>))
    -> Result<SearchSummary, SearchError>`:
    - It runs on the calling (background) thread. The caller owns the thread, so the crate stays
      executor-free, like T62's `walk`.
    - An invalid pattern returns `SearchError::InvalidPattern(message)` before any IO.
  - Results:
    - `FileMatches { path: PathBuf, rel: String, lines: Vec<LineMatch> }`. `rel` is `/`-separated
      and lossy like the finder's.
    - All of a file's matches arrive together, in line order. Files arrive in walk-completion
      order. Sorting is the panel's job.
    - `LineMatch { line_number: u64 (1-based), line_offset: u64, ranges: Vec<Range<usize>>,
      preview: String, preview_start: usize }`:
      - `line_offset` is the line's byte offset in the file, so the editor can jump without
        counting lines.
      - `ranges` are every match on the line, as byte ranges relative to the line.
      - `preview` is the line, lossy UTF-8 and without its terminator. When the line is over 512
        bytes, it's a window of at most 512 bytes on char boundaries that contains the first
        match, and `preview_start` is where that window starts in the line. Ranges outside the
        preview are still reported.
  - **Streaming:**
    - Walker threads (`ignore::WalkBuilder::build_parallel`) search files and send `FileMatches`
      over a channel. The calling thread collects them and calls `sink`, so `sink` needs no
      `Send` and is never called concurrently.
    - A batch is flushed when it reaches 64 files or 1,000 matches, when the channel is empty
      for now, or when the walk ends. Files with no matches aren't sent.
    - `threads: 1` walks sequentially on the calling thread, in path order (`sort_by_file_name`),
      with batches flushed by count only. That makes it deterministic for tests.
  - **Cap:**
    - A shared match counter is checked by the workers. A worker stops searching its file once
      the cap is reached (the `Sink` returns `Ok(false)`) and starts no new file. The walk then
      quits.
    - The collector trims so that exactly `min(total, max_matches)` match ranges are delivered.
      When it trims, the last line keeps only the ranges that fit.
    - `summary.truncated` is set when the cap cut the search short.
  - **Cancel:**
    - `cancel` is checked before each file and at each match, and the walk then quits.
    - Once `cancel` is set, `sink` is not called again, including when it's set from inside
      `sink`. `summary.cancelled` is set.
  - **Bounds per file:**
    - Binary detection is `BinaryDetection::quit(b'\0')`: a file with a NUL byte is abandoned and
      counted in `summary.binary_skipped`.
    - No memory maps (`MmapChoice::never()`), so the crate needs no `unsafe` and a file truncated
      mid-search can't fault.
    - `heap_limit` is 16 MiB, so one enormous line can't take unbounded memory. Such a file is
      counted in `summary.errors` with its path, like an unreadable one, and the search goes on.
    - BOM-sniffed UTF-16 is transcoded, which is `grep-searcher`'s default.
  - `SearchSummary { files_searched, files_matched, matches, binary_skipped, errors: Vec<(PathBuf,
    String)>, truncated, cancelled }`. `files_searched` counts files whose search began, so tests
    can bound the work.
- Tests in a new `crates/fe-project/src/tests/search.rs`, declared from
  `crates/fe-project/src/tests/mod.rs`. Fixtures are written into a `tempfile` dir by the tests
  themselves. The cap and batch tests use `threads: 1` where they assert exact counts.
- Bench `crates/fe-project/benches/search.rs` (criterion, `harness = false`):
  - The corpus is generated once into a temp dir by the bench itself: 10,000 files of about
    4 KB of deterministic Rust-like lines (about 40 MB), with a `.gitignore`d directory.
  - It measures:
    - a rare literal (a handful of hits)
    - a common literal that hits the default cap
    - a regex (`fn \w+_\d+\(`)
    - a case-insensitive whole-word literal
    - time to first batch: `iter_custom` until the first `sink` call, which then sets `cancel`
  - Record one run in a new "Project search (`cargo bench -p fe-project --bench search`)" section
    of `PERF.md`, next to the provisional target below. Nothing is asserted.

Respect the workspace lints. `unreachable_pub` is warned and the gate runs clippy with
`-D warnings`.

## Out of scope

- The search panel, opening a result, and highlighting matches in the editor.
- Replace.
- Include and exclude globs.
- Searching unsaved buffer contents.
- Multiline patterns.
- Re-running a search on watcher changes.
- A max file size. `heap_limit` and binary detection bound the bad cases.

## Decided (lead defaults; the user may revisit)

- **Search what the tree shows:** gitignored files are never searched and dotfiles are, the same
  rules as `FileTree` (`show_ignored: false`) and the finder. There's no "search ignored files"
  toggle yet.
- **Defaults:** case-insensitive, literal, not whole-word.
- **At most 10,000 matches** per search (VS Code uses 20,000), with `truncated` reported.
- Results arrive grouped per file in completion order, not sorted.
- **No memory maps**, and a 16 MiB per-line heap limit.
- **Provisional PERF.md target:** first batch under 50 ms on the bench corpus, the same as the
  plan's finder budget, since the plan gives no number for search. Recorded only.
