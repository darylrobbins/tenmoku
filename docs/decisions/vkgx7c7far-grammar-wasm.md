# First-party WebAssembly grammars, second half (T1305)

Card 3 of `docs/grammar-size-audit/README.md`, second half: C++, SQL, PHP and
Ruby. It follows `docs/decisions/jd5c0dwjt7-grammar-wasm.md`, except for how the
two workflows share a release (below).

## The four languages

- **No script or workflow line names a language.** Both find them from
  `extensions/grammars/*/grammar-source.toml` and `target/grammar-wasm/*.wasm`, as
  T1304 planned, so adding the directories is what adds them to the build and the
  upload. `every_extension_is_listed` keeps `wasm_parity.rs`'s table complete.
- **SQL** is `tree-sitter-sequel`, whose export `tree_sitter_sql` fixes the file
  name `tree-sitter-sql.wasm`.
- **PHP** is the `php` grammar in `php/src`, as `LANGUAGE_PHP` is natively, not
  `php_only`. It's the one language of the four with an injection query, ours
  (`queries/php-injections.scm`). Its heredoc and HTML injections go to built-in
  languages in the parity test.
- **C++'s `highlights.scm`** is tree-sitter-c's query then tree-sitter-cpp's, as
  the built-in concatenates them. tree-sitter-c's licence is byte for byte
  tree-sitter-cpp's, so one `LICENSE` covers both.
- **Ruby** is the first with `file_names` (Gemfile and so on), so the built-in's
  file selection carries over; `assert_as_built_in` now checks those too.
- **LICENSE sources.** The tree-sitter-cpp, -ruby and -sequel crates don't package
  their licence, so those texts are the upstream repositories' `LICENSE` at the
  tags `v0.23.4`, `v0.23.1` and `v0.3.11`. tree-sitter-php's is the crate's.

## One creator of the release (T1304's finding F1)

T1304's publisher created the release if it had to. That was safe for it, but
`release.yml` does `gh release view`, then `gh release create` with the app's
files, and a create by this workflow in between failed the app's publish, leaving
the release without the zip or checksum.

`release.yml` is now the only creator: `grammars.yml` waits for the release, then
uploads. This changes only `grammars.yml`, and `release.yml` needs no change: with
one creator, nothing can come between its view and its create.

- **The wait** is bounded at 2 hours on a tag push, longer than a release build
  should take. If `release.yml` fails, this job fails too, saying to rerun it once
  the release exists. The grammars aren't published to a release with no app.
- **A manual run** looks once, as `release.yml`'s manual run publishes nothing.
- **The regression check** is in `wasm_parity.rs`, module `publishing`. It runs
  both workflows' real scripts against a fake `gh` whose `create` fails when the
  release exists: the app first, the grammars first, and the grammars between the
  app's view and create.
