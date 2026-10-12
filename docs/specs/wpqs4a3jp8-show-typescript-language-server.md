---
task_uid: "01m4r0ptrckac24wwpqs4a3jp8"
task: "T73"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::tsserver_diagnostics::decodes_tsserver_diagnostics: 1-based line/offset in UTF-16 code units decode to the right byte ranges under both utf-8 and utf-16 sessions (with an emoji and a multibyte character before the range); error/warning/suggestion/message map to Error/Warning/Hint/Information, reportsUnnecessary and reportsDeprecated to their tags, code is kept and source is typescript; a malformed entry is skipped"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::tsserver_diagnostics::pulls_through_tsserver_request: for a spec with pull_diagnostics TsserverRequest, after didOpen and each change the fake gets workspace/executeCommand typescript.tsserverRequest for syntacticDiagnosticsSync, semanticDiagnosticsSync and suggestionDiagnosticsSync back to back; their answers become one LspEvent::Diagnostics at the version sent before them; one failing drops that pull; the server unversioned publishDiagnostics are still dropped"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::tsserver_diagnostics::builtin_specs: the built-in typescript-language-server spec has pull_diagnostics TsserverRequest and unversioned_diagnostics Drop; typescript-go, pyright and rust-analyzer have Advertised, the default"
    required: true
    manual: false
  - id: "C4"
    text: "FE_LSP_REQUIRE_SERVERS=1 scripts/lsp-real.sh passes on Linux, with real_server_typescript_language_server seeing the type error in the unsaved file as LspEvent::Diagnostics at the edited version within 30 s; the decision record holds the run output and the server source evidence (FileDiagnostics merge, fence commands), and lsp-client.md §4.1 and §10 are updated"
    required: true
    manual: false
  - id: "C5"
    text: "No file outside the scope changes, apart from the task's own spec under docs/specs/ and its decision record; Cargo.toml and Cargo.lock are unchanged; every earlier fe-lsp test passes with its files unchanged"
    required: true
    manual: false
---

# Show typescript-language-server diagnostics by pulling them through typescript.tsserverRequest (Mac T305)

# fe-lsp: typescript-language-server's diagnostics, pulled through `typescript.tsserverRequest`

## Problem

Found by T148's real-server tests (2026-10-09, `docs/decisions/lsp-client.md` §10, "Which servers
version their diagnostics"). typescript-language-server 6.0.1, the default TypeScript server, never
sets `version` on `publishDiagnostics`. fe-lsp drops unversioned publications
(`UnversionedDiagnostics::Drop`, §4.1, `server.rs` `published`), so TypeScript and JavaScript files
show no diagnostics under the default server.

**No verified guarantee is possible** (§4.1's exception), from its source (npm
`typescript-language-server@6.0.1`, `lib/cli.mjs`; checked 2026-10-11 by the card drafter):

- `FileDiagnostics` keeps one list per tsserver event kind (`syntaxDiag`, `semanticDiag`,
  `suggestionDiag`). It replaces a kind as its event arrives, never clears them on a change, and
  publishes all three merged, debounced 50 ms (`firePublishDiagnostics`). So one publication can join
  syntax diagnostics of the new text with semantic ones of an older text.
- It has no `diagnosticProvider`, so T306's `textDocument/diagnostic` doesn't apply.

It does register the `typescript.tsserverRequest` command (`TSServerRequestCommand`, listed in
`executeCommandProvider.commands`), which runs any tsserver command through its request queue.
tsserver's `change`, `open`, `close` and `updateOpen` are **fence** commands
(`SingleTsServer.fenceCommands`): no request queued after one runs before it. So a
`*DiagnosticsSync` request sent after a `didChange` is computed on that change's text, the same rule
T306 uses for pulled diagnostics.

## Change

Builds on T306's pull machinery (`crates/fe-lsp/src/pull.rs`): its triggers, one pull in flight per
document, version at request, and merge with pushed diagnostics.

- **`ServerSpec`** (`launcher.rs`) gains `pull_diagnostics: PullDiagnostics`, re-exported from
  `lib.rs`:
  - `Advertised` (the default) is T306's behaviour: `textDocument/diagnostic` when the server
    advertises `diagnosticProvider`.
  - `TsserverRequest` pulls through `workspace/executeCommand`. Its doc comment names the guarantee
    above and cites this task's test and decision record.
- **The built-in `typescript-language-server` spec** (`registry.rs` `builtin_specs`) sets
  `TsserverRequest` and keeps `unversioned_diagnostics: Drop`: its pushes are still dropped.
  typescript-go and the others keep `Advertised`.
- **A `TsserverRequest` pull** (new `crates/fe-lsp/src/tsserver.rs`, called from `pull.rs`) sends
  three `workspace/executeCommand` requests back to back, with no notification between them:
  - `{ command: "typescript.tsserverRequest", arguments: [<cmd>, { "file": <uri> }] }`
  - `<cmd>` is `syntacticDiagnosticsSync`, then `semanticDiagnosticsSync`, then
    `suggestionDiagnosticsSync`.

  All three belong to the version sent before the first. When all three have answered, their
  diagnostics together are one pulled list for that version. If any of the three fails, is cancelled
  or times out, that pull is dropped, as in T306.
- **Decoding tsserver's diagnostics** (`tsserver.rs`). The reply is tsserver's response object;
  `body` is the diagnostic array. Each diagnostic has `start` and `end` `{ line, offset }`, both
  1-based, with `offset` in UTF-16 code units **whatever position encoding the session negotiated**.
  Decode them in the ring's text for the version.
  - `category`: `error` → Error, `warning` → Warning, `suggestion` → Hint, `message` → Information.
  - `reportsUnnecessary` → the Unnecessary tag, `reportsDeprecated` → Deprecated.
  - `code` is kept, and `source` is `"typescript"`.
  - Malformed entries are skipped and logged.
- Confirm the exact request and reply shape against the real server first, and record it in the
  decision record.
- **Real servers**: `real_server_typescript_language_server` (`crates/fe-lsp/tests/real_server.rs`)
  checks that the type error in the unsaved file reaches the sink as `LspEvent::Diagnostics` at the
  edited version. `versions_diagnostics` still records that its pushes carry no version.
- **Docs**: in `docs/decisions/lsp-client.md`, §4.1 names this server's mechanism, and §10's
  typescript-language-server entry gets the evidence. The real-server run's output goes in this
  task's decision record.

## Tests (new file)

`crates/fe-lsp/src/tests/tsserver_diagnostics.rs`, declared in `crates/fe-lsp/src/tests/mod.rs`, on
the fake server (`set_reply("workspace/executeCommand", …)`, with delays).

## Out of scope

Other tsserver commands, code actions, a user setting for `pull_diagnostics`, and any UI change.

## Implementation notes

- **Back to back.** `document.rs` queues one pull request in the outbox. For a tsserver pull that's
  the first of the three (`tsserver::Lead`), carrying the other two: as the actor sends it, it
  moves them to the puller's `released` list, and `Actor::issue` sends them right behind it in
  the same step, so nothing gets between them. They may take async-lsp's queue up to two past
  `QUEUED_MAX`.
- **UTF-16 whatever the session.** `document.rs` decodes a pulled list in `link.encoding`, so
  `Actor::pulled` sets it to utf-16 for that one call when the reply is tsserver's
  (`Pulled::encoding`) and restores it after. tsserver positions become 0-based UTF-16 LSP
  positions in `tsserver::diagnostics`; the ring's text decodes them like any pulled list.
- **One failing.** The three replies are joined with `try_join3` under one `PULL_TIMEOUT`: the
  first error drops the pull, and the requests still out are cancelled.
- **The command must be registered.** A `TsserverRequest` session pulls only if the server lists
  `typescript.tsserverRequest` in `executeCommandProvider.commands`.
- **Tests.** The fake answers one scripted reply per method, taken as each request arrives, so the
  tests pace the client's writes (`Writes::Paced`) and change the reply between the three.
