# typescript-language-server's diagnostics through tsserver

Status: **in place**, measured 2026-10-12 (T73, Mac T305) on Linux.

typescript-language-server never versions its `publishDiagnostics`, so fe-lsp
drops them (`docs/decisions/lsp-client.md` §4.1). Its diagnostics are now
pulled through its `typescript.tsserverRequest` command instead
(`PullDiagnostics::TsserverRequest`, `crates/fe-lsp/src/tsserver.rs`; design in
§4.1, findings in §10, "Which servers version their diagnostics").

## The server source

typescript-language-server 6.0.1 (`tools/lsp/node_modules/typescript-language-server/lib/cli.mjs`),
running tsserver from `typescript` 6.0.3, checked 2026-10-12:

- **No guarantee for its pushes.** `class FileDiagnostics` keeps
  `diagnosticsPerKind`, one list per tsserver event kind. `update(kind, …)`
  replaces that kind's list and calls `firePublishDiagnostics`, a 50 ms
  `pDebounce` of `publishDiagnostics`, which publishes every kind merged
  (`getDiagnostics`), with no version. Nothing clears the lists on a change,
  so one publication can join kinds computed on different texts.
- **No `diagnosticProvider`**, so `textDocument/diagnostic` doesn't apply.
- **The command.** `class TSServerRequestCommand` (`static ID='typescript.tsserverRequest'`)
  is registered with the `CommandManager` and listed in
  `executeCommandProvider.commands`. `executeCommand` calls
  `execute(...params.arguments)`, so the arguments are `[command, args]`.
  When `args.file` names an open document by URI, it's replaced by the
  document's path (`toOpenDocument`); the command then runs through
  `tsClient.executeCustom`, which resolves with tsserver's whole response
  object, and rejects (an LSP error, code 1) when tsserver answers
  `success: false`.
- **The fences.** `SingleTsServer.fenceCommands` is `change`, `close`,
  `open` and `updateOpen`; `getQueueingType` gives them
  `RequestQueueingType.Fence`, and no request queued after a fence is sent
  to tsserver before it. `LspDocuments.onDidChangeTextDocument` queues a
  `change` (`executeWithoutWaitingForResponse`) for each content change as
  it reads the `didChange`, and `executeCustom` queues its command as it reads
  the `executeCommand`. So a `*DiagnosticsSync` request sent after a
  `didChange` is computed on that change's text.

## The request and reply

Checked against the real server with a probe (Node, over stdio, the
`typescript` fixture of `crates/fe-lsp/tests/fixtures/real`): `didOpen`, then
a `didChange` adding, after the file's five lines,

```

const é😀 = 1; export const broken: number = "x";
/** @deprecated */ function old() {} old();
let
```

then the three requests at once, each like this:

```
{"command": "typescript.tsserverRequest", "arguments": ["semanticDiagnosticsSync", {"file": "file:///…/greet.ts"}]}
```

The `result` of each (paths shortened):

```
{"seq": 0, "type": "response", "command": "syntacticDiagnosticsSync", "request_seq": 5, "success": true, "performanceData": {"updateGraphDurationMs": 5.035202000000027}, "body": [{"start": {"line": 7, "offset": 8}, "end": {"line": 7, "offset": 10}, "text": "Invalid character.", "code": 1127, "category": "error"}, {"start": {"line": 7, "offset": 11}, "end": {"line": 7, "offset": 12}, "text": "Variable declaration expected.", "code": 1134, "category": "error"}, {"start": {"line": 7, "offset": 13}, "end": {"line": 7, "offset": 14}, "text": "Variable declaration expected.", "code": 1134, "category": "error"}]}
{"seq": 0, "type": "response", "command": "semanticDiagnosticsSync", "request_seq": 6, "success": true, "body": [{"start": {"line": 7, "offset": 7}, "end": {"line": 7, "offset": 8}, "text": "Variable 'é' implicitly has an 'any' type.", "code": 7005, "category": "error"}, {"start": {"line": 7, "offset": 29}, "end": {"line": 7, "offset": 35}, "text": "Type 'string' is not assignable to type 'number'.", "code": 2322, "category": "error"}, {"start": {"line": 9, "offset": 1}, "end": {"line": 9, "offset": 4}, "text": "Cannot find name 'let'.", "code": 2304, "category": "error"}]}
{"seq": 0, "type": "response", "command": "suggestionDiagnosticsSync", "request_seq": 7, "success": true, "body": [{"start": {"line": 8, "offset": 38}, "end": {"line": 8, "offset": 41}, "text": "The signature '(): void' of 'old' is deprecated.", "code": 6387, "category": "suggestion", "reportsDeprecated": true, "relatedInformation": [{"span": {"start": {"line": 8, "offset": 5}, "end": {"line": 8, "offset": 17}, "file": "/…/greet.ts"}, "message": "The declaration was marked as deprecated here.", "category": "error", "code": 2798}]}]}
```

- Positions are 1-based `{ line, offset }`, `offset` in UTF-16 code units:
  `é` takes one, and `😀` (offset 8 to 10) two, though the session
  negotiated nothing (utf-16 by default).
- `reportsDeprecated` came as `true`. tsserver's protocol types it and
  `reportsUnnecessary` as `{}`, so fe-lsp takes any value but `false` or
  `null` as set.
- `relatedInformation` and `performanceData` are ignored.
- A file the server hasn't open answers an LSP error (`code` 1, "TypeScript
  Server Error … No Project."), which drops that pull.

## The run

`FE_LSP_REQUIRE_SERVERS=1 scripts/lsp-real.sh` at commit d03b485, load
average (1 min) about 8 from other lanes. Servers:

- rust-analyzer 1.98.1 (48a229c 2026-09-01), the pinned toolchain's rustup
  component
- pyright 1.1.414, typescript-language-server 6.0.1 (with `typescript`
  6.0.3) and typescript-go (`typescript@7.0.2`) from `tools/lsp`, on Node
  24.21.0

Its output (`--success-output final`), each line once, test results first:

```
PASS [   3.268s] (1/7) fe-lsp::real_server real_server_rust_analyzer_utf16
PASS [   3.465s] (2/7) fe-lsp::real_server real_server_rust_analyzer_unresolved_import_unsaved
PASS [   3.693s] (3/7) fe-lsp::real_server real_server_rust_analyzer_diagnostics_follow_saves
PASS [  30.557s] (4/7) fe-lsp::real_server real_server_typescript_go
PASS [  30.797s] (5/7) fe-lsp::real_server real_server_pyright
PASS [  31.476s] (6/7) fe-lsp::real_server real_server_typescript_language_server
PASS [  33.441s] (7/7) fe-lsp::real_server real_server_rust_analyzer_utf8
Summary [  33.441s] 7 tests run: 7 passed, 105 skipped
rust-analyzer: Ready 3.03 s after launch
rust-analyzer: Ready 3.24 s after launch
rust-analyzer: the unresolved import reached the sink 13.28ms after the edit
rust-analyzer:   Some(Error) Some("rust-analyzer") Some("E0432"): unresolved import
rust-analyzer: Ready 3.37 s after launch
rust-analyzer: after the save: Some("rustc") Some("E0308"): mismatched types
rust-analyzer: after the save: Some("rustc") Some("E0308"): expected `&'static str` because of return type
rust-analyzer: after the save: Some("rust-analyzer") Some("E0308"): expected &'static str, found i32
typescript-go: Ready 0.53 s after launch
typescript-go: diagnosticProvider advertised
typescript-go: pulled for greet.ts: "full" report, 0 items
typescript-go: pulled for greet.ts: "full" report, 1 items
typescript-go: the unsaved error reached the sink 11.11ms after the edit
typescript-go:   Some(Error) Some("ts") Some("2322"): Type 'string' is not assignable to type 'number'.
pyright: Ready 0.72 s after launch
pyright: diagnosticProvider not advertised
pyright: pushed for greet.py: version Some(1), 1 items
pyright: the unsaved error reached the sink 300.21ms after the edit
pyright:   Some(Error) Some("Pyright") Some("reportAssignmentType"): Type "Literal['x']" is not assignable to declared type "int"
typescript-language-server: Ready 1.42 s after launch
typescript-language-server: diagnosticProvider not advertised
typescript-language-server: pushed for greet.ts: version None, 1 items
typescript-language-server: pulled for greet.ts: syntacticDiagnosticsSync: "response" true, 0 items
typescript-language-server: pulled for greet.ts: semanticDiagnosticsSync: "response" true, 0 items
typescript-language-server: pulled for greet.ts: suggestionDiagnosticsSync: "response" true, 0 items
typescript-language-server: pulled for greet.ts: semanticDiagnosticsSync: "response" true, 1 items
typescript-language-server: the unsaved error reached the sink 22.90ms after the edit
typescript-language-server:   Some(Error) Some("typescript") Some("2322"): Type 'string' is not assignable to type 'number'.
rust-analyzer: Ready 3.25 s after launch
rust-analyzer: diagnosticProvider advertised
rust-analyzer: pulled for src/greet.rs: error -32802 server cancelled the request
rust-analyzer: pulled for src/greet.rs: "full" report, 1 items
rust-analyzer: the unsaved error reached the sink 4.92ms after the edit
rust-analyzer:   Some(Error) Some("rust-analyzer") Some("E0308"): expected i32, found &'static str
```

`real_server_typescript_language_server` saw the unsaved type error (TS2322)
reach the sink as `LspEvent::Diagnostics` at the edited version 23 ms after
the edit, by `semanticDiagnosticsSync`. Its one push for the file in the
window still carried no version and was dropped. The first three replies,
with no items, are the pull after `didOpen`, answered once the window had
started.
