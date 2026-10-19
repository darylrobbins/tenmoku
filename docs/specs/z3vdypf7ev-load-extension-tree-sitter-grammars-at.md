---
task_uid: "01m55bz5n6vsdxh3z3vdypf7ev"
task: "T845"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "a_wasm_grammar_highlights_as_the_compiled_one: the fixture registered as json_wasm gives the same highlight spans as the built-in json on the same text, parsed on the main thread and by a ParseJob on another"
    required: true
    manual: false
  - id: "C2"
    text: "runtime_languages_are_found_after_built_ins: x.jsonw resolves to json_wasm and language_names lists it; a spec claiming the name rust or the extension json is refused for that clash; x.json is still the built-in"
    required: true
    manual: false
  - id: "C3"
    text: "bad_grammars_are_errors: garbage bytes, a module over 16 MiB and a highlights query error each give a GrammarError naming the file (the query's with row and column)"
    required: true
    manual: false
  - id: "C4"
    text: "extension_grammars_register: an extension with a [[contributes.grammars]] entry in a temp dir is registered with fe_syntax after kiln::init"
    required: true
    manual: false
  - id: "C5"
    text: "cargo deny check passes, and the finish notes give first-load time, full-parse and one-row-reparse times on a 10k-row file, and the release binary's size before and after"
    required: true
    manual: false
  - id: "C6"
    text: "a_looping_grammar_is_stopped_at_its_deadline: a fixture .wasm grammar (built in-test or checked in under tests/fixtures with its source noted) whose parse loops forever, and one whose module initializer loops forever, are each stopped at their deadline and return a GrammarError or parse failure naming the file, in a bounded time the test enforces by a bounded wait, and the process keeps running and parses other languages afterwards (crates/fe-syntax/tests/runtime_grammar.rs)"
    required: true
    manual: false
  - id: "C7"
    text: "a_bad_export_is_an_error_not_an_abort: a valid wasm module whose tree_sitter_\u003cname\u003e export is mistyped, and one where it is missing, each return a GrammarError naming the file instead of aborting the process (test)"
    required: true
    manual: false
  - id: "C8"
    text: "the vendored tree-sitter (vendor/tree-sitter, used through [patch.crates-io]) has a README.md recording its upstream version and commit, the wasm_store.c patch (epoch deadline + callback per store, engine epoch ticker thread, asserts after wasmtime calls turned into errors) and when to drop it, as vendor/tree-sitter-yaml/README.md does; THIRD_PARTY.md notes the patch; cargo deny check still passes"
    required: true
    manual: false
  - id: "C9"
    text: "the extensions decision record (docs/decisions/vmtnmkay62-extensions-and-plugins.md, the §6 sandbox text) and vendor/tree-sitter/README.md state the narrowed claim: Wasm code can't touch host memory, but a malformed grammar's tables can still crash the editor until T1005 validates them; both name T1005"
    required: true
    manual: false
---

# Load extension tree-sitter grammars at runtime as WebAssembly through tree-sitter's wasm feature

## Problem

Stage-1 extensions contribute tree-sitter grammars and their queries
(`[[contributes.grammars]]`, extensions record §2, §6), but every grammar is compiled in: one
Cargo feature each, a `static DEFS` table and `&'static Language` throughout
(`crates/fe-syntax/src/language.rs`), and each parse makes a fresh `Parser`
(`crates/fe-syntax/src/syntax.rs` `parse_tree` ~883, `imports.rs` ~474). The record decides how
grammars load at runtime. **This card is written for its recommendation, WebAssembly through
tree-sitter's `wasm` feature; if the user signs off another choice, respec it before it's
readied.**

## Change

- **Dependency.** `Cargo.toml`: `tree-sitter` with `features = ["wasm"]` (0.27 links
  `wasmtime-c-api-impl` 48, Apache-2.0 WITH LLVM-exception, which `deny.toml` allows);
  `Cargo.lock` follows. `cargo deny check` passes. `crates/fe-syntax/Cargo.toml` needs no new
  feature: the runtime loader is always built.
- **Loading.** New `crates/fe-syntax/src/runtime.rs` (`mod runtime;` and its exports in
  `crates/fe-syntax/src/lib.rs`): `pub struct GrammarSpec { language, wasm: PathBuf,
  extensions, file_names, highlights: Option<PathBuf>, injections: Option<PathBuf> }` and
  `pub fn register_language(spec: &GrammarSpec) -> Result<&'static Language, GrammarError>`:
  reads the module (at most 16 MiB), loads it into a `tree_sitter::WasmStore` on a shared
  `wasmtime::Engine`, compiles the queries with the same capture mapping as `Language::compile`,
  and leaks the result (one per module version loaded). Errors (bad module, ABI, query error at
  a row and column) come back as `GrammarError` with the file. A name, extension or file name
  a built-in has is refused for that clash (the built-in wins).
- **Lookup.** `language_by_name`, `language_name_for_path`, `language_names` and
  `injected_language` (`crates/fe-syntax/src/language.rs`) consult the registered languages
  after the built-ins.
- **Parsing.** A parser for a wasm language needs a `WasmStore`: `syntax.rs` and `imports.rs`
  take parsers from a per-thread cache that holds one store, set with `set_wasm_store`;
  compiled-in languages parse as today. Timeouts and `parse_limit` apply unchanged.
- **fastedit's kind.** `crates/fastedit/src/config.rs` `with_extensions`: `Config::extension_kind("grammars",
  schema)` (an array of `{ language, wasm, extensions, file_names, highlights, injections,
  license }`) and an `on_extensions` callback calling `fe_syntax::register_language` for each,
  off the main thread; a failure is logged with the extension's id. Paths resolve inside the
  extension's directory (as the manifest loader checks).
- **Budget.** Measure on the fixture grammar: first-load time, a full parse and a one-row
  reparse of a 10k-row file against the 5 ms reparse target, and the release binary's size
  before and after; record them in the finish notes (not `PERF.md`, which several inbox cards hold).
- **Fixture.** `crates/fe-syntax/tests/fixtures/tree-sitter-json.wasm`, built with
  `tree-sitter build --wasm` from the `tree-sitter-json` version Cargo.lock has (MIT), with its
  licence and the command in `crates/fe-syntax/tests/fixtures/tree-sitter-json.wasm.md`. The test
  registers it as language `json_wasm` for `*.jsonw` (the built-in `json` keeps its name) and
  compares its highlights with the compiled-in JSON's on the same text.

Test: `crates/fe-syntax/tests/runtime_grammar.rs`.

## Out of scope

Query overrides for built-in languages (a later card, no wasm needed); replacing a built-in
language; unloading a registered language while documents use it.

## Decision 2026-10-19

The user chose option (a): vendor tree-sitter 0.27 as `vendor/tree-sitter` (as
`vendor/tree-sitter-yaml` is vendored, through `[patch.crates-io]`) with a `wasm_store.c` patch,
to meet the extensions record §6 prerequisite (an enforceable execution deadline):

- each wasm store gets an epoch deadline and an epoch-deadline callback that checks a per-parse /
  per-module-init deadline and traps when it has passed;
- one engine epoch ticker thread increments the epoch;
- the `ts_assert(!error)`s after `wasmtime_func_call` (module initializer, `tree_sitter_<name>`)
  become errors, so a valid module with a mistyped or missing export is a `GrammarError`, not an abort.

`runtime.rs` sets the deadline for module initialization and for every parse entry point, and maps
a trap to a file-bearing `GrammarError`/parse failure. The vendor directory carries a README
recording the upstream version, commit and the patch (as `vendor/tree-sitter-yaml/README.md`
does), and `THIRD_PARTY.md`'s tree-sitter row notes the patch.

Review nrbw1j F3 (editing only a `.wasm` or `.scm` does not re-register, because kiln calls
`on_extensions` only on manifest or diagnostic changes) is not part of this card: it is filed as a
follow-up (see the task list, "T845 review F3").

## Decision 2026-10-19 (F8)

The user accepts landing T845 with the extensions record's sandbox claim narrowed: Wasm code can't
touch host memory, but a malformed grammar's static tables can still crash the editor (grammars are
user-installed). Review w0fgxx F8 (table validation) is T1005, which restores the claim. This card
records the narrowed claim in the decision record and `vendor/tree-sitter/README.md`, naming T1005.

## Implementation notes

- **Two files beyond the scope list.** `crates/fastedit/Cargo.toml` gains `fe-syntax` and
  `serde` (config.rs calls `fe_syntax::register_language` and derives the kind's schema).
  `crates/fe-syntax/src/injection.rs` takes its layer parser from the same per-thread cache:
  an injection can name a registered language, and a parser without a store can't set a wasm
  language (it would have hit the `expect`).
- **Grammar symbol from the file name.** tree-sitter finds `tree_sitter_<grammar>` in the module
  by the name `load_language` is given, which needn't be the registered name (`json_wasm` holds
  `tree_sitter_json`). It comes from the module's file name as the CLI writes it:
  `tree-sitter-<grammar>.wasm` or `<grammar>.wasm`, `-` read as `_`.
- **`earlier_patterns_win` on `GrammarSpec`** (and the `grammars` kind, default false). JSON's
  query lists `key: (_) @string.special.key` before `(string) @string`, so the fixture only
  matches the built-in with the same precedence; without the flag C1 fails.
- **Re-registering.** The same name with unchanged files (a hash of module, queries, grammar
  name and precedence) returns the language already loaded, so a kiln reload leaks nothing; with
  changed files it loads and leaks a new one, which replaces the old for new lookups. Between two
  runtime languages, the earlier registered wins a path or injection.
- **Stores.** A module is loaded in a throwaway `WasmStore` on the shared engine (the language
  doesn't reference the store); each thread's cached parser gets its own store the first time it
  sets a wasm language, and is reset (stopped parse, included ranges) when handed back.
- Files already open when an extension's grammar registers keep their language until reopened.

### Budget (measured 2026-10-19, M-series Mac, release)

`cargo test --release -p fe-syntax --test runtime_grammar -- --ignored --nocapture`, three runs,
on a 10,000-row JSON file (one-row reparse: 20 one-character inserts at row 5,000):

| | first load | full parse | one-row reparse, median (max) |
|---|---|---|---|
| `json_wasm` (fixture) | 8–21 ms (21 ms cold) | 50–53 ms | 2.4–2.8 ms (3.8–4.5 ms) |
| built-in `json` | — | 32–35 ms | 2.7–2.9 ms (3.7–5.9 ms) |

The reparse is inside the 5 ms target and as fast as the compiled grammar's; a full parse costs
about 1.5×. Release `fastedit` binary: 83,171,328 bytes before, 91,278,160 after (+8.1 MB);
stripped 74,648,696 → 81,189,784 (+6.5 MB), wasmtime with Cranelift.

### Review round 1 (codex nrbw1j)

- F4 fixed: `register_language` runs one registration at a time (lookup to publish), so the same
  files load once and the last call wins; fastedit numbers each `on_extensions` batch, runs batches
  one at a time, and a batch stops once a newer one has started.
- F5 fixed: the name clash is checked against `json`, and against `rust` only with that feature.
- F3 (editing only a `.wasm` or `.scm` doesn't re-register, as kiln calls `on_extensions` only when
  manifests or diagnostics change): not fixed here. It needs kiln to watch contributed files
  that aren't its own kinds. Until then a grammar's files take effect on restart or a manifest
  change.
- F1 (no execution deadline) and F2 (a structurally valid module with a mistyped export reaches
  `ts_assert(!error)` in tree-sitter's `wasm_store.c`, live since `cc` doesn't define `NDEBUG`):
  blocked. The extensions record §6 makes a deadline a prerequisite of this card and says it is
  blocked if tree-sitter gives no hook, and it gives none: tree-sitter creates the wasmtime store
  inside `ts_wasm_store_new`, a store's epoch deadline and fuel both start at zero (so enabling
  either on our `Engine` traps every call), and `WasmStore` exposes neither the store nor a setter.

### Rev 5: deadlines and the vendored tree-sitter

- `vendor/tree-sitter` is 0.27.0 with the patch its README describes. Each store gets an epoch
  deadline and callback, which asks the Rust binding whether the thread's deadline
  (`tree_sitter::set_wasm_deadline`, a thread-local `Instant`) has passed. The asserts after calls
  into the module become errors. Three things go beyond the decision's list, for the same
  failure class:
  - The lexer and scanner table entries are type-checked: they're called with
    `wasmtime_func_call_unchecked`, so a mistyped entry would be unsound, not just an abort.
  - `ts_wasm_store_reset_heap` stops asserting and restores the stack pointer. It runs once a
    stopped parse ends, while the deadline still holds, and a stopped call leaves the stack
    pointer lowered.
  - A failed main or keyword lexer call stops the parse, as a failed scanner call already did.
- `runtime.rs`: one engine with epoch interruption. A `wasm-epoch` ticker thread advances the
  epoch every 5 ms while any deadline is running and waits on a condvar otherwise, so an idle
  editor takes no wake-ups. `GrammarDeadlines` (`set_grammar_deadlines`) defaults to 10 s for a
  load and 5 s for a parse, matching `FULL_PARSE_CAP`. The load deadline also covers Cranelift's
  compile, which counts toward it; that's why it's generous.
- Where the deadlines apply:
  - a load, where a stop is `GrammarError::Deadline` with the file;
  - instantiating the module in a parsing thread's store (`ThreadParser::set_language`);
  - every parse, through `parse_text` (`ThreadParser::parse`), so syntax, imports recovery and
    injection layers all get it.

  A stopped parse returns `None`, so the job gives up (`Syntax::failed`), and it's logged with the
  module's path and the language. A parse on the main thread with a hostile grammar can still
  take up to the parse deadline once before the buffer drops to plain text.
- Fixtures: `tree-sitter-loop.wasm` (its scanner spins) and `tree-sitter-loopinit.wasm` (a
  constructor spins), built as `tests/fixtures/hostile-grammars.md` records. C7's modules are the
  JSON fixture with one byte changed in the test: the export's function index (mistyped), or a
  letter of its name (missing).

### Budget with the deadline patch (2026-10-19, same command, three runs)

| | first load | full parse | one-row reparse, median (max) |
|---|---|---|---|
| `json_wasm` (fixture) | 9–24 ms (24 ms cold) | 52–54 ms | 2.2–2.3 ms (2.2–3.7 ms) |
| built-in `json` | — | 32–39 ms | 2.2–2.3 ms (3.5–6.8 ms) |

The epoch checks cost nothing measurable here. The release `fastedit` binary is now 91,785,664
bytes (81,611,240 stripped). Against the 83,171,328 (74,648,696) measured before this card, at its
first base `f319d1a`, that's +8.6 MB (+7.0 MB stripped), and part of that is main's own commits
since.

### Review w0fgxx

- **F1 fixed.** `engine()` is an error when the ticker thread can't start, so nothing loads or
  parses Wasm without deadlines. A load gives `GrammarError::Module` with the file, and a parser's
  store fails `set_language`. Unit test `runtime::tests::no_ticker_is_no_engine`.
- **F4 fixed.** The files are read under the registration lock. The call that takes the lock last
  reads them last and is the one kept.
- **F6 fixed.** The lexer callbacks trap with no lexer running, and `debug_message` needs its string
  inside the module's memory. Fixtures `lexnull` and `debugmsg`. With the null check removed, the
  test process segfaults.
- **F7 fixed.** Tokens from the main lexer, keyword lexer and scanner are bounds-checked against
  `token_count` and `external_token_count`, and a bad one fails the parse. Fixtures `badlex`,
  `badkeyword` and `badscan`. With the scanner check removed, the test fails.
- **F9 fixed.** The binding's hook is now a scoped check, `with_wasm_interrupt`, not a deadline.
  `parse_text` hands it the caller's `out_of_time`, the same check tree-sitter's progress callback
  makes, together with the grammar's parse deadline. A wasm language is set on the parser, which
  instantiates it and runs its initializer, inside that parse rather than in `set_language`. So a
  budgeted parse, injection layer or imports recovery stops at its own budget. Tested by a 20 ms
  `parse_within` returning under half the deadline, and by a looping injection. The injection tests
  have no highlight budget (`no-query-budget`), so each highlight call stops at the grammar
  deadline there. In the app it stops at the frame's.
- **F8: follow-up T1005** (user decision 2026-10-19, rev 6). The grammar's static tables are
  still trusted. The narrowed sandbox claim, naming T1005, is in the decision record's §6 and
  §1 and in `vendor/tree-sitter/README.md`.
- **F3:** T1004.
