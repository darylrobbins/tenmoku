---
task_uid: "01m7kpwy0q9gsgyheft22be002"
task: "T372"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench tests tests::memory_report::show_memory_usage_opens_a_read_only_report_tab, a_buffer_in_two_editors_is_counted_once, a_parsed_file_reports_a_syntax_tree_row, the_report_has_process_totals_and_an_unattributed_row and a_closed_tab_drops_its_rows pass"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test tests::memory::history_total_is_the_sum_over_origins and fe-syntax test node_count_estimate_is_within_2x_of_malloc_growth (crates/fe-syntax/tests/tree_memory_estimate.rs, a small Rust sample) pass"
    required: true
    manual: false
  - id: "C3"
    text: "Command memory.show (Show Memory Usage) is in the palette: typed_command_ids_tests and command_catalog_tests list it, its title is in l10n/en-US/commands.ftl, and the fe-workbench l10n lint passes with every new report label through tr!"
    required: true
    manual: false
  - id: "C4"
    text: "Process totals: macOS phys_footprint and ledger_tag_graphics_footprint from task_info(TASK_VM_INFO) plus malloc_zone_statistics size_in_use; Linux VmRSS plus glibc mallinfo2 uordblks; Windows working set; rows not measurable on a platform say not measured; the crate builds for Linux and Windows targets"
    required: true
    manual: false
  - id: "C5"
    text: "tenmoku --perf prints memory by module: rows after its memory after scrolling line (that line unchanged), and PERF.md has a ### Memory by module section with one real run's table (10k-line Rust file) and the bytes-per-node constant from tree_ratio"
    required: true
    manual: false
  - id: "C6"
    text: "Language servers: the report lists each running server process and its children with resident memory and footprint read by pid (macOS task_info / Linux /proc), rust-analyzer's rust-analyzer/memoryUsage text under its row, and a Restart action per server; tested with fe-lsp's fake server (a fake process pid and a canned memoryUsage reply)"
    required: true
    manual: false
---

# Show Memory Usage: an on-demand report of bytes held by module (buffer text, undo, syntax trees, wasm stores, symbol index, project tree, file views, terminals, LSP) with process totals and an unattributed remainder, in a read-only tab and tenmoku --perf (Mac T1906)

# Show Memory Usage: a built-in memory report by module

## Why

The user asked: "Is there a way to track more finely where our memory is being
spent at least by module?" and said "Yes please" to this card.

`footprint -p` on the user's running Tenmoku (335 MB) gave: IOSurface 94 MB,
Malloc Small 82 MB, Malloc Large 52 MB, owned unmapped graphics 42 MB,
CG Image 32 MB, IOAccelerator 10 MB, __TEXT 31 MB. Nothing in the app says
which subsystem holds the ~134 MB of malloc. This card adds an on-demand report
where each subsystem states the bytes it holds, beside process totals and an
"unattributed" remainder. The external, no-code script is a separate card
(heap by crate); the CG Image region is another.

## Design decisions (made; respec if they don't hold)

- **No shared trait, no accounting allocator.** Each crate gets plain
  inherent methods that return what it holds (`bytes`, `count`, and whether
  the bytes are an estimate); the collector in fe-workbench knows the owners
  (they are GPUI entities in panes, docks and the project) and builds the
  rows. A trait in a base crate (fe-text is the only crate under fe-buffer,
  fe-fileview, fe-nav and fe-lsp) would add coupling and still need the
  collector to enumerate owners. Everything is computed when asked, never
  kept up to date.
- **Types**: `fe_workbench::memory_report::{MemoryReport, MemoryRow,
  ProcessMemory}` in a new `crates/fe-workbench/src/memory_report.rs`.
  `MemoryRow { module: &'static str (stable id), label (tr!), bytes: u64,
  count: Option<u64>, estimate: bool }`. `MemoryReport::collect(&Workbench, cx)`
  and `MemoryReport::to_text()` (the table both outputs print).
- **Per-editor numbers** come from fe-gpui: a new crate-level module
  `crates/fe-gpui/src/memory.rs` (declared in `crates/fe-gpui/src/lib.rs`)
  with `impl EditorView { pub fn memory(&self, cx) -> EditorMemory }`;
  `EditorView.syntax` is `pub(crate)` so no change to `view.rs` is needed
  (T1897 is active in `view.rs`).
- **Buffers shared by several editors are counted once** (dedupe by the
  model / buffer entity id); rope nodes shared with snapshots are not added
  again.

## What each subsystem can report (checked on main a0c2e9057)

| Row | Source | Exact or estimate |
|---|---|---|
| Buffer text | fe-text `Text` (ropey): no heap API. Add `Text::heap_bytes_estimate()` = len × the ratio `crates/fe-buffer/examples/load_memory.rs` measures (budget 1.5×; print the measured ratio in the doc comment) | estimate |
| Undo history | fe-buffer `Buffer::history_bytes(&Origin)` and `history_limit()` exist (`buffer.rs` ~L1105–1119); add a total over origins; `examples/history_memory.rs` measures the real overhead | edit-text bytes, exact; overhead estimate |
| Syntax trees | tree-sitter has no size API. `tree.root_node().descendant_count()` is O(1) in the vendored C (`ts_subtree_visible_descendant_count` + 1, node.c L529). Bytes = nodes × a bytes-per-visible-node constant calibrated in `crates/fe-syntax/benches/tree_ratio.rs` (which already measures malloc growth per parse; print bytes/node there and record it in PERF.md "Tree memory by language"). Cross-check: source bytes × `parse_limit.rs` ratio. Count = nodes | estimate |
| WebAssembly grammars | stores are thread-local (`runtime.rs` `PARSER` ~L610, created at ~L668), unreachable from the main thread: keep a process-wide `AtomicUsize` of live stores and loaded languages in fe-syntax. Linear memory bytes need `wasmtime_memory_data_size` on `TSWasmStore.memory` (vendor/tree-sitter/src/wasm_store.c L129/L346): out of this card's scope, so report count only, bytes "not measured", and note it (T1892 shows a store reaching its 128 MiB) | count only |
| Symbol index | fe-nav `SymbolIndex::stats().approx_bytes` (index.rs L212, existing) | estimate |
| Project tree / finder | fe-project `tree.rs` `stats()` and `finder.rs` `stats()` exist: use their counts, add bytes if cheap | estimate |
| Large-file views | fe-fileview `cached_bytes()` (lib.rs L142, existing) | exact |
| Terminals | scrollback: `scrolling_history` is up to 100,000 lines (fe-terminal `pty_loop.rs` L796); bytes = `grid.history_size()` (+ screen rows) × columns × `size_of::<Cell>()` | estimate |
| Language servers | in-process: open documents, held requests (bounded by `QUEUED_MAX` = 32 in `fe-lsp/src/server.rs`), diagnostics count. The servers are child processes: list each with its own footprint if fe-lsp exposes the pid cheaply, under a "child processes (not in the total)" heading; otherwise count only | count |
| Minimap tiles, display maps | from fe-gpui `EditorMemory`: tile count × tile bytes (element/minimap.rs), wrap/fold map sizes if fe-display exposes lengths | estimate |
| GPU | GPUI's atlases are private in gpui-pre (not vendored): no per-atlas row. Instead the process row below gives the graphics ledger | n/a |

Rows the worker finds are not reachable without scope outside this card
stay "not measured" in the report rather than widening scope.

## Process totals and the remainder

- macOS: `task_info(TASK_VM_INFO)`: `phys_footprint` and
  `ledger_tag_graphics_footprint` (GPU, IOSurface, IOAccelerator). `mach2`
  0.6 (already in Cargo.lock, transitively) has `task_vm_info`; add it as a
  macOS-only dependency of fe-workbench, or declare the struct by hand. Malloc
  in use: `malloc_zone_statistics(NULL, &stats).size_in_use` (libc).
  `crates/fastedit/src/perf.rs` reads `proc_pid_rusage` `ri_phys_footprint`
  today (L615); keep its line unchanged.
- Linux: `VmRSS` from `/proc/self/status` (as perf.rs L647) and, on glibc,
  `mallinfo2().uordblks` for malloc in use. Windows: working set
  (perf.rs L672's call); malloc in use "not measured".
- **Unattributed** = malloc in use − Σ attributed heap rows, shown even when
  negative (estimates can overshoot) with a note.

## Surfaces

1. Command `memory.show` "Show Memory Usage" (palette; `#[command]` in
   `crates/fe-workbench/src/workbench_commands.rs`, title in
   `l10n/en-US/commands.ftl`, ids in `typed_command_ids_tests.rs` and
   `command_catalog_tests.rs`): opens a read-only editor tab titled "Memory
   Usage" with the table (the `debug_show_log` pattern in
   `debug_session.rs` L958: `EditorModel::new(Buffer::from_file_contents(..))`,
   `set_read_only(true)`, `set_tab_title`). Columns: module, bytes (MB),
   count, `~` for estimates; process totals first, unattributed last. No
   live refresh; running the command again opens a fresh snapshot.
2. `tenmoku --perf`: after the existing `memory after scrolling` line, print
   the same table (`MemoryReport::collect` on the perf run's workbench) as
   `memory by module:` lines. Document both in PERF.md (new
   `### Memory by module` under `## Fixed memory`) with one real run.

## Tests (new files)

- `crates/fe-workbench/src/tests/memory_report.rs` (declared in
  `src/tests/mod.rs`): headless workbench tests.
- `crates/fe-buffer/src/tests/memory.rs` (declared in `src/tests/mod.rs`):
  history total.
- `crates/fe-syntax/tests/tree_memory_estimate.rs`: the node-count estimate
  for a parsed Rust sample is within 2× of malloc growth across that parse
  (measure as `tree_ratio.rs` does; keep the sample small).

## Notes

- l10n: row labels and the title go through `tr!` (R3 lint flags new literal
  strings; module ids are `verbatim!`-style consts).
- Platform: the report is a text tab, no new UI chrome, so no screenshot
  goldens; tests are headless and run on Linux too. Process totals are cfg'd
  per OS in fe-workbench (not kiln, so the kiln OS cfg boundary doesn't apply).


## Language servers (added by the lead, 2026-11-16)
The user then asked: "Anything we can do about the language server memory usage? Fastedit is a big Rust project but this is pretty expensive" (rust-analyzer was at 8.3 GB resident on this repo). The report lists each running language server process (name, pid, resident and footprint, from the OS by pid) and its children (for example rust-analyzer-proc-macro-srv), and for rust-analyzer adds its own breakdown from the rust-analyzer/memoryUsage request (shown as the text it returns). Each server row has a Restart action. T1904 sets rust-analyzer's memory-conscious defaults.

## Implementation notes

- **Scope widened by respec** (before any code): fe-lsp `launcher.rs`
  (`ServerProcess::pid`, default `None`; `LocalProcess` gives its child's id
  until it's seen to exit), `manager.rs` (`LspManager::running_servers`),
  `tests/fake.rs` (`FakeServer::set_pid`); fe-workbench `lsp.rs`
  (`restart_server`, split out of `run_server_op` for the Restart lens),
  `lsp_consent.rs` and `tests/lsp.rs` (their process wrappers pass `pid`
  on), `nav.rs` and `quick_open.rs` (the symbol index's and quick open's
  indexes are private to those modules). The pid lives in the server
  thread's shared `Session`, cleared when a session ends.
- **`MemoryRow.bytes` is `Option<i64>`**, not `u64`: `None` prints "not
  measured", and unattributed can be negative.
- **Process totals.** macOS: `task_info(TASK_VM_INFO)` with the struct
  declared by hand (packed to 4, through revision 5) so no new dependency;
  the count the kernel returns is checked before `phys_footprint` and
  `ledger_tag_graphics_footprint` are read. Linux malloc in use is glibc's
  `uordblks + hblkhd` (`hblkhd` holds the large chunks malloc maps on its
  own, as `benches/support` counts them); musl is "not measured". Windows:
  the working set (`Win32_System_ProcessStatus` feature). The macOS and
  Windows `os` modules were type-checked and clippy-clean for
  `aarch64-apple-darwin` and `x86_64-pc-windows-msvc` in a scratch crate;
  only Linux ran.
- **Language servers.** Their memory by pid: macOS `proc_pid_rusage`
  (`ri_resident_size`, `ri_phys_footprint`, the same ledger as
  `task_info`'s footprint, but it needs no `task_for_pid` rights over the
  other process); children from `proc_listpids(PROC_PPID_ONLY)`. Linux
  `/proc/<pid>/status` (`VmRSS`; footprint is `RssAnon + VmSwap`) and each
  `/proc/*/stat`'s parent. Windows a Toolhelp snapshot and
  `GetProcessMemoryInfo` (private bytes as footprint). Descendants are
  listed three deep. `rust-analyzer/memoryUsage` goes to each server named
  rust-analyzer (`MemoryReport::collect_with_breakdowns`, shared by the tab
  and `--perf`, which waits for it before soft wrap); the report is
  complete once every answer is in, or after 10 s with "no answer within 10
  seconds". The Restart action is a code lens over the server's line
  running `lsp.restart` with `server`, `instance` and `generation`
  arguments (`RunningServer::instance`, the `ServerId`'s number, and the
  workbench's LSP manager generation, since a new root's manager numbers
  its servers from 0 again: no root path in JSON, which a non-UTF-8 root
  wouldn't survive); an instance that has restarted since the report, or
  one of an earlier root's manager, toasts that it's gone. `lsp.restart` without
  them still restarts the active editor's server.
- **Terminals on the alternate screen** say their scrollback is not
  measured: alacritty keeps the primary grid but doesn't expose it, so the
  row is "not measured" while any terminal shows a full-screen program.
- **Review round 1** (z8gyb0): F1 the shared collector above; F2 the
  alternate screen; F3 the instance id; F4 the grammar count now rises
  only once `register_language` keeps the language (an inline
  `runtime.rs` test, as `tests/runtime_grammar.rs` is out of scope).
- **Review round 2** (2cm3q3): F5 the manager generation in the Restart
  lens, with a regression keeping a report across a root change; F6
  `--perf` drops the frames painted while the report came in before soft
  wrap's first step (`discard_frames`). That has no automated test: perf.rs
  runs only as `tenmoku --perf` in a real window, and its tests are pure
  functions; the change is one call before `wrap_step`.
- **Not measured, as the spec allows:** WebAssembly stores' linear memory
  (count of live stores and loaded grammars only, from process-wide
  atomics in `runtime.rs`); minimap tiles (the tile cache is private to
  `element/minimap.rs`); display maps (not reported); the in-process LSP
  client (documents counted, bytes not). Not counted: injected languages'
  layers (`injection.rs`), the editors outside tabs (commit box, diff views'
  sides), and the symbol index's own file set (fe-nav doesn't expose it).
- **Measured constants.** Rope heap 1.12× its text (`load_memory`, 100 MB,
  Linux release). Tree memory 110 B per visible node: the median of
  `tree_ratio`'s 26 languages (Rust 117 B); PERF.md "Bytes per visible
  node". `tree_ratio` also stopped panicking on a language with no golden
  sample (gitcommit) and skips it instead.
- **Carried finding T335 F2 (Reveal in Finder palette title) not fixed.**
  The palette resolves `command-project_panel-reveal_in_os` through
  `kiln_command::command_label`, which formats it with kiln's label
  arguments only; a per-platform title needs an `$os` label argument set
  from fe-workbench's init and a Fluent selector, which the panel's
  registry (`project_panel.rs`, `kiln_l10n::message` with no arguments)
  would then format with a missing-variable error. That's kiln-command and
  project-panel work beyond this card.
