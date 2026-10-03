---
task_uid: "01m417z48ngbbq16vdxe113rn8"
task: "T20"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test tests::each_language_highlights_a_snippet passes with a case for each added language"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test tests::golden::highlights_match_golden_files passes with a sample for each added language"
    required: true
    manual: false
  - id: "C3"
    text: "the syntax_features gate stage passes with each new feature built alone"
    required: true
    manual: false
  - id: "C4"
    text: "the deny gate stage passes"
    required: true
    manual: false
---

# Add Swift, Kotlin, SQL, Dockerfile and Make grammars

## Problem

The second batch of T-14's grammar candidates: **Swift, Kotlin, SQL, Dockerfile, Make**. Their crates are less uniform than batch 1's. Several are community-maintained, some may not ship a highlights query or may lag tree-sitter's ABI, and SQL has several competing grammars. Batch 1 (Ruby, Lua, PHP, C#, Scala, XML) is a separate card and lands first.

## Change

Follow the batch 1 procedure (see that card's spec) for each language: audit, wire up, record and test it.

- Audit each crate's licence, the provenance of each query and compatibility with `tree-sitter` 0.27 before adding it. Never use tree-sitter-language-pack, Zed's highlight queries, or Zed's GPL crates (`CLEAN_ROOM.md`).
- A grammar whose crate ships no usable highlights query gets one translated from nvim-treesitter (Apache-2.0) as `crates/fe-syntax/queries/<lang>.scm`. It carries a provenance header and gets a `THIRD_PARTY.md` row, using T-14a's override mechanism. nvim-only predicates are translated.
- Dockerfile and Make are matched by file name (`Dockerfile`, `Containerfile`, `Makefile`, `GNUmakefile`) as well as extension (`dockerfile`, `mk`). Golden samples are named `<language>.<ext>` so the harness resolves them.
- For SQL, pick the grammar that is permissively licensed, maintained and ABI-compatible, and record why in the decision file.
- A candidate that fails the audit is left out and reported, not substituted.

`Cargo.lock`, `THIRD_PARTY.md` and `deny.toml` are human paths, so a person must approve the merge.

## Open questions (draft card: needs the user's approval)

- Same as batch 1: is the candidate list right, and is about 25 the target? Is this batch wanted at all, or only on demand?
- Which SQL dialect matters (generic, PostgreSQL, SQLite)? It decides which grammar fits.

## Implementation notes

- **Audit (2026-10-03).** All five grammars ship as MIT crates that link
  through `tree-sitter-language` and load in `tree-sitter` 0.27 (parser ABI 15
  for Swift and Dockerfile, 14 for the rest). The SQL and Make crates ship no
  licence file; their repositories hold the MIT text. Kotlin's shipped query
  says it is based on nvim-treesitter (Apache-2.0, allowed); no other query
  names an outside source. None was left out. The Swift crate's tarball also
  contains an unused `node_modules/`; its build script compiles only
  `src/parser.c` and `src/scanner.c`.
- **Kotlin crate.** fwcd's `tree-sitter-kotlin` 0.3.8, the latest on
  crates.io, depends on `tree-sitter` < 0.23, and fwcd's 0.4.0 (which moved to
  `tree-sitter-language`) was never published. `tree-sitter-kotlin-sg` 0.4.1
  is ast-grep's publication of fwcd's repository: one commit on top that
  renames the crate, five commits behind fwcd's main. It is the grammar
  nvim-treesitter uses. `tree-sitter-kotlin-ng` (tree-sitter-grammars, a
  different grammar) was not chosen: it ships no queries, and nvim-treesitter's
  queries target fwcd's node names.
- **Dockerfile crate.** camdencheek's `tree-sitter-dockerfile` 0.2.0, the
  latest on crates.io, depends on `tree-sitter` 0.20. `tree-sitter-containerfile`
  0.9.2 (wharflab) continues that grammar, keeps camdencheek's MIT licence
  next to its own, and is actively maintained. It is young (2026) with few
  users; if that's a concern, Dockerfile can be dropped without touching the
  others. Its injections query is used: `RUN` bodies and heredocs highlight
  as Bash, and `COPY` heredocs by file extension as JSON, YAML, TOML or XML.
- **SQL grammar.** `tree-sitter-sequel` 0.3.11 (DerekStride/tree-sitter-sql):
  MIT, maintained (pushed 2026-09), ABI 14 through `tree-sitter-language`, and
  the grammar nvim-treesitter uses. It parses generic SQL with PostgreSQL,
  MySQL and SQLite constructs, so the dialect question doesn't need answering
  yet. The alternative `tree-sitter-sql` 0.0.2 (m-novikov) needs
  `tree-sitter` 0.19 and ABI 13, so it fails the audit.
- **Overrides.** `queries/kotlin.scm` goes after the crate's query, which is
  an old copy of nvim-treesitter's. It takes five patterns from
  nvim-treesitter's current file (`#lua-match?` becomes `#match?`): function
  names after modifiers or annotations (`private fun f` lost its name), the
  `package` keyword and path, `companion`, and SCREAMING_CASE constants.
  `queries/sql.scm` restates the crate's two number patterns as regexes; the
  crate writes them with the Lua class `%d`, so every number was a string.
  The crate queries' whole-node captures (`(primary_constructor) @constructor`,
  `(package_header) @namespace`, `(jump_expression) @keyword.return`) leave
  whitespace-only spans, as nvim-treesitter's own query does; they colour
  nothing visible.
- **Precedence.** Every query lists general patterns first, so
  `earlier_patterns_win` is false for all five.
- **Aliases.** `variable.member` (Swift's fields) maps to `property`, like
  `field`, and `character` (Kotlin's char literals) to `string`. No existing
  language's golden output changed.
- **File names.** Dockerfile matches `Dockerfile`, `Containerfile` and
  `.dockerfile`; Make matches `Makefile`, `GNUmakefile` and `.mk`.
- **Known gaps, left upstream.** Swift colours free functions as
  `function.method` and doesn't mark `associatedtype` or `case` in
  `for case let`. Make treats a `#` comment after a variable's value as part
  of the value.
