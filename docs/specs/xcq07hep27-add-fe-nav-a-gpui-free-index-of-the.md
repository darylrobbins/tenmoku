---
task_uid: "01m48tscxrmdrcx6xcq07hep27"
task: "T103"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-nav test tests/extract.rs::extracts_definitions_with_loaded_offsets: for a Rust file with a struct, a field, an impl with a method and a fn, FileSymbols lists Struct, Field, Method (container \"impl Foo\") and Function but no Impl item; for each symbol the loaded text at name_range equals its name, also in a CRLF copy of the file; len and modified match the file metadata"
    required: true
    manual: false
  - id: "C2"
    text: "fe-nav test tests/extract.rs::skips_what_it_cannot_index: a .md, a .txt and a file with no extension give Skipped::Unsupported; a .rs file over the Rust parse limit gives TooLarge; a .rs file with a NUL byte in its first 8 KiB gives Binary; a missing file gives Unreadable(NotFound); a .rs file with invalid UTF-8 is indexed with offsets into the lossy text"
    required: true
    manual: false
  - id: "C3"
    text: "fe-nav test tests/index.rs::build_indexes_what_the_finder_lists: over a fixture with nested dirs, a .gitignored dir and a .git dir holding .rs files, the indexed files equal the supported-language paths FinderIndex::walk lists; no symbol from an ignored file or .git is found; BuildSummary counts files_seen, files_indexed and skips by reason; threads: 1 and default threads give the same sorted dump of every definition"
    required: true
    manual: false
  - id: "C4"
    text: "fe-nav test tests/index.rs::definitions_are_exact_and_ordered: definitions(\"helper\") returns every symbol named exactly helper (not Helper, helper_two or a container named helper) in (rel, start) order, across Rust, Python, TypeScript and Go files"
    required: true
    manual: false
  - id: "C5"
    text: "fe-nav test tests/index.rs::extend_replaces_and_remove_under_is_exact: extending a file again replaces its symbols (an old name is gone, a new one found); remove(rel) drops one file; remove_under(\"src/a\") drops src/a/x.rs and src/a/b/y.rs but keeps src/ab.rs; stats() files, symbols and distinct_names follow each step"
    required: true
    manual: false
  - id: "C6"
    text: "fe-nav test tests/index.rs::fuzzy_ranks_workspace_symbols: fuzzy(\"fb\", 10) over FooBar (struct), foo_bar (fn), fizzbuzz (fn) and Bar returns the first three with char indices into the name, ordered by score then kind (struct before fn at equal score); limit caps the results; an empty query returns nothing"
    required: true
    manual: false
  - id: "C7"
    text: "fe-nav test tests/index.rs::queries_see_files_indexed_so_far: after extend with a first batch and before finish, definitions finds that batch and is_complete() is false; after finish it is true"
    required: true
    manual: false
  - id: "C8"
    text: "fe-nav test tests/index.rs::cancel_stops_the_build: with cancel already set, sink is never called and summary.cancelled is set; over 600 files with threads: 1 and cancel set inside the first sink call, sink is not called again; no sink batch has more than 256 files"
    required: true
    manual: false
  - id: "C9"
    text: "scripts/check-no-gpui.sh prints ok: fe-nav; Cargo.lock gains only the fe-nav package (no new third-party crates); Cargo.toml gains only the fe-nav line; crates/fe-nav contains no unsafe; cargo clippy --workspace -D warnings passes"
    required: true
    manual: false
---

# Add fe-nav: a GPUI-free index of the project's definitions, built in parallel from fe-syntax symbols

# Add fe-nav: a GPUI-free index of the project's definitions, built in parallel from fe-syntax symbols

Milestone M4 "Navigate". The plan's Architecture table: "`fe-nav` | Search-based navigation (a tags
index plus ripgrep) for instant go-to-def, references and symbols. Hands off to the LSP once it's
ready | GPUI: no". This card creates the crate and its index: every definition in the project, by
name, built in the background. Go-to-def, references, the `NavigationProvider` seam, watcher
updates and the UI are later cards. It depends on T102 (`fe_syntax::symbols`) and on nothing in M3's
UI, so it can run as soon as T102 lands.

## Problem

Nothing knows where a name is defined outside the open file. Go-to-def "by search" needs a lookup
from a name to its definitions that answers in well under the plan's 100 ms, and workspace symbols
need a fuzzy query over all names. Both must be built off the UI thread, skip what the project tree
hides, and stay bounded on large repos.

## What exists (checked 2026-10-06)

- `fe_syntax::{language_for_path, Syntax, symbols, symbols_supported, OutlineKind}` (T64, T102).
  A file is parsed with `Buffer` → `Syntax::new` → `parse_job().run()` → `finish`.
- `fe_project::FinderIndex::walk(root, cancel, sink)` (T62): the project's paths, relative to the
  root, in batches, with the tree's ignore rules and `.git` excluded; executor-free, runs on the
  caller's thread with `ignore`'s parallel walker inside.
- `fe-workbench`'s loader opens a file with `Buffer::from_reader` (line endings normalised, invalid
  UTF-8 as U+FFFD). Offsets this index stores must index **that** text, so a jump lands right in a
  CRLF file.
- `frizbee` (MIT) is a workspace dependency already (T62's finder).

## Change

### Crate

- `crates/fe-nav/Cargo.toml`: depends on `fe-buffer`, `fe-syntax` (default features), `fe-project`
  and `frizbee`, all `workspace = true`; dev-dependency `tempfile`. `[lints] workspace = true`.
- Workspace `Cargo.toml`: one line, `fe-nav = { path = "crates/fe-nav" }`, in the internal crates
  block (append only). `Cargo.lock` gains the `fe-nav` package and nothing else.
- `crates/fe-nav/src/lib.rs`: crate docs (GPUI-free, executor-free; the caller owns threads, as in
  `fe_project::search`), `mod` lines and re-exports.

### `crates/fe-nav/src/extract.rs`: one file's definitions

- `pub fn extract(root: &Path, rel: &Path) -> Result<FileSymbols, Skipped>`:
  - The language comes from `language_for_path(rel)`; `Skipped::Unsupported` unless
    `symbols_supported`. No content sniffing.
  - `Skipped::TooLarge` when the file's length (from `metadata`, before reading) is over the
    language's parse limit.
  - Read with `Buffer::from_reader`, as the loader does. `Skipped::Binary` when the first 8 KiB hold
    a NUL byte. `Skipped::Unreadable(io::ErrorKind)` on IO errors.
  - Parse whole on this thread and take `symbols()`; a failed parse is `Skipped::ParseFailed`.
- `FileSymbols { rel: PathBuf, language: &'static str, len: u64, modified: Option<SystemTime>,
  symbols: Vec<Symbol>, truncated: bool }`. `len` and `modified` are from the same `metadata` call,
  for the watcher card to tell unchanged files apart.
- `Symbol { name: String, kind: OutlineKind, name_range: Range<usize>, range: Range<usize>, row:
  u32, container: Option<String> }`: offsets into the loaded text; `container` is the parent item's
  name (an `impl` parent gives its header, e.g. `impl Display for Foo`).
- `Impl`, `Heading` and `Table` items are not symbols (an `impl` is a header, not a definition);
  their children still are.

### `crates/fe-nav/src/index.rs`: `SymbolIndex`

- `SymbolIndex::new(root)`, `root()`.
- `extend(&mut self, batch: Vec<FileSymbols>)`: adds files; a file already present has its symbols
  **replaced**. `remove(rel)` removes one file; `remove_under(rel_dir)` removes every file under a
  directory (`src/a` doesn't match `src/ab`).
- `finish()` marks the build complete; `is_complete()`; `progress() -> (files, complete)`.
- `definitions(&self, name: &str) -> Vec<SymbolRef<'_>>`: exact, case-sensitive matches in (rel,
  start) order, from a hash lookup by name (no scan). `SymbolRef` exposes `rel`, `name`, `kind`,
  `name_range`, `range`, `row` and `container`.
- `file_state(rel) -> Option<(u64, Option<SystemTime>)>` and `file_symbols(rel)`.
- `fuzzy(&self, query: &str, limit: usize) -> Vec<SymbolMatch<'_>>` for workspace symbols:
  `frizbee` over the **distinct** names, then expanded to their symbols; `SymbolMatch { symbol,
  score, indices }` with `indices` as char indices into the name. Order: score descending, then
  kind (types, then functions and methods, then the rest), then shorter rel, then rel, then start.
  An empty query returns nothing.
- Compact storage: each distinct name stored once (interned), symbols as fixed-size records with
  `u32` offsets and ids (files over `u32::MAX` bytes are never parsed). Removed records may be
  tombstoned and compacted later; that's the implementer's choice. `stats() -> IndexStats { files,
  symbols, distinct_names, approx_bytes }`.
- `SymbolIndex` is `Send + Sync` (shared later behind an `RwLock`).

### `crates/fe-nav/src/build.rs`: building in the background

- `pub fn build(root, &BuildOptions, cancel: Arc<AtomicBool>, sink: impl FnMut(Vec<FileSymbols>))
  -> BuildSummary`, run on the caller's (background) thread:
  - `FinderIndex::walk` lists paths; only paths whose language has symbols go on to extraction
    (cheap filter before any IO).
  - `BuildOptions { threads: usize }`: 0 means half the available cores, at least 1, so a build
    never takes every core from the UI. Extraction runs on `std::thread::scope` workers fed as the
    walk produces paths, so walking and parsing overlap.
  - `sink` is called on the calling thread only, with batches of at most 256 files, so the caller
    can `extend` an index others are already querying.
  - `threads: 1` extracts on the calling thread in walk-batch order (deterministic enough for
    tests that compare the final index).
  - `cancel` is checked before each file; once set, `sink` isn't called again, including when set
    from inside `sink`.
- `BuildSummary { files_seen, files_indexed, skipped: SkipCounts (one count per `Skipped` reason),
  symbols, cancelled, elapsed }`.

### Tests (new files, integration tests on the public API)

`crates/fe-nav/tests/extract.rs` and `crates/fe-nav/tests/index.rs`. Fixtures are written into a
`tempfile` dir by the tests. Read `CARGO_MANIFEST_DIR` at run time if any test needs it.

## Out of scope

- Keeping the index current from watcher batches (next card), go-to-def ranking, references, the
  `NavigationProvider` seam, the UI, and benches (a later perf card).
- Persisting the index to disk between runs.
- Shebang detection, injected languages, unsaved buffer contents.

## Decided (lead defaults; the user may revisit)

- **Definitions come from T102's `symbols()`**, not from grammar `tags.scm` files or the
  `tree-sitter-tags` crate: no new dependency, one capture convention, and the same nesting the
  outline uses.
- **In memory only**, rebuilt per session; the build uses at most half the cores by default.
- Offsets are into the text as the loader builds it (normalised line endings), not disk bytes.
- No new crates in `Cargo.lock`.
