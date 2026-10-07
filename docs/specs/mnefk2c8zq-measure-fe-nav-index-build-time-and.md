---
task_uid: "01m48tyd2e4hanxamnefk2c8zq"
task: "T108"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-nav/benches/nav.rs builds in the gate and measures build (default threads and threads: 1), go-to-def with a complete index (rare, 50-file and same-file names), go-to-def through the ripgrep fallback (rare and common), references time to first batch and to completion (rare and common), and workspace_symbols for 1-, 3- and 6-char queries, over a generated corpus of 10,000 Rust-like files (~40 MB)"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md gains a Navigation (cargo bench -p fe-nav --bench nav) section with one run of every measurement, the index stats (files, symbols, distinct names, approx_bytes) after the build, and the plan 100 ms go-to-def target beside the go-to-def rows (recorded, not asserted); any go-to-def case over 100 ms is named with its cause and proposed follow-up"
    required: true
    manual: false
  - id: "C3"
    text: "Cargo.lock changes only in fe-nav dependency list (criterion is already in the lock); no change under crates/fe-nav/src; scripts/check-no-gpui.sh prints ok: fe-nav"
    required: true
    manual: false
---

# Measure fe-nav: index build time and memory, and go-to-def, references and symbol latency

# Measure fe-nav: index build time and memory, and go-to-def, references and symbol latency

Milestone M4 "Navigate". The plan's performance targets include "Search-based navigation: go-to-def
under 100 ms before the LSP is ready". T103–T106 build the index, go-to-def, references and
workspace symbols without measuring them. This card adds one bench and records a run in `PERF.md`,
as T62 and T65 did for the finder and project search. No behaviour changes.

## Change

- `crates/fe-nav/Cargo.toml`: dev-dependency `criterion` (workspace; already in the lock) and
  `[[bench]] name = "nav", harness = false`.
- `crates/fe-nav/benches/nav.rs`. The corpus is generated once into a temp dir by the bench itself:
  10,000 Rust-like files (about 40 MB) in nested directories, deterministic, with structs, fields,
  enums, impls, fns and calls between files, a few hundred names shared by many files, and a
  `.gitignore`d directory. It measures:
  - **build**: `fe_nav::build` with default threads and with `threads: 1`; files per second
  - **memory**: `SymbolIndex::stats()` symbols, distinct names and `approx_bytes` after the build
    (printed, not timed), and the process's peak RSS growth if cheap to read
  - **go-to-def, index complete**: a rare name, a name defined in 50 files, a same-file hit
  - **go-to-def, index empty and incomplete**: the ripgrep fallback for a rare and a common name
  - **references**: time to first batch and to completion for a rare and a common name
  - **workspace symbols**: `workspace_symbols` for 1-, 3- and 6-char queries
- `PERF.md`: a new "Navigation (`cargo bench -p fe-nav --bench nav`)" section with one run of these
  numbers next to the plan's 100 ms go-to-def target (recorded, not asserted). If any go-to-def case
  is over 100 ms, the section names the cause and the follow-up to file, and the PR description says
  so.

## Out of scope

- Optimisations. File them as follow-ups from the numbers.
- A 100k-file run in the gate. It may be run once by hand and recorded, if it fits a lane session.

## Decided (lead defaults; the user may revisit)

- Same corpus size as T65's search bench (10,000 files, about 40 MB), so the two are comparable.
- Recorded, not asserted, like the other benches.
