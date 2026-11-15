# C# `///` doc comments: XML layers through `injection.doc-prefix` (T649, Mac T1835)

## Context

tree-sitter-c-sharp 0.23.5 makes each `///` line one `comment` node, the
`///` included. Injecting XML per line would leave the prefix in the layer and
split `<summary>` from `</summary>` when they sit on lines of their own.
`injection.combined` combines a node with every same-kind sibling under its
parent, so plain `//` comments and the next member's doc block would join it.
T637 (Mac T1826) added `injection.trim-delimiters`, which cuts PHP-style
`/body/flags` delimiters: not a prefix cut, so it doesn't fit.

## Decision

One property of our own, `injection.doc-prefix "<prefix>"`, documented in
`crates/fe-syntax/src/injection.rs`'s module doc:

- a content node is injected only if its text starts with the prefix and
  isn't followed by the prefix's last byte again (`////` isn't a `///` line);
- the prefix is left out of the layer's ranges (nvim's `#offset!` start
  offset, but by the prefix's length, so only real doc lines are cut);
- the node is combined with the **run** of such siblings of its kind on
  consecutive rows, with no other node between: one layer per doc block.

The run is found by one forward walk over the parent's children up to the
run's end, checking the deadline at each; the run's lines go into the
per-call `combined` set so later matches in it are skipped. The prefix sits
in a `doc_prefixes` vector beside `Pattern` rather than in it, so
`injection/tests.rs` (held by other cards), which builds `Pattern` literally
and calls `match_ranges` and `content_ranges`, compiles unchanged.

The query is the same file for the built-in and the WebAssembly extension
(`csharp-injections.scm` = `extensions/grammars/csharp/injections.scm`),
which also ships T636's regex injections to the extension.

## Consequences

- A doc block is a run of root elements (`<summary>`, `<param>`, `<returns>`);
  tree-sitter-xml parses it with error recovery and still colours every tag,
  attribute and value (see the golden). `(ERROR) @error` and
  `(CharData) @markup` map to no highlight of ours, so text keeps the comment
  colour.
- Limits: `/** */` doc comments aren't injected; a blank line inside a block
  splits it into two layers; an unclosed or misnested tag can leave the rest
  of its block plain.
