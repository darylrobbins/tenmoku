---
task_uid: "01m7j3ft93h4f3rrq9kh2waf5k"
task: "T1905"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "sh scripts/test-heap-by-crate.sh exits 0: its inline malloc_history -allBySize sample folds to the expected rows (a Vec growth charged to fe_syntax, a hashbrown insert charged to fe_nav, a ts_ C stack to tree-sitter (C), a CoreText stack to CoreText, one legacy-mangled and one v0-mangled Rust name), the total equals the input's, --depth 2 splits a crate by module, an unknown frame shape lands in unattributed (not dropped), and empty or unreadable input exits non-zero with a message"
    required: true
    manual: false
  - id: "C2"
    text: "python3 scripts/heap-by-crate.py --help gives the whole recipe: build with symbols (cargo build --profile bench -p fastedit, since [profile.release] strips symbols; CARGO_PROFILE_BENCH_DEBUG=line-tables-only for inlined frames), launch with MallocStackLogging, run malloc_history -allBySize or save a leaks --outputGraph memgraph, and add footprint categories with --footprint"
    required: true
    manual: false
  - id: "C3"
    text: "PERF.md has a ## Heap by crate section after ## Fixed memory, linked from ## Measuring, with the recipe, what each tool sees (malloc tools: heap only; footprint: GPU, IOSurface, CG Image, __TEXT), the inlining caveat, and one real by-crate table from a launched dev build (empty window and the 10k-line Rust file) with date and build. Known limits, deferred to follow-up card T1919: the xctrace Allocations toc question is stated as unverified (recording failed to attach here), and the tables are open-and-settle, not hand-scrolled."
    required: true
    manual: false
---

# Heap by crate on macOS without code changes: scripts/heap-by-crate.py folds live malloc stacks (MallocStackLogging + malloc_history, xctrace if it exports) by Rust crate, plus a PERF.md Heap by crate section

# Live heap by crate on macOS: a script and a PERF.md section

## Why

The user asked: "Is there a way to track more finely where our memory is being
spent at least by module?" and said "Yes please" to this card.

`footprint -p` on the user's running Tenmoku (335 MB) gave only categories:
IOSurface 94 MB, Malloc Small 82 MB, Malloc Large 52 MB, owned unmapped
graphics 42 MB, CG Image 32 MB (one region), IOAccelerator 10 MB, __TEXT 31 MB
clean. The ~134 MB of malloc is what nobody can attribute today. This card
gives a no-code-change way to split live heap by Rust crate (and by C library
/ system framework), and documents it. The built-in report is a separate card
(Show Memory Usage); the CG Image region is another.

## What exists

- `Cargo.toml` `[profile.release]`: `strip = "symbols"` (no symbol table, so
  stacks are unnamed). `[profile.bench]` inherits release with
  `strip = "none"`: symbols kept, no debuginfo. There is **no** `profiling`
  profile and this card adds none: build with
  `cargo build --profile bench -p fastedit` (binary `target/bench/tenmoku`).
  With LTO and `codegen-units = 1` many frames are inlined; for inlined frames
  in stacks, `CARGO_PROFILE_BENCH_DEBUG=line-tables-only cargo build --profile bench -p fastedit`
  (macOS packs a dSYM next to it). The script's `--help` and PERF.md say both.
  A bench build is a cold build: ask the lead first (shared-disk build gating).
- PERF.md has `## Measuring`, `### Memory (physical footprint)`,
  `## Fixed memory` (footprint -v categories for the empty file) and two tree
  memory sections. Add the new section right after `## Fixed memory`, named
  `## Heap by crate`, and link it from `## Measuring`.
- Xcode 27's `xctrace` (checked on this Mac): `xctrace record --template
  Allocations (--attach <pid|name> | --launch -- <cmd>) --output x.trace
  [--time-limit 10s]`; `xctrace export --input x.trace --toc` lists the
  exportable tables, and `--xpath '/trace-toc/run[@number="1"]/data/table[@schema="…"]'`
  exports one. Caveat to check first: the Allocations instrument is a legacy
  instrument and may expose no allocations table in `--toc` (only the
  process / metadata tables). Attaching also misses allocations made before
  the attach, so `--launch` is the useful mode.
- `malloc_history` (no Xcode needed) works on a process launched with
  `MallocStackLogging=lite` (or `1`): `malloc_history <pid> -allBySize` prints
  every live allocation stack with its count and bytes; `leaks <pid>
  --outputGraph=x.memgraph` saves the heap with stacks to analyse offline
  (`malloc_history x.memgraph -allBySize`). `-callTree -invert` is the
  alternative output. These run against a live process the user started;
  they read it, they do not stop it.

## What to build

1. `scripts/heap-by-crate.py` (python3, stdlib only, like
   `scripts/check_kiln_boundary.py`):
   - input: a `malloc_history -allBySize` text file (or `-` for stdin), or
     with `--pid <pid>` it runs `malloc_history <pid> -allBySize` itself; if
     the worker finds `xctrace export` does give an allocations table, also
     accept that XML (`--xctrace x.trace`), else document why not;
   - attribution: walk each stack from the allocation up and charge it to the
     first frame that is not allocator / std plumbing (`libsystem_malloc`,
     `alloc::`, `core::`, `std::`, `hashbrown::`, `smallvec::`, `<alloc::…`
     trait impls; a `--skip <prefix>` flag adds more). Demangle Rust legacy
     and v0 names (strip `_ZN…E`/`_R` with a small demangler or `rustfilt` /
     `c++filt` when on PATH; a missing tool degrades, not fails). Crate = the
     first path segment (`fe_syntax`, `gpui`, `ropey`, `wasmtime`,
     `tree_sitter`, …), with `<impl X for Y>` charged to Y's crate. C frames:
     `ts_` → `tree-sitter (C)`, `wasmtime_`/`wasm_` → `wasmtime`; frames in
     another image (CoreText, AppKit, Metal, QuartzCore, libobjc) are charged
     to that image's name;
   - output: a table sorted by bytes: crate, live bytes, %, allocations;
     `--depth N` splits the top crates by their next path segment (module);
     `--top N`; a total line that matches the input's total;
   - `--footprint <pid>` appends `footprint -p <pid>` category summary (or
     reads a saved `footprint` output file) so the heap rows sit beside the
     GPU / image categories malloc tooling can't see;
   - `--help` gives the whole recipe: build with symbols, launch with
     `MallocStackLogging=lite target/bench/tenmoku <file>` (or `open` the
     bundle with the env set via `launchctl setenv` — say which works), wait,
     run the script.
2. `scripts/test-heap-by-crate.sh` (sh, in the style of
   `scripts/test-bench-compare.sh`): writes a small synthetic
   `-allBySize` sample inline (a handful of stacks: one Rust Vec growth in
   `fe_syntax`, one through `hashbrown` from `fe_nav`, a `ts_subtree_new_leaf`
   C stack, a CoreText one, a mangled legacy and a v0 name) and checks the
   folded table: the right crate per stack, totals equal the input's, the
   `--depth 2` split, and that an unknown frame shape is reported as
   `unattributed`, not dropped. Also an unreadable / empty input exits non-zero
   with a message. No process is attached or recorded by the test.
3. PERF.md `## Heap by crate`: the recipe, what each source sees (malloc tools:
   heap only; footprint: GPU, IOSurface, CG Image, __TEXT), the inlining
   caveat, and one real table from a dev run (an empty window, and the 10k-line
   Rust file, after `--perf`-like scrolling by hand) with the date and build.

## Out of scope

Code changes in crates, a new Cargo profile, an accounting allocator, CI
wiring for the script test (none of `scripts/test-*.sh` run in CI).
