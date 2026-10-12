---
task_uid: "01m4rgwh1n8gbbm54b3ph0razf"
task: "T41"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo bench -p fe-syntax --bench tree_ratio prints a source MB, tree MB and ratio line for every listed language other than rust, json and markdown, on macOS and on Linux with glibc (mallinfo2), and 'not measured on this platform' elsewhere"
    required: true
    manual: false
  - id: "C2"
    text: "parse_limit.rs RATIOS has an entry for every name language_names() yields, each the measured ratio rounded up and at least 20; rust, json and markdown stay 40, 30 and 85"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test parse_limit::limits_follow_the_tree_budget passes with its ratio() mirror updated to the same values; rust_limit, json_limit and markdown_limit pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "PERF.md gains an appended 'Tree memory by language' subsection with each language's measured ratio and parse limit, the machine, platform and date; no existing PERF.md line is edited"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-syntax -p fe-nav, cargo clippy -p fe-syntax --all-targets -- -D warnings and scripts/check-syntax-features.sh pass; Cargo.lock is unchanged"
    required: true
    manual: false
---

# Measure every grammar's syntax-tree memory per source byte and set each language's parse limit from it, on macOS and Linux (Mac T498)

Filed on helium from Mac T498 (~/coordination/helium-cards/T498.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-u.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Helium notes (from the Mac lead, note u)

- On Linux, measure with glibc's mallinfo2. Criterion C5 says Cargo.lock stays unchanged, so declare it yourself (extern "C" fn mallinfo2 with a #[repr(C)] struct, under cfg(all(target_os = "linux", target_env = "gnu"))) unless fe-syntax already depends on libc. Adding a new dependency to fe-syntax would change Cargo.lock.
- Label the measured numbers "helium" and give the machine line, as PERF.md's "Buffer" section (T32, Mac T404) does: machine, commit, date, load average. No macOS numbers are taken on helium, so say so, and leave the macOS half of C1 to the Mac.
- PERF.md is added to this task's scope (criterion C4 appends a PERF.md subsection). It waits for T38 (Mac T413), which also holds PERF.md.

## Problem

The plan's large-file target is "no more than 1.5× file size in memory", and PERF.md, "Tree memory", gives each
file's syntax tree a 300 MB budget (`TREE_BUDGET_BYTES`). A language's parse limit is `300 MiB / ratio`, with the
ratio hard-coded in `crates/fe-syntax/src/parse_limit.rs` (`RATIOS`, :32). Only Rust (40×), JSON (30×) and
Markdown (85×) were measured; the other 24 languages take `DEFAULT_RATIO` (40), which PERF.md's table marks
"40× (not measured; the Rust ratio)". A grammar whose trees cost more than 40× its source (Markdown is 85×;
JSON's worst case 96×) can exceed the budget below its limit, and one that costs less is cut off early and loses
its full tree for nothing. Tree memory is measured only on macOS (`benches/support/mod.rs` `allocated()` is
`0` elsewhere), so Linux hosts can't measure it.

## Change

1. `crates/fe-syntax/benches/support/mod.rs`: on Linux with glibc, `allocated()` reads `mallinfo2()`'s `uordblks`
   (declared `extern "C"` there, as the macOS path declares `malloc_zone_statistics`; no new dependency). Other
   targets keep `0`.
2. New bench `crates/fe-syntax/benches/tree_ratio.rs` (`[[bench]] name = "tree_ratio"`, `harness = false`, in
   `crates/fe-syntax/Cargo.toml`): for every language `language_names()` yields except Rust, JSON and Markdown,
   build about 4 MB of source by repeating that language's `crates/fe-syntax/tests/golden/<lang>.<ext>` sample
   whole, parse it once (median of 3 parses, each in a fresh `Syntax`), and print one line per language through
   `support::tree_memory`: name, source MB, tree MB, ratio. It asserts nothing and prints "not measured on this
   platform" where `allocated()` is `0`.
3. `crates/fe-syntax/src/parse_limit.rs`: `RATIOS` gets an entry for every language in the table, each the
   measured ratio rounded up to a whole number, never below 20 (a floor so a tiny sample can't lift a limit
   past 15 MiB). Rust, JSON and Markdown keep 40, 30 and 85. `DEFAULT_RATIO` stays for names not in the table.
   The module doc says where the new ratios come from.
4. `crates/fe-syntax/tests/parse_limit.rs`: its `ratio()` mirror gets the same values.
5. PERF.md (`shared_append_only` in regatta.toml, so appended, not edited): a new subsection after "Tree
   memory", "Tree memory by language (`cargo bench -p fe-syntax --bench tree_ratio`)", with the measured row
   and resulting parse limit for each language, the machine, platform and date, and one line saying it
   supersedes the "every other language" row above.

## Implementation notes

- `allocated()` on Linux glibc is `uordblks + hblkhd`, not `uordblks` alone: glibc serves large allocations
  (over the mmap threshold) from their own mappings, which `uordblks` leaves out and `hblkhd` counts.
  macOS's `size_in_use` counts both.
- Four samples don't repeat cleanly, and a corpus with parse errors measures error recovery (XML repeated
  whole measured 72×, 46× clean; Lua 0.7×, its first shebang swallowing the rest). `tree_ratio`'s `corpus`
  keeps Lua's shebang, Kotlin's package and imports and XML's prolog in the first copy only, wraps XML's
  copies in one root element, and gives PHP 50 copies (0.1 MB): tree-sitter-php stops recognising heredocs
  after about 200 in one file. Other languages at 50 copies come within 0.2× of their 4 MB ratios. The bench
  skips, and says so, any parse that has errors or no tree.
- Only helium (Linux) numbers were taken; the macOS half of C1 is left to the Mac.
