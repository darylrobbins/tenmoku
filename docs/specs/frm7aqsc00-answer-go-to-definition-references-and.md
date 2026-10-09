---
task_uid: "01m49phf966anwgmfrm7aqsc00"
task: "T144"
rev: 7
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-nav: Definition.kind is Option\u003cOutlineKind\u003e, NavRequest has buffer: Option\u003c(BufferId, Version)\u003e and Location has snapshot: Option\u003c(BufferId, Version)\u003e; SearchNavigation sets kind Some and snapshot None; crates/fe-nav/tests/provider.rs, definition.rs and references.rs change only to fill the new fields, and every fe-nav test passes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp test tests::navigation::routing_follows_readiness: with Navigation holding SearchNavigation at 0 and LspNavigation at 100 over the fake, definitions are answered by search (precision Search) while the server is Starting, and by the fake's exact location (precision Exact) without calling search once it is Ready; Restarting reads NotYet; a language with no configured server, LSP disabled, and a Stopped server read Unsupported"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp test tests::navigation::empty_answers_and_errors: an empty definition reply from a progress-mode server and from a server-status server with quiescent false gives Err(Unavailable) and search answers; from a server-status server whose latest status is quiescent true it is final and search is not called; a timeout, ServerDown, ContentModified, a document over max_file_bytes, and NavRequest.buffer None each give Unavailable and search answers"
    required: true
    manual: false
  - id: "C4"
    text: "fe-lsp test tests::navigation::stale_and_cancelled_requests: a request whose buffer version is older than the version already sent gives Unavailable(\"buffer changed\") and search answers on request.text; setting the cancel flag while the fake holds its reply makes the fake receive $/cancelRequest for that id and the call return Err(Cancelled) within 50 ms"
    required: true
    manual: false
  - id: "C5"
    text: "fe-lsp test tests::navigation::offsets_by_file: for Location and LocationLink replies, a target in the request's file has a range indexing request.text, tier SameFile and snapshot equal to the request's buffer; a target in another open document is decoded against the text the server held at admission and carries that buffer's (BufferId, Version); a target in an unopened CRLF file with an emoji before it selects the name in Text::from_reader's text, with tier Project and snapshot None; every kind is None; utf-8 and utf-16 sessions give the same ranges"
    required: true
    manual: false
  - id: "C6"
    text: "fe-lsp test tests::navigation::references_and_symbols: references sends includeDeclaration true and emits one FileReferences batch per file with the request's file first, every reference verified, is_definition set only on the range equal to the definition reply, and preview and preview_ranges from the target line; workspace symbols map name, kind and location from workspace/symbol, with indices from the same fuzzy matcher SearchNavigation uses"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test tests::lsp_nav::goto_definition_before_and_after_ready: with the fake Starting, f12 jumps by search and the status reads (by search); after the fake reports Ready, f12 jumps to the fake's location and no (by search) shows; text typed above the target after the answer and before the jump is rebased through Location.snapshot; requests carry the active editor's (BufferId, Version); shift-f12 lists the fake's references in T112's panel, rebased the same way"
    required: true
    manual: false
  - id: "C8"
    text: "Manual: in the app on this repository with rust-analyzer installed, go-to-definition on a method call answers by search before rust-analyzer is Ready and precisely (no \"by search\") after it; noted in the criteria file"
    required: true
    manual: false
  - id: "C9"
    text: "LspNavigation is added once per workbench at precedence 100 from crates/fe-workbench/src/lsp.rs; Cargo.lock changes only fe-lsp's dependency list (gains fe-nav, no new packages); scripts/check-no-gpui.sh prints ok: fe-lsp and ok: fe-nav"
    required: true
    manual: false
---

# Answer go-to-definition, references and workspace symbols from language servers, through fe-nav's NavigationProvider seam

# Answer go-to-definition, references and workspace symbols from language servers, through fe-nav's NavigationProvider seam

Card 6 of `docs/decisions/lsp-client.md` §8. The design is §5 (all of it, including its "Seam gaps"
1–3), with §3.4 "Empty answers", §4.1 "Request admission" and §7's Definition, References and Workspace
symbols rows. It builds on cards 1–5, on T105's `Navigation` and `NavigationProvider`, on T106's
`references` with a streaming sink, on T107's workbench nav (`nav.rs`, `nav_picker.rs`) and on
T112's references panel. T114's cmd-click picks this up unchanged.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"):

- **Q5 (c):** TypeScript and JavaScript route to whichever TS server `lsp.typescript_server` picks.
- **Q6 (a):** until the lazily started server is `Ready`, search answers.

## Change

### `fe-nav` seam additions (§5 "Seam gaps" 1–3)

These go in `crates/fe-nav/src/provider.rs`. `search.rs` and `references.rs` fill the new fields.

- `Definition.kind` becomes `Option<OutlineKind>`. Search always sets `Some`.
- `NavRequest` gains `buffer: Option<(BufferId, fe_buffer::Version)>`, the snapshot `text` came
  from.
- `Location` gains `snapshot: Option<(BufferId, fe_buffer::Version)>`. `None` means the file's
  loaded text on disk, which is search's case.
- Existing fe-nav tests change only to fill the new fields.

### `crates/fe-lsp/src/navigation.rs` (new): `LspNavigation`

- **One provider for every language.** It is bound to the workbench's manager and root, and asks the
  manager which server handles `NavRequest.language`.
- **Readiness:**
  - `Ready` when that server is `Ready`
  - `NotYet` while it is `Starting` or `Restarting`
  - `Unsupported` with no server, LSP disabled, or `Stopped`
- **`definitions`.** Admit the request with its `(BufferId, Version)` (card 2b), send
  `textDocument/definition` (`linkSupport`), and `block_on` the reply, polling `cancel` every 10 ms.
  When the flag is set, send `$/cancelRequest` and return `Err(Cancelled)`.
- **Conversion.** `Location` and `LocationLink` are converted per §5 "Offsets":
  - **The request's file:** against `request.text`, with tier `SameFile` and `snapshot` set.
  - **Other open documents:** against the ring entry captured at admission, carrying its
    `(BufferId, Version)`.
  - **Unopened files:** read with `Text::from_reader` once per file per answer, with tier `Project`
    and `snapshot` `None`.
  - `kind` is `None`.
- **Errors.** `Unavailable` covers:
  - timeouts, `ServerDown`, `Retry` and `ContentModified`
  - a too-large file
  - `buffer` `None`
  - a version the buffer has moved past
  - a non-final empty answer (card 4)

  A non-empty answer is final.
- **`references`.** Send `includeDeclaration: true`, with a `definition` request alongside. Emit
  one `FileReferences` per file, the request's file first. Each has `verified: true`,
  `is_definition` when its range equals a definition, and `preview` and `preview_ranges` from the
  target line.
- **`workspace_symbols`.** `workspace/symbol` maps onto `WorkspaceSymbol`. `indices` come from the
  same fuzzy matcher search uses.
- `crates/fe-lsp/Cargo.toml` gains `fe-nav` (so `Cargo.lock`'s fe-lsp entry changes).

### `fe-workbench`

- **`src/lsp.rs`:** adds one `LspNavigation` to `Workbench::navigation()` at precedence 100 when
  the manager is created.
- **`src/nav.rs`:** requests carry the active editor's `(BufferId, Version)`. Before selecting a
  `Location` with a `snapshot`, rebase its range with `BufferSnapshot::map_offset` from that version,
  or fall back to its row when the version is below the edit-log floor.
- **`src/references_panel.rs`:** rebases references the same way before opening one.
- **`src/nav_picker.rs`:** shows no kind when `kind` is `None`.

### Manual check

M4's "go-to-def … before and after the LSP is ready": in the app on this repository with
rust-analyzer, go-to-definition answers "by search" before Ready and precisely after it.

## Out of scope

- Multi-root `readiness` (§5 gap 4).
- Hover and completion (cards 8 and 9).
- Real-server tests (card 10).
