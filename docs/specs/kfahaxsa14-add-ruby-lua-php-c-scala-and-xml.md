---
task_uid: "01m417z47g67smjxkfahaxsa14"
task: "T19"
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

# Add Ruby, Lua, PHP, C#, Scala and XML grammars

## Problem

The plan targets about 25 languages, and 15 ship (`crates/fe-syntax/Cargo.toml` features, plus tsx). T-14 (draft, P3) lists candidates: Swift, Kotlin, Ruby, Lua, SQL, Dockerfile, XML, PHP, C#, Scala, Make. This card adds the first batch: **Ruby, Lua, PHP, C#, Scala, XML**. These grammars are maintained under the tree-sitter and tree-sitter-grammars orgs and are expected to ship their own highlight queries. The second batch (Swift, Kotlin, SQL, Dockerfile, Make) is a separate card.

## Change

For each language:

1. **Audit before adding.** Check the crate's licence (MIT or another permissive licence that `deny.toml` already allows) and the provenance of every query file it ships, the same way the existing grammars were audited (`THIRD_PARTY.md`, "Tree-sitter grammars and highlight queries"). It must work with `tree-sitter` 0.27 (ABI). Never use tree-sitter-language-pack, Zed's highlight queries, or Zed's GPL `language`/`languages` crates (`CLEAN_ROOM.md`). If a crate fails the audit or ships no usable highlights query, leave that language out and say why in the decision file. Don't substitute another source.
2. **Wire it up.** Add a workspace dependency in `Cargo.toml`, then an optional dependency and a same-named feature (in `default`) in `crates/fe-syntax/Cargo.toml`. Add a `Def` in `crates/fe-syntax/src/language.rs` with extensions and file names (PHP `php`; C# `cs`; Ruby `rb`, `Gemfile`, `Rakefile`; Lua `lua`; Scala `scala`, `sc`; XML `xml`, `svg`, `plist`). Use the crate's own highlights constant, and set `earlier_patterns_win` only if its query is written that way. Map any new capture names onto our vocabulary through `ALIASES` in `crates/fe-syntax/src/highlight.rs` rather than adding names.
3. **Record it.** Add a row per crate (feature, crate, version, upstream, licence, query) to `THIRD_PARTY.md`. A grammar whose query is weak can use T-14a's `crates/fe-syntax/queries/<lang>.scm` override mechanism (nvim-treesitter, Apache-2.0, provenance header plus a `THIRD_PARTY.md` row).
4. **Test it.** Add a case to `each_language_highlights_a_snippet` (`crates/fe-syntax/src/tests.rs`), and an original 50–100 line golden sample `crates/fe-syntax/tests/golden/<lang>.<ext>` with its generated `.golden` file. Both tests assert every language has one.

`Cargo.lock`, `THIRD_PARTY.md` and `deny.toml` are human paths in `regatta.toml`, so a person must approve the merge.

## Open questions (draft card: needs the user's approval)

- Is the candidate list and this batch split right, and is about 25 still the target?
- Is binary size or build time a concern with roughly 11 more C grammars in the default feature set?

## Implementation notes

- **Audit (2026-10-03).** All six crates are MIT (`license = "MIT"`; the Ruby,
  Lua and XML crates ship no licence file, their repositories hold the text),
  link through `tree-sitter-language` and load in `tree-sitter` 0.27 (parser
  ABI 14 for Ruby and XML, 15 for the rest). No shipped query names another
  source; Scala's carries a contributor credit line only. None was left out.
  The Scala crate's tarball also contains an unused `node_modules/`; its
  build script compiles only `src/parser.c` and `src/scanner.c`.
- **Grammars.** PHP uses `LANGUAGE_PHP` (PHP inside HTML, the usual `.php`
  file) rather than `LANGUAGE_PHP_ONLY`; the HTML around the tags stays plain,
  since the crate's injections query doesn't inject it. XML uses
  `LANGUAGE_XML`, not the DTD grammar. Lua and PHP also use their shipped
  injections queries (Lua's `ffi.cdef` strings as C, PHP heredocs by label).
- **Precedence.** Every query lists general patterns first, so
  `earlier_patterns_win` is false for all six.
- **Aliases.** Scala's, Lua's and XML's older nvim-style names map onto our
  vocabulary: `conditional`, `repeat`, `include`, `exception`,
  `storageclass`, `preproc` and `type.qualifier` to `keyword`; `field` to
  `property`; `float` to `number`; `method` to `function.method`;
  `parameter` to `variable.parameter`; `tag.attribute` to `attribute`.
- **Ruby override.** The crate's `((identifier) @function.method (#is-not?
  local))` relies on locals tracking, which we don't do, so every local
  variable was a method. `queries/ruby.scm` goes after the crate's query: a
  plain `(identifier) @variable`, then the crate's own later identifier
  patterns restated (MIT, same crate, no new source). Bare receiver-less
  calls without arguments (`block_given?`) now read as variables, the same
  ambiguity nvim-treesitter accepts.
- **Size.** In the debug build the C# and Scala parser archives are the
  largest (about 5 MB and 4 MB), Ruby and PHP about 2 MB each, Lua and XML
  small. Release binary size wasn't measured; that open question stands.
