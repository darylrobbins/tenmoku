---
task_uid: "01m5n9hzn3cqkxywfpvr53er1s"
task: "T217"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "prepare_call_hierarchy and prepare_type_hierarchy return HierarchyItems (file paths, LSP ranges with their encoding, kind as OutlineKind, the raw item) or Ok(None) without the provider, sending nothing; non-file URIs are dropped"
    required: true
    manual: false
  - id: "C2"
    text: "incoming_calls, outgoing_calls, supertypes and subtypes send the raw item back unchanged to the server it came from and decode CallEdges with call-site ranges; null or timeout is empty; results capped"
    required: true
    manual: false
  - id: "C3"
    text: "HierarchyItem::selection_in and range_in convert against any text in UTF-8 and UTF-16"
    required: true
    manual: false
  - id: "C4"
    text: "New tests in hierarchy_tests.rs pass with cargo nextest run -p fe-lsp; no Cargo change"
    required: true
    manual: false
---

# Add the call hierarchy and type hierarchy requests to fe-lsp: prepare, incoming and outgoing calls, supertypes and subtypes (Mac T1114)

## Problem

Groundwork for call hierarchy and type hierarchy. fe-lsp has neither `textDocument/prepareCallHierarchy` + `callHierarchy/incomingCalls|outgoingCalls` nor `textDocument/prepareTypeHierarchy` + `typeHierarchy/supertypes|subtypes` (`rg -i hierarchy crates` is empty). lsp-types 0.95.1 has all six (`CallHierarchyPrepare`, `CallHierarchyIncomingCalls`, `CallHierarchyOutgoingCalls`, `TypeHierarchyPrepare`, `TypeHierarchySupertypes`, `TypeHierarchySubtypes`) without the `proposed` feature.

## Change

New `crates/fe-lsp/src/hierarchy.rs` (+ `pub mod hierarchy` and `pub use hierarchy::{HierarchyItem, CallEdge}` in lib.rs), on the document_highlight pattern:

- `HierarchyItem { name: String, detail: Option<String>, kind: OutlineKind, path: PathBuf, range: lsp_types::Range, selection_range: lsp_types::Range, encoding: PositionEncoding, deprecated: bool, raw: serde_json::Value }`. A hierarchy item can be in a file that isn't open, so its ranges stay LSP positions (like `FileEdits` for closed files); `HierarchyItem::selection_in(&self, text: &Text) -> Range<usize>` and `range_in` convert with `from_lsp` against any text. `raw` is the item exactly as the server sent it, sent back unchanged when asking for its calls or types (it carries `data`). Items whose uri isn't a `file:` path are dropped.
- `CallEdge { item: HierarchyItem, from_ranges: Vec<lsp_types::Range> }`: for incoming calls `item` is the caller and `from_ranges` the call sites in the caller's file; for outgoing calls `item` is the callee and `from_ranges` the call sites in the item asked about.
- `LspManager::prepare_call_hierarchy(buffer, version, offset) -> Future<Result<Option<Vec<HierarchyItem>>, LspError>>`, `incoming_calls(server_of: BufferId, item: &HierarchyItem)`, `outgoing_calls(..)`, and the type pair `prepare_type_hierarchy`, `supertypes`, `subtypes`. The prepare calls use `document_request_if_provided` (`Ok(None)` and nothing sent without `callHierarchyProvider`/`typeHierarchyProvider`); the follow-ups go to the server of the buffer they came from (the same session; a restarted server answers `Unavailable("stale")`) through `LspManager::request`. A `null` reply or timeout is `Ok(Some(vec![]))`. Results capped at 2_000 edges.
- Uses `outline_kind` from the document symbol card (`crate::document_symbol::outline_kind`), so this card depends on it.
- Client capabilities in server.rs (`call_hierarchy`, `type_hierarchy`: `dynamic_registration`); requests.rs `timeout`: the two prepare methods in the definition group (5 s), the four follow-ups in the references group (30 s). No new settings.

## Tests

New `crates/fe-lsp/src/hierarchy_tests.rs` (#[path] mod at the end of hierarchy.rs; fake server): `prepare_call_hierarchy_decodes_items`, `incoming_calls_carry_the_callers_call_sites`, `outgoing_calls_carry_call_sites_in_the_item`, `raw_item_round_trips_to_the_server` (the fake records the params), `no_provider_sends_nothing` (both kinds), `non_file_uris_are_dropped`, `null_reply_and_timeout_are_empty`, `type_hierarchy_supertypes_and_subtypes_decode`, `selection_in_converts_utf16_positions`, `edges_are_capped`. Linux-OK (headless).
