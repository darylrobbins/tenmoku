---
task_uid: "01m4rbq5s1yktxf9tv5em2w19w"
task: "T36"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "scip_index: an encoded Index with two documents loads; definitions(symbol) and references(symbol) list its sites with and without the Definition role, and a 'local 0' symbol in each document stays separate per document"
    required: true
    manual: false
  - id: "C2"
    text: "scip_index: occurrence_at finds an identifier after 'é' and an emoji on the same line in documents with UTF-8, UTF-16, UTF-32 and unspecified (read as UTF-16) position encodings; 3- and 4-element ranges both decode and other lengths are skipped"
    required: true
    manual: false
  - id: "C3"
    text: "scip_index: ScipSite::byte_range is None for a line past the text's end, an offset past its line, and an offset splitting a char"
    required: true
    manual: false
  - id: "C4"
    text: "scip_index: a truncated encoding and random bytes return ScipError::Decode and a missing path ScipError::Io(NotFound), none panicking"
    required: true
    manual: false
  - id: "C5"
    text: "cargo deny check passes with the new dependencies and cargo tree -i gpui -p fe-nav finds nothing"
    required: true
    manual: false
---

# Decode a SCIP index in fe-nav: ScipIndex loads index.scip into per-file occurrences and per-symbol definition and reference sites (Mac T488)

Filed on helium from Mac T488 (~/coordination/helium-cards/T488.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-r.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note r): this card changes Cargo.toml and Cargo.lock (T488 adds the scip and protobuf crates), so it depends on helium T30 (Mac T467, ec4rs), which also changes them; start from a main that has T30. cargo deny check must pass with the new crates. List every Cargo.lock change in the docs/specs file for the batch notes. Title the host commit "... (Mac T<n>)".

## Problem

The plan's extension seams end with "`NavigationProvider` (search-based → LSP → SCIP later)"
(~/.claude/plans/glittery-foraging-graham.md, Architecture). Search (crates/fe-nav/src/search.rs) and the
language servers (crates/fe-lsp/src/navigation.rs) answer today. A SCIP index (`index.scip`, written by
scip-typescript, scip-java, scip-python, rust-analyzer's `scip` command and others) gives exact definitions
and references for a whole repository with no server running, which suits a browse-first editor on a large
or unbuilt checkout. Nothing in fastedit can read one.

## Change

Add `ScipIndex`, a decoded SCIP index, in a new crates/fe-nav/src/scip.rs, exported from
crates/fe-nav/src/lib.rs. The provider that answers from it is a separate card that depends on this one.

- Dependency: `scip = "0.10"` (Apache-2.0) and the `protobuf` crate it re-exports or needs (MIT), added to the
  root Cargo.toml `[workspace.dependencies]` and crates/fe-nav/Cargo.toml. Both are on deny.toml's allow
  list; no deny.toml change.
- `ScipIndex::load(path)` reads the file and `ScipIndex::from_bytes(bytes)` decodes `scip::types::Index`.
  Both return `ScipError` (`Io(io::ErrorKind)`, `Decode(String)`) and never panic on a truncated or garbage
  file. The decoded protobuf isn't kept: per document (by `relative_path`) it keeps the language, its position
  encoding, and its occurrences sorted by start; across documents, a map from global symbol to its definition
  sites and to its reference sites. Symbols starting `local ` are kept per document, never global.
  `SymbolInformation` keeps `display_name` and `kind` for the provider's workspace symbols.
- Positions: an occurrence's `range` is `[line, start, end]` or `[start_line, start, end_line, end]`; any
  other length is skipped. Character offsets are in the document's `position_encoding`: UTF-8, UTF-16 or
  UTF-32 code units from the line start, and an unspecified encoding is read as UTF-16, as LSP does.
  `ScipSite { rel, start: (u32, u32), end: (u32, u32), encoding }` has
  `byte_range(&self, text: &fe_text::Text) -> Option<Range<usize>>`, `None` when the line is past the end of
  `text` or the offset is past its line or splits a char.
- Queries: `documents()`, `language(rel)`, `occurrence_at(rel, text, offset) -> Option<ScipOccurrence>` (the
  occurrence whose byte range in `text` contains `offset` or ends at it; the innermost if several), and
  `definitions(&symbol)` / `references(&symbol)` as `&[ScipSite]`. `ScipOccurrence` has the symbol, its
  `ScipSite`, its byte range in `text`, and whether its `symbol_roles` has the Definition bit.

Tests go in a new crates/fe-nav/tests/scip_index.rs. They build `scip::types::Index` values in code and
encode them with `protobuf::Message::write_to_bytes`, so no binary fixture is checked in.

## Implementation notes

- Dependencies: `scip = "0.10"` and `protobuf = "3.7"` in the root `[workspace.dependencies]`; fe-nav
  also takes `fe-text` (already a workspace crate) for `ScipSite::byte_range`. scip 0.10.0 pins
  `protobuf = "=3.7.2"`, which is past the fix for RUSTSEC-2024-0437. `cargo deny check` passes with no
  deny.toml change, and `cargo tree -i gpui -p fe-nav` matches no package.
- Cargo.lock changes:
  - added `scip 0.10.0` (Apache-2.0; depends on protobuf)
  - added `protobuf 3.7.2` (MIT; depends on once_cell, protobuf-support, thiserror 1.0.69, all already
    in the lock)
  - added `protobuf-support 3.7.2` (MIT; depends on thiserror 1.0.69)
  - `fe-nav` gains `fe-text`, `protobuf` and `scip` in its dependency list
- protobuf 3.7.2 takes a nested message that the input ends inside of as ending there, so an index cut
  through its last document decoded as that document with its tail missing. `from_bytes` therefore also
  walks the top-level fields, and into any (unknown) groups there, and returns
  `ScipError::Decode("truncated")` when one runs past the end or a group is left open: a cut file always
  cuts its last top-level field. The test cuts a one-document index at every length, and puts an empty
  group before a cut document (review r2r4vc F1).
- Locals: `definitions`/`references` answer for global symbols only; a document's locals are queried
  with `local_definitions(rel, scope, symbol)` / `local_references(rel, scope, symbol)`, where
  `scope` (`ScipOccurrence::scope`) is which of the index's documents for that path the local came
  from, so documents sharing a `relative_path` keep separate `local 0`s (review r2r4vc F2).
  `definitions_of(&occurrence)` / `references_of(&occurrence)` answer for either kind. `symbol_info(symbol)` and
  `symbols()` give the `display_name` and `kind` from documents' and `external_symbols`'
  `SymbolInformation` (locals' are dropped).
- Ranges: an occurrence with an empty `range` falls back to the `typed_range` oneof (single- or
  multi-line) that scip 0.10 adds. Occurrences with an empty symbol, a negative value, or an end before
  the start are skipped. Documents sharing a `relative_path` are merged, each occurrence keeping its
  own document's encoding and local scope.
- `occurrence_at` looks back only as many lines as the document's longest occurrence spans, and picks
  the shortest range containing or ending at the offset; between two adjacent ones it picks the one the
  offset is inside rather than at the end of.
