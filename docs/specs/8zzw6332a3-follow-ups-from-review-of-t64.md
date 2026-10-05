---
task_uid: "01m46sjdsnxkjne38zzw6332a3"
task: "T66"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T64

Findings of the review of T64 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-syntax/queries/outline/markdown.scm:20: Setext headings use only the heading node's range, so they cannot parent subsequent headings or body text. For `A\n===\n\n## B\n`, B becomes a top-level item instead of A's child; conversely, a level-1 setext heading following `# A` incorrectly becomes A's child. The current test places setext last and misses both cases.
  fix: Derive Markdown section extents and nesting from heading levels for both heading forms, keeping containment and item_at consistent; test setext parents and same-level mixed-form siblings.
  instances: crates/fe-syntax/queries/outline/markdown.scm:20, crates/fe-syntax/queries/outline/markdown.scm:22

- F2 medium crates/fe-syntax/queries/outline/c.scm:4: C declarator coverage silently omits valid definitions: functions returning three pointers or using parenthesized declarators, and typedefs such as `typedef int Numbers[4];` or `typedef void (*Callback)(int);`. Function matching stops at two pointer levels, while typedef matching permits only a bare name or one pointer.
  fix: Cover the supported grammar's declarator chains for function definitions and typedefs, extracting the declared identifier without including parameter names; add golden cases for these forms.
  instances: crates/fe-syntax/queries/outline/c.scm:4, crates/fe-syntax/queries/outline/c.scm:21

- F3 medium crates/fe-syntax/queries/outline/go.scm:22: Declarations with multiple names produce separate matches for the same definition node, and node deduplication discards every name after the first. Thus `const A, B = 1, 2` and `typedef int A, B;` only expose A rather than preserving the complete name capture span.
  fix: Capture all declaration names in one match so the specified first-to-last name-range rule applies; add multi-name Go constant and C typedef samples.
  instances: crates/fe-syntax/queries/outline/go.scm:22, crates/fe-syntax/queries/outline/c.scm:21

- F4 medium crates/fe-syntax/queries/outline/rust.scm:16: Rust declaration variants are missing: function signatures outside traits, including valid extern-block functions, never become Functions; signatures inside impls never become Methods; and associated type declarations such as `trait T { type Output; }` never become Types.
  fix: Add a function_signature_item fallback after the method patterns, include signatures in the impl method pattern, and capture associated_type as Type; test their kinds, nesting and deduplication.
  instances: crates/fe-syntax/queries/outline/rust.scm:7, crates/fe-syntax/queries/outline/rust.scm:16, crates/fe-syntax/queries/outline/rust.scm:31

- F5 medium crates/fe-syntax/queries/outline/java.scm:15: Java compact record constructors and annotation methods use distinct grammar nodes that these patterns omit. Consequently `record R(int x) { R {} }` loses its constructor, and the supplied Marker annotation already loses its value() method; the golden codifies that omission.
  fix: Capture compact_constructor_declaration and annotation_type_element_declaration as Methods, add a compact-constructor sample, and update the Java golden to include value().
  instances: crates/fe-syntax/queries/outline/java.scm:13, crates/fe-syntax/queries/outline/java.scm:15, crates/fe-syntax/tests/outline_samples/java.java.outline:10

- F6 medium crates/fe-syntax/src/outline.rs:249: ATX closing-marker cleanup runs on every Heading, including setext titles, and runs after truncation. A setext title `Title #` incorrectly becomes `Title`; an ATX title truncated immediately after an internal ` #` can also lose legitimate content.
  fix: Remove closing sequences only from actual ATX heading content, before whitespace normalization and truncation; test literal hashes in setext titles and truncation near internal hashes.
  instances: crates/fe-syntax/src/outline.rs:249, crates/fe-syntax/src/outline.rs:308

- F7 medium crates/fe-syntax/queries/outline/markdown.scm:8: Every ATX pattern requires heading_content, but the grammar allows empty headings such as `#`. These sections disappear entirely, and subsequent nonempty child headings lose their outline parent, contrary to listing each section.
  fix: Support empty ATX headings with an empty name and a valid zero-length name_range, and test empty parents and all six levels.
  instances: crates/fe-syntax/queries/outline/markdown.scm:8, crates/fe-syntax/queries/outline/markdown.scm:10, crates/fe-syntax/queries/outline/markdown.scm:12, crates/fe-syntax/queries/outline/markdown.scm:14, crates/fe-syntax/queries/outline/markdown.scm:16, crates/fe-syntax/queries/outline/markdown.scm:18

## Implementation notes

- **F1, F7:** `markdown.scm` now captures each `atx_heading` and `setext_heading` node, with an
  optional `heading_content` for ATX. `outline.rs` gives every heading the range of its section:
  from the heading to the next heading of the same or a higher level in the same container (the
  document, a block quote or a list item, i.e. the nearest ancestor that isn't a grammar
  `section`), else to the container's end. tree-sitter-md's own sections can't be used: it makes
  one for a setext heading at the start of the document but not after an ATX section. Ranges
  derived this way nest, so `item_at` and parents stay consistent. An empty ATX heading is listed
  with an empty name and a zero-length `name_range` at the end of its marker.
- **F6:** the closing sequence is cut from the ATX content's byte range before the name is
  normalized and truncated, and only for `atx_heading`s. `name_range` now ends before it, so it
  covers just the name. As in CommonMark, only the last run of `#`s goes (`# a # #` is `a #`), and
  only after a space.
- **F2:** C's function and typedef patterns capture the whole `declarator`, and a `@name` capture
  on a `*_declarator` node stands for the identifier at the end of its `declarator` chain, through
  parenthesized and attributed declarators, which have no field. Parameter names are never on that
  chain.
- **F3:** Go's const spec and C's typedef capture all their names in one match (`name: … @name
  ("," name: … @name)*`). If the cursor also returns a match of the same pattern on the same node
  with fewer names, the name ranges are merged, so the result doesn't depend on match order. The
  name is the text from the first name to the last, as the T64 rule says, so
  `typedef int a, *b;` is named `a, *b`.
- **F4, F5:** query additions only; the samples and goldens cover each new form.
