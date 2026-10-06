---
task_uid: "01m48tvj6v8fz0fdyrtjr4rrw6"
task: "T105"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-nav test tests/provider.rs::exact_provider_takes_over_when_ready: with SearchNavigation at precedence 0 and a fake Exact provider at 10, NotYet routes definitions to search (Answer.provider \"search\", precision Search); flipping the fake to Ready routes to it and search is not called; Ready for rust but Unsupported for python routes per request language"
    required: true
    manual: false
  - id: "C2"
    text: "fe-nav test tests/provider.rs::fallthrough_and_final_answers: the fake returning Err(Unavailable) makes search answer; the fake returning Ok with no items is final and search is not called; a provider added or removed through \u0026self after requests have run is used from the next request; cancel set before the call gives Err(Cancelled) without calling any provider"
    required: true
    manual: false
  - id: "C3"
    text: "fe-nav test tests/definition.rs::same_file_beats_imports_beats_project: in a Rust fixture where src/main.rs has use crate::util; and calls helper() and parse(), parse defined in main.rs and src/lexer.rs ranks main.rs first (SameFile) and unambiguous is true; helper defined in src/util.rs and src/zzz/helper.rs ranks util.rs first with tier Imported and unambiguous true; two Project-tier candidates give unambiguous false"
    required: true
    manual: false
  - id: "C4"
    text: "fe-nav test tests/definition.rs::kind_hint_orders_candidates: in TypeScript with class Item in models.ts and function Item in helpers.ts (neither imported), the caret on Item in let x: Item ranks the class first and on Item() ranks the function first"
    required: true
    manual: false
  - id: "C5"
    text: "fe-nav test tests/definition.rs::words_comments_and_definitions: the caret right after helper still finds helper; a word with non-ASCII letters (größe) is found whole; $store is one word in JavaScript; the caret on helper inside a // comment or a string gives no items; the caret on the name of fn helper gives exactly that definition"
    required: true
    manual: false
  - id: "C6"
    text: "fe-nav test tests/definition.rs::unsaved_text_is_used_for_the_same_file: a definition present only in NavRequest.text (not on disk) is found as SameFile with offsets into that text, and the disk copy of the file is not listed"
    required: true
    manual: false
  - id: "C7"
    text: "fe-nav test tests/definition.rs::ripgrep_fallback_before_the_index_is_complete: with an empty incomplete SymbolIndex, go-to-def finds a definition in another file with partial set, extracting only files that contain the word (at most 64); with a complete index lacking the word, the answer is empty and no search ran (a test-visible stat says so)"
    required: true
    manual: false
  - id: "C8"
    text: "fe-nav test tests/definition.rs::workspace_symbols_from_the_index: Navigation::workspace_symbols(\"hlp\", 10) returns helper with its location and char indices, answered by search"
    required: true
    manual: false
  - id: "C9"
    text: "scripts/check-no-gpui.sh prints ok: fe-nav; no Cargo.toml or Cargo.lock change; no unsafe in crates/fe-nav; NavigationProvider and Navigation are Send + Sync (static assertion in a test)"
    required: true
    manual: false
---

# Answer go-to-definition and workspace symbols by search, behind a NavigationProvider seam

# Answer go-to-definition and workspace symbols by search, behind a NavigationProvider seam

Milestone M4 "Navigate". The plan's extension seams include "`NavigationProvider` (search-based →
LSP → SCIP later)", and M4 is done when "go-to-def works instantly by search, then precisely via
rust-analyzer, pyright and tsserver", within "go-to-def under 100 ms before the LSP is ready". This
card defines that seam in `fe-nav` and the search-based provider's go-to-definition and workspace
symbols over T103's `SymbolIndex`. References are the next card; the UI is later. The `fe-lsp`
design (T100) will plug its provider into this seam.

## Problem

Go-to-def by search has to pick the right one of many same-named definitions, answer before the
index is complete, and give way to a precise provider once one is ready for the file's language,
without the UI knowing which provider answered beyond a label.

## Change

### `crates/fe-nav/src/provider.rs`: the seam

- Types:
  - `Precision { Search, Exact }`.
  - `Readiness { Ready, NotYet, Unsupported }`.
  - `NavRequest { root: PathBuf, rel: PathBuf, language: Option<&'static str>, text:
    fe_buffer::Text, offset: usize }`: the active buffer's current text (unsaved edits included)
    and the caret's byte offset in it.
  - `Location { rel: PathBuf, range: Range<usize>, row: u32 }`: bytes in the file's loaded text
    (T103's convention). For the request's own file, offsets are into `request.text`.
  - `Definition { location, name: String, kind: OutlineKind, container: Option<String>, tier:
    Tier }` with `Tier { SameFile, Imported, Project }` (ordered).
  - `Definitions { word: Option<String>, items: Vec<Definition>, unambiguous: bool, partial:
    bool }`: `unambiguous` when there is exactly one item, or the first item's tier is strictly
    better than the second's; the UI jumps straight there, otherwise lists them. `partial` when the
    answer came while the index was incomplete.
  - `NavError { Unavailable(String), Cancelled, Unsupported }`.
- `pub trait NavigationProvider: Send + Sync`:
  - `fn name(&self) -> &str`, `fn precision(&self) -> Precision`,
    `fn readiness(&self, language: Option<&str>) -> Readiness`;
  - `fn definitions(&self, req: &NavRequest, cancel: &AtomicBool) -> Result<Definitions, NavError>`;
  - `fn workspace_symbols(&self, query: &str, limit: usize, cancel: &AtomicBool) ->
    Result<Vec<WorkspaceSymbol>, NavError>` (`WorkspaceSymbol { location, name, kind, container,
    indices }`).
  - Calls are **blocking and run on a caller-owned background thread**, with an `AtomicBool`
    cancel, the same convention as `fe_project::search` and T103's `build`. An async provider (an
    LSP client) blocks on its own response inside the call, so `fe-nav` stays runtime-free.
- `pub struct Navigation` (the router), `Send + Sync`:
  - `add(&self, provider: Arc<dyn NavigationProvider>, precedence: i32)` and `remove(&self, name)`
    take `&self` (an `RwLock` inside), so a host can register an LSP provider after startup.
  - A request goes to the highest-precedence provider whose `readiness(language)` is `Ready`. If it
    returns `Err(Unavailable)` or `Err(Unsupported)` (it doesn't do this operation) the next `Ready`
    one is asked; `Cancelled` and `Ok` answers are final,
    **including an empty `Ok`** (an exact provider saying "no definition" isn't second-guessed).
  - **Replace, don't merge:** one provider answers each request. The answer says which:
    `Answer<T> { provider: String, precision: Precision, value: T }`, so the UI can label "by search".
  - `cancel` is checked before each provider is asked. Cancelling a stale request is the caller's
    job: it sets the old request's flag when it starts a new one.

### `crates/fe-nav/src/search.rs`: `SearchNavigation`

- `SearchNavigation::new(root, index: Arc<RwLock<SymbolIndex>>)`; `precision()` is `Search`;
  `readiness` is `Ready` for every language (it degrades rather than waits).
- **The word** at `offset`: the maximal run of identifier chars (`char::is_alphanumeric` or `_`,
  plus `$` for javascript, typescript and tsx) touching the offset, so a caret just after a word
  counts. No word gives `Ok` with `word: None` and no items.
- **Parse the request's text once** with its language (when supported, under its parse limit):
  - If the word lies in a comment or string (every byte under a `comment*` or `string*` highlight
    from `Syntax::highlights`), there are no items.
  - A **kind hint** from the highlight on the word: `function*` or `constructor` → callable
    (Function, Method, Macro); `type*` → type (Struct, Class, Enum, Interface, Trait, Type);
    `module` → Module; `property` → Field or Method; anything else → none.
  - **Same file:** `symbols()` of the request text, items named the word. If the caret is inside
    one's `name_range`, the answer is that definition alone.
- **Project:** `index.definitions(word)`, minus the request's own `rel` (the live text replaces it).
- **Before the index is complete** (`!is_complete()`), add a ripgrep fallback:
  `fe_project::search` for the word (literal, case-sensitive, whole word, `max_matches` 2,000),
  then `extract` (T103) the first 64 matched files whose language has symbols, keeping symbols
  named the word, merged with what the partial index already has and deduplicated by (rel,
  name_range). Stop extracting once 80 ms have passed since the request started and set `partial`.
  A complete index is authoritative: no search runs.
- **Ranking**, by key in order:
  1. tier: `SameFile`, then `Imported`, then `Project`;
  2. kind matches the hint;
  3. same directory as the request file;
  4. more leading path components shared with the request file;
  5. rel path, then start offset.
- **Imported** means the candidate file's module name occurs as a whole word on an import line of
  the request text. The module name is the file stem, or the parent directory's name for stems
  `mod`, `lib`, `main`, `index` and `__init__`. Import lines, by language: rust `use`, `pub use`,
  `mod`, `extern crate`; python `import`, `from`; javascript/typescript/tsx `import`, `export … from`,
  `require(`; go the lines of `import` declarations; java `import`; c `#include`. A small table in
  `search.rs`; other languages have no imports tier.
- At most 50 items.
- `workspace_symbols` is `SymbolIndex::fuzzy` mapped to `WorkspaceSymbol`; empty while the index has
  nothing yet.

### `crates/fe-nav/src/lib.rs`

`mod` lines and re-exports.

### Tests (new files)

- `crates/fe-nav/tests/provider.rs`: the router, with a fake exact provider (counters of calls,
  settable readiness and result).
- `crates/fe-nav/tests/definition.rs`: `SearchNavigation` over `tempfile` fixtures, with the index
  built by T103's `build`, or left incomplete to exercise the fallback.

## Out of scope

- References (next card), the UI, cmd-click, peek views, benches (perf card).
- Any LSP code. `fe-lsp` implements `NavigationProvider` in its own cards.
- Import resolution beyond the module-name heuristic (no path resolution, no `tsconfig` paths).

## Decided (lead defaults; the user may revisit)

- **Blocking provider calls on a background thread with an `AtomicBool` cancel**, not futures: no
  runtime in `fe-nav`, and it matches `fe-project`. T100 may ask to revisit if `fe-lsp` wants async.
- **Precedence, readiness per language, replace-not-merge, empty exact answers are final.**
- **Ranking: same file > imports > project**, then kind hint, then path proximity.
- Before the index is complete, a capped ripgrep pass (64 files, 80 ms) keeps go-to-def inside the
  100 ms budget instead of waiting for the build.
