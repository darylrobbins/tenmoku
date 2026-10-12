---
task_uid: "01m4rbq61dhkrq5cxdscb8f90w"
task: "T37"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "scip_provider: readiness is Ready for an indexed language (a 'TypeScriptReact' document counting as tsx) and Unsupported for another; readiness(None) is Ready exactly when the index has a global symbol"
    required: true
    manual: false
  - id: "C2"
    text: "scip_provider: definitions at a use returns the definition in another file with its byte range and row in that file's text, tier Project and unambiguous; a local symbol resolves within its own document only"
    required: true
    manual: false
  - id: "C3"
    text: "scip_provider: when an edit in the request text moves the identifier off its indexed position, definitions returns NavError::Unavailable, and a Navigation with a search provider after it answers from search; a definition site whose file changed is dropped"
    required: true
    manual: false
  - id: "C4"
    text: "scip_provider: references hands each file's references to sink in line order, verified, with is_definition from the role; a cancel set after the first batch ends with summary.cancelled and no further sink call"
    required: true
    manual: false
  - id: "C5"
    text: "scip_provider: workspace_symbols returns at most limit matching global symbols ranked by WorkspaceSymbol::rank, each at its first definition site, leaving out one whose file changed"
    required: true
    manual: false
---

# Answer go-to-definition, references and workspace symbols from a SCIP index: ScipNavigation, an exact NavigationProvider in fe-nav (Mac T489)

Filed on helium from Mac T489 (~/coordination/helium-cards/T489.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-r.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note r): this card changes Cargo.toml and Cargo.lock (T488 adds the scip and protobuf crates), so it depends on helium T30 (Mac T467, ec4rs), which also changes them; start from a main that has T30. cargo deny check must pass with the new crates. List every Cargo.lock change in the docs/specs file for the batch notes. Title the host commit "... (Mac T<n>)".

## Problem

The card before this one adds `ScipIndex` (crates/fe-nav/src/scip.rs), which decodes a SCIP index. The plan's
seam is "`NavigationProvider` (search-based → LSP → SCIP later)" (~/.claude/plans/glittery-foraging-graham.md),
and `Navigation` (crates/fe-nav/src/provider.rs:353) routes each request to the ready providers in precedence
order, moving to the next one on `NavError::Unavailable` or `NavError::Unsupported` (provider.rs:465). No
provider answers from an index yet.

## Change

Add `ScipNavigation` in a new crates/fe-nav/src/scip_provider.rs, exported from crates/fe-nav/src/lib.rs,
implementing `NavigationProvider` from an `Arc<ScipIndex>` and the project root. Registering it in the
workbench (finding `index.scip`, its precedence against the language servers) is a later Mac card; nothing
outside fe-nav changes here.

- `name()` is `"scip"`, `precision()` is `Precision::Exact`.
- `readiness(Some(language))`: `Ready` when some indexed document's language, lowercased, equals the
  fe-syntax name (`"typescriptreact"` counts as `"tsx"`, `"c++"` as `"cpp"`, `"c#"` as `"csharp"`, `"shell"`
  as `"bash"`), else `Unsupported`. `readiness(None)`: `Ready` when the index has any global symbol.
- Request text: the occurrence comes from `ScipIndex::occurrence_at(req.rel, req.text, req.offset)`, and it
  counts only when its bytes in `req.text` equal `req.word()`. Otherwise (unsaved edits or a file changed since
  it was indexed) the answer is `Err(NavError::Unavailable("not in the SCIP index"))`, so search answers.
- Other files are read with `fe_project::open_file_under(root, rel)` into the text the editor loads (as
  crates/fe-nav/src/extract.rs does), each at most once per request. A site whose bytes in that text aren't the
  symbol's name (the file changed since indexing) is dropped, as is a site in a file that can't be read.
- `definitions`: every definition site of the symbol as a `Definition` with `Location { rel, range, row,
  snapshot: None }`; the request's own file uses `req.text` and `req.buffer`. Tier `SameFile` or `Project`,
  `kind` from `SymbolInformation.kind` where it names an `OutlineKind`, `unambiguous` for exactly one,
  `partial` false. A local symbol's sites are its document's only. No site left: `Unavailable`.
- `references`: the symbol's occurrences grouped per file in line order, handed to `sink` one file at a time,
  each `Reference` `verified: true`, `is_definition` from the Definition role, previews from
  `Reference::preview`. `cancel` is checked between files, with the summary's `cancelled` set as the trait
  documents. A local symbol searches its document only.
- `workspace_symbols(query, limit)`: the global symbols with a definition site and a kind that maps to an
  `OutlineKind`, located at their first definition site, ranked by `WorkspaceSymbol::rank`. No file is read
  for the ranking; only the `limit` returned are resolved to byte ranges, and one whose file changed is dropped.

Tests go in a new crates/fe-nav/tests/scip_provider.rs, building indexes in code as the index card's tests do,
over files written to a tempdir.

## Rev 2 (helium lead): the alias rule after review round 3's F5

Review rounds ran out with F5 open (C3 and C4 unmet). Same criteria; the spelling check must be tight enough that a
stale site is never reported as exact and verified:

- Every **definition** site of a global symbol is checked against the symbol's own name (display name, else its last
  descriptor's) before any caret exception. A definition renamed in place (`foo` -> `bar`, caret on `bar`) is a
  changed file: `definitions` drops it (no site left: `Unavailable`, so search answers), and `references` doesn't
  report it.
- A **non-definition** site spelt differently from the symbol's name (an import alias) is accepted only with evidence
  from the index that the alias is that site's indexed spelling, not because it is in the request's file. For example:
  the request file's index has an occurrence of the alias itself (a local or import symbol at the alias's declaration)
  whose range and bytes in the request text match, and the site lies where that alias is in scope. If you can't get
  such evidence from SCIP for a site, drop the site rather than accept it.
- Regression tests in crates/fe-nav/tests/scip_provider.rs for both of F5's cases: (1) the indexed global definition
  renamed from `foo` to `bar` with the caret on `bar`: no exact answer, and no verified reference at it; (2) in the
  alias fixture, the canonical import `crate::foo` changed to `crate::bar` while requesting the final `bar`: the
  changed import isn't reported as a verified reference to `foo`. The existing alias test
  (`an_alias_resolves_to_its_symbol_spelt_as_defined`) keeps passing.

## Rev 3 (helium lead): the site's language after review round 3's F7

Rev 2's rounds ran out with F7 open (C3, C4, C5 unmet). F6 is fixed. Same criteria; one more rule for the spelling
check:

- The identifier chars at a **site** (whether `$` is one) follow the language the index gives that site's document
  (`ScipIndex::language(rel)`, normalized as the readiness languages are, through `syntax_name`), not its file name.
  Fall back to `language_name_for_path` only when the index has no language for the document. The request's own text
  keeps using the request's language. This covers both checks that now call `language_name_for_path(&site.rel)`
  (`accept` and `site_range` in crates/fe-nav/src/scip_provider.rs).
- Regression test in crates/fe-nav/tests/scip_provider.rs: an extensionless file the index records as JavaScript,
  whose indexed global definition `foo` has grown into `foo$` on disk: no exact answer from `definitions`, and no
  verified reference at that site from `references`.
- Nothing else changes from rev 2. Keep the existing tests passing.

## Implementation notes

- No Cargo.toml or Cargo.lock change: `scip` and `protobuf` came with T36 (Mac T488). Nothing outside
  crates/fe-nav changes.
- crates/fe-nav/src/scip.rs gains one method, `ScipIndex::has_global_symbols` (a global symbol occurring in a
  document, or one only described, an external symbol say), because the index's undescribed globals can't be
  seen through its public API. `readiness(None)` is `Ready` exactly when it's true.
- Besides the four spellings in the spec, `"javascriptreact"` counts as `"javascript"` (fe-syntax parses
  `.jsx` as javascript).
- Rev 2's alias rule. A global symbol's sites are spelt by its own name (its display name, or its last
  descriptor's when it has none). A definition site, the caret's included, must have that name, so a definition
  renamed in place (`foo` -> `bar`) is a changed file: no exact answer at it, and it's never a reference. A
  non-definition site spelt otherwise is taken only with evidence from the index: another symbol (the alias's own,
  as an import alias's local) occurs on exactly the site's span, and that symbol is defined in the same file at a
  site whose bytes now are the site's spelling. The occurrence on the span is the indexer saying the alias is in
  scope there. Without it the site is dropped, in the request's file as in any other, and a caret on such a site
  is `Unavailable`. The index stores positions, not text, so it can't tell `use crate::foo as bar` with its
  `foo` changed to `bar` from the alias itself on symbol occurrences alone; the alias's own symbol is the
  evidence. An indexer that writes no symbol for an alias gets search's answer at it.
- crates/fe-nav/src/scip.rs gains `ScipIndex::occurrences_on(site, text)`, every occurrence spanning exactly a
  site, for that evidence. When the caret's innermost occurrence is a local sharing its span with a global
  symbol (an alias's use), the request is for the global one.
- A local symbol's sites are checked against the word at the caret.
- `references` hands over the request's own file first (from `req.text`, located with `req.buffer`), then the
  rest by path; a site the index lists twice is reported once. The cancel flag is checked before each file and
  again before its batch goes to `sink`, so a cancel while a file is read sends nothing more. `files_searched`
  counts files whose sites were looked at, `files_verified` those that could be read; `dropped` stays 0.
- `workspace_symbols` names a symbol as above; `container` is `None`. Candidates are sorted by name, path and
  position before ranking, so equal scores come out in a stable order. Only the best `limit` are resolved, and
  one whose file changed is left out without being replaced, so a stale index reads at most `limit` files.
- Files are read up to 32 MiB, as references search loads them. A `#[cfg(test)]` hook on file loads lets the
  unit tests in scip_provider.rs cancel a request while a file is read.
- Review round 1 of rev 2:
  - A name is checked whole: a site whose bytes are the name but sit inside a longer identifier (`foo` grown into
    `foobar`) is changed, for definitions, references, workspace symbols and alias evidence alike.
  - A global alias symbol is spelt by its own name in the index too, so an alias renamed (declaration and uses
    together) is no evidence.
  - The alias's symbol counts only from the same document (scope) as the aliased symbol's occurrence on the span,
    and a caret's local is promoted to a global only from its own document: `occurrences_on` reports each
    occurrence's scope.
  - References resolve each file's sites to bytes before ordering them, since a file's documents may count columns
    in different encodings; a site listed twice is reported once, as a definition if either listing says so.
    Definitions drop such duplicates too.
  - `ScipIndex::languages()` lists every document's language, a file split across documents included, for
    readiness.
- Review round 2 of rev 2: identifier chars for the whole-name check are the search's (`fe_project::is_word_char`,
  Unicode's `\w`, so a combining mark grows a name), with `$` in JavaScript, TypeScript and TSX files (by the
  site's file name; by the request's language at the caret), as `NavRequest::word` has them. A site taken as an
  alias must be a whole identifier itself, besides the alias's declaration.
- Rev 3: whether `$` is an identifier char at a site follows the language the index gives the site's document
  (`ScipIndex::language`, through `syntax_name`), falling back to the file name only when the index gives none; the
  caret's word still follows the request's language. One method, `ScipNavigation::dollar_at`, serves `accept`,
  `site_range` (through `Texts::locate`) and the alias evidence alike.
- Rev 3, review round 1: a site in the request's own file is checked in the request's text, so its identifier chars
  follow the request's language, as the caret's word does: for definition locations, reference sites and alias
  evidence alike. Other files follow the index's language for their document, then their file name.
