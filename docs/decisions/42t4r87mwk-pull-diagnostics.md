# Pull diagnostics against real servers

Status: **in place**, measured 2026-10-11 (T39, Mac T306) on Linux.

fe-lsp now pulls `textDocument/diagnostic` for open documents of servers that
advertise `diagnosticProvider` (design: `docs/decisions/lsp-client.md` §7,
"Pull diagnostics"; findings: §10, "Which servers version their
diagnostics").

## The run

`FE_LSP_REQUIRE_SERVERS=1 scripts/lsp-real.sh` at commit d59b580, load
average (1 min) about 9-13 from other lanes. Servers:

- rust-analyzer 1.98.1 (48a229c 2026-09-01), the pinned toolchain's rustup
  component
- pyright 1.1.414, typescript-language-server 6.0.1 and typescript-go
  (`typescript@7.0.2`, `tsc` "Version 7.0.2") from `tools/lsp`, on Node
  24.21.0

Its output (`--success-output final`), each line once, test results first:

```
PASS [   4.178s] (1/7) fe-lsp::real_server real_server_rust_analyzer_unresolved_import_unsaved
PASS [   5.099s] (2/7) fe-lsp::real_server real_server_rust_analyzer_diagnostics_follow_saves
PASS [   5.105s] (3/7) fe-lsp::real_server real_server_rust_analyzer_utf16
PASS [  30.642s] (4/7) fe-lsp::real_server real_server_typescript_go
PASS [  31.356s] (5/7) fe-lsp::real_server real_server_pyright
PASS [  32.228s] (6/7) fe-lsp::real_server real_server_typescript_language_server
PASS [  35.284s] (7/7) fe-lsp::real_server real_server_rust_analyzer_utf8
Summary [  35.285s] 7 tests run: 7 passed, 101 skipped
rust-analyzer: Ready 3.85 s after launch
rust-analyzer: the unresolved import reached the sink 25.26ms after the edit
rust-analyzer:   Some(Error) Some("rust-analyzer") Some("E0432"): unresolved import
rust-analyzer: Ready 4.56 s after launch
rust-analyzer: after the save: Some("rustc") Some("E0308"): mismatched types
rust-analyzer: after the save: Some("rustc") Some("E0308"): expected `&'static str` because of return type
rust-analyzer: after the save: Some("rust-analyzer") Some("E0308"): expected &'static str, found i32
rust-analyzer: Ready 4.88 s after launch
typescript-go: Ready 0.61 s after launch
typescript-go: diagnosticProvider advertised
typescript-go: pulled for greet.ts: "full" report, 0 items
typescript-go: pulled for greet.ts: "full" report, 1 items
typescript-go: the unsaved error reached the sink 7.85ms after the edit
typescript-go:   Some(Error) Some("ts") Some("2322"): Type 'string' is not assignable to type 'number'.
pyright: Ready 0.85 s after launch
pyright: diagnosticProvider not advertised
pyright: pushed for greet.py: version Some(1), 1 items
pyright: the unsaved error reached the sink 283.93ms after the edit
pyright:   Some(Error) Some("Pyright") Some("reportAssignmentType"): Type "Literal['x']" is not assignable to declared type "int"
typescript-language-server: Ready 2.11 s after launch
typescript-language-server: diagnosticProvider not advertised
typescript-language-server: pushed for greet.ts: version None, 1 items
typescript-language-server: the unsaved error never reached the sink
rust-analyzer: Ready 5.02 s after launch
rust-analyzer: diagnosticProvider advertised
rust-analyzer: pulled for src/greet.rs: error -32802 "server cancelled the request"
rust-analyzer: pulled for src/greet.rs: "full" report, 1 items
rust-analyzer: the unsaved error reached the sink 3.72ms after the edit
rust-analyzer:   Some(Error) Some("rust-analyzer") Some("E0308"): expected i32, found &'static str
```

## What it shows

- **rust-analyzer** answers pulls, and its own diagnostics reach an unsaved
  buffer: its E0308 for `pub fn broken() -> i32 { "x" }` reached the sink as
  `LspEvent::Diagnostics` at the edited version 4 ms after the edit. The
  first pull after the edit was answered `ServerCancelled` (-32802) and
  dropped; the pulls after it held the error.
- **An unresolved import is an experimental diagnostic** in rust-analyzer
  1.98.1, off unless `diagnostics.experimental.enable` is set. Under the
  built-in spec (no such setting), every pull reply for the unsaved file held
  no item for `use crate::missing;`: a scratch run (not committed) that kept
  editing tried `use crate::missing;`, `use std::missing2;`,
  `use missing_crate::x;` and `use crate::greet::missing3;`, while
  `pub fn broken() -> i32 { "x" }` in the same edit came back as "expected
  i32, found &'static str". So under the built-in spec the import shows
  only after a save, as `cargo check`'s E0432, and
  `real_server_rust_analyzer_utf8` checks the type mismatch unsaved and
  asserts the import isn't among rust-analyzer's own diagnostics.
  `real_server_rust_analyzer_unresolved_import_unsaved` starts it with
  `initializationOptions` `{ "diagnostics": { "experimental": { "enable":
  true } } }`, as a user's `[lsp.servers.rust-analyzer]` would: the
  unresolved import (`source` `rust-analyzer`, E0432) came by pull and
  reached the sink as `LspEvent::Diagnostics` at the edited version 25 ms
  after the edit, the file never saved.
- **typescript-go** answers pulls: TS2322 reached the sink 8 ms after the
  unsaved edit. The first run (before the fix) failed: typescript-go
  answered every pull `InvalidParams` ("null value is not allowed for field
  \"previousResultId\""), because lsp-types 0.95.1 serializes the unset
  field as `null`. fe-lsp now sends the params as JSON with unset fields left
  out, and `pulls_after_open_change_and_save` checks the keys.
- **pyright** doesn't advertise `diagnosticProvider` even with the client
  offering pull, so it isn't pulled; its versioned pushes are as before.
- **typescript-language-server** doesn't advertise it either; its
  unversioned pushes are still dropped (T305).
- **Duplicates:** after saving a type mismatch, rust-analyzer's event holds
  `rustc`'s pushed E0308 (two items: the error and its "because of return
  type" note) and its own pulled E0308 for the same expression. fe-lsp shows
  both; no other server does both.

`versions_diagnostics` (pushes holding the unsaved error, and whether they
carry a version) and the new `pulls_diagnostics` (pull replies holding it,
and `diagnosticProvider` advertised) now match the servers:

| Server | Pushed | Pulled | Shown unsaved |
|---|---|---|---|
| rust-analyzer | none | yes | yes (its E0308; E0432 with experimental diagnostics on) |
| pyright | versioned | no provider | yes |
| typescript-language-server | unversioned | no provider | no |
| typescript-go | none | yes | yes |
