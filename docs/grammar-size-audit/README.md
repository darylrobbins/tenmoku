# Tree-sitter grammar size audit (T1287)

The stripped release binary is about 89 MB and the 27 built-in tree-sitter
grammars are about 35 MB of it (`docs/specs/ffg5hm7557-strip-symbols-from-release-builds-strip.md`).
This audit measures each grammar, picks a day-one built-in set and proposes
loading the rest as WebAssembly grammars. It changes no manifest, feature or
workflow; the proposal is follow-up cards.

## Per-grammar size

Measured 2026-11-01 at commit `10d070147` (base `19eee3b29`, which added the gitignore grammar) (debug build of `-p fe-syntax`,
default features) with `docs/grammar-size-audit/grammar-sizes.sh debug`.
The script reads an existing build-dir (`cargo metadata`'s `build_directory`,
else `target_directory`) and never builds. For each `tree-sitter-*` crate it
sums `__TEXT` and `__DATA*` sections of the crate's static library
(`size -m`, debug info excluded; parse tables are constant data) and adds the
bytes of the `.scm` files fe-syntax `include_str!`s for that language
(`queries/`, outline/definitions/indents/imports/injections). Sizes in bytes:

```
grammar             code+data      queries        total
kotlin-sg             5803994         5450      5809444
c-sharp               5338040         2751      5340791
scala                 3972873         7517      3980390
swift                 3769342         4343      3773685
cpp                   3442453         6208      3448661
typescript            2909127         2796      2911923
sequel                2503171         2115      2505286
php                   2128718         4660      2133378
ruby                  2116929         4951      2121880
bash                  1385323         1106      1386429
rust                  1132854         6291      1139145
md                     791841         1177       793018
c                      632447         6179       638626
python                 478758         5353       484111
javascript             436133         7913       444046
java                   432655         4856       437511
go                     230043         3030       233073
yaml                   198865          653       199518
make                   180296         1228       181524
css                    139873         1347       141220
xml                    117541            0       117541
containerfile          115953          336       116289
lua                     61750         6288        68038
toml-ng                 37990        11765        49755
html                    27862            0        27862
gitignore               15441            0        15441
json                     7795          778         8573
TOTAL                38408067        99091     38507158

TOTAL 36.6 MB code+data, 0.09 MB queries (1 MB = 1048576 bytes)
```

Caveats:

- `typescript` is the TypeScript + TSX crate (two grammars); `md` is block +
  inline Markdown; `kotlin-sg`, `sequel`, `toml-ng`, `containerfile` are the
  crates behind Kotlin, SQL, TOML, Dockerfile.
- Upstream highlight/injection queries that the crates themselves embed
  (`HIGHLIGHTS_QUERY` and friends) live in the Rust rlibs, not the static
  libraries, so they are not in the table. They are tens of KB per language,
  small against the parse tables.
- A debug build compiles the C at `-O0`, so `__text` is larger than in release;
  the parse tables (`__const`, the bulk of every row) do not depend on
  optimisation. The static-library sum is therefore an upper bound of each
  grammar's contribution, and release linking drops unreferenced sections
  (`-dead_strip`), so a real release difference should come out somewhat lower.
  The spec's earlier estimate (~35 MB in release) vs. 36.6 MB here is consistent
  with that. The exact gap is unmeasured until the cross-check below runs.
- Eight grammars (Kotlin, C#, Scala, Swift, C++, SQL, PHP, Ruby) are 27.8 MB,
  three quarters of the total.

## Release cross-check

Not run: awaiting the user's go-ahead. No `cargo build --release` was started,
and no approval was given or quoted. Proposed once approved (a release build of
fastedit is slow and writes tens of GB to a lane target dir; disk is 384 GB
free on this Mac): 

Every crate depends on `fe-syntax` through `[workspace.dependencies]` with
default features, so Cargo unifies them and a `--config` override cannot
reduce the set. Build B therefore needs a temporary edit of the `default`
list in `crates/fe-syntax/Cargo.toml` (never committed; restore with
`git checkout crates/fe-syntax/Cargo.toml`): remove `"cpp"`, `"ruby"`,
`"php"`, `"csharp"`, `"scala"`, `"swift"`, `"kotlin"`, `"sql"`, leaving the 19
retained features (rust, python, javascript, typescript, json, toml, go, c,
bash, html, css, yaml, markdown, java, lua, xml, dockerfile, make, gitignore).

```zsh
export PATH=$HOME/.cargo/bin:$PATH
# Each build gets its own target dir AND build dir (~/.cargo/config.toml sets a
# shared build.build-dir, which would otherwise keep the intermediates).
for v in a b; do mkdir -p /Volumes/Builds/t1287-$v; done

# A: default features
CARGO_BUILD_BUILD_DIR=/Volumes/Builds/t1287-a/build \
  cargo build --release -p fastedit --target-dir /Volumes/Builds/t1287-a/target

# B: after the temporary manifest edit above. Verify the resolved features first:
#    all 8 removed names must be absent, all 19 present.
cargo tree -p fastedit -e features -i fe-syntax | rg 'fe-syntax feature'
CARGO_BUILD_BUILD_DIR=/Volumes/Builds/t1287-b/build \
  cargo build --release -p fastedit --target-dir /Volumes/Builds/t1287-b/target

git checkout crates/fe-syntax/Cargo.toml
strip -x /Volumes/Builds/t1287-a/target/release/fastedit \
         /Volumes/Builds/t1287-b/target/release/fastedit   # then compare ls -l
rm -rf /Volumes/Builds/t1287-a /Volumes/Builds/t1287-b      # targets and build dirs; the shared build dir is untouched
```

Expected: the stripped difference is about 25–28 MB; disk use about 2 × 20–40 GB
until the last line runs.

## Day-one languages

Sources (checked 2026-11-01):

- [Stack Overflow Developer Survey 2025, Technology](https://survey.stackoverflow.co/2025/technology/):
  JavaScript is the most used language (about two thirds of respondents),
  then HTML/CSS (61.9%), SQL (58.6%) and Python; Bash/shell, TypeScript, Java,
  C#, C++, C, PHP, Go and Rust follow. Docker is used by 71.1% (so Dockerfile
  matters). The secondary summaries I could reach disagree on a few figures;
  take the order, not the decimals.
- [GitHub Octoverse 2025](https://github.blog/news-insights/octoverse/octoverse-a-new-developer-joins-github-every-second-as-ai-leads-typescript-to-1/):
  TypeScript passed Python and JavaScript as the most used language (August
  2025); nearly 80% of new repositories use Python, JavaScript, TypeScript,
  Java, C++ or C#. Secondary coverage lists the top ten as TypeScript, Python,
  JavaScript, Java, C#, PHP, Shell, C++, HCL, Go (not confirmed against the
  original chart).

Tenmoku's own needs, whatever the surveys say:

- Markdown (preview, chat) and its inline grammar, with fence injections.
- TOML and JSON (settings, Cargo, `package.json`), YAML (CI, Kubernetes).
- HTML, which injects JavaScript and CSS; Markdown fences inject most
  languages, but an uninstalled one simply stays plain inside the fence.
- Rust: Tenmoku is written in it and a 1.1 MB grammar is the price of dogfooding.
- Shell, Dockerfile, Makefile: tiny and in every repository.

## Proposed built-in set

Rust, Python, JavaScript, TypeScript + TSX, Go, C, Java, Bash, HTML, CSS,
JSON, TOML, YAML, Markdown (block + inline), XML, Lua, Dockerfile, Make,
gitignore (added at the base commit, 15 KB)
(19 grammar crates).

| | Code + data | Queries |
|---|---|---|
| Proposed built-in (19) | 8.96 MB (9 393 643 B incl. queries) | included |
| Moved to on-demand (8: Kotlin, C#, Scala, Swift, C++, SQL, PHP, Ruby) | 27.76 MB | included |
| Today (27) | 36.7 MB | |

Saving: 27.8 MB of the static-library sum, i.e. about 76% of the grammar
share and roughly 30% of the 89 MB binary (89 → about 61–64 MB after
accounting for the debug-build upper bound). Lua (0.07 MB), Dockerfile,
Make, XML and Go are cheap enough to keep for completeness; Bash (1.4 MB) is
kept because shell scripts are everywhere.

Judgement calls the user may reverse: C++ (3.4 MB) and C# (5.3 MB) are top-ten
languages on GitHub and would be on-demand under this proposal; SQL (2.4 MB) is
58.6% in the Stack Overflow survey. Keeping all three costs 11.1 MB. Cheapest
to move are Kotlin, C#, Scala and Swift (18.4 MB together) whatever else is
decided.

## On-demand path

Built on what exists, nothing new in the engine:

- `fe_syntax::register_language(&GrammarSpec)` (`crates/fe-syntax/src/runtime.rs`)
  loads a `tree-sitter-<name>.wasm` plus highlight/injection queries and
  registers it after the built-ins; extensions declare these as
  `[[contributes.grammars]]` (`crates/fastedit/src/config.rs`,
  `ExtensionGrammar`) and load on a background thread, with epoch deadlines
  and size caps (T845). A built-in with the same name or extension is a
  `Clash` error, so a language is either built in or an extension, never both:
  the built-in feature must be off for the extension to register.
- **Source of grammars:** one first-party extension per moved language (or a
  small pack), built by CI from the same grammar sources and versions that
  `Cargo.lock` pins, with `tree-sitter build --wasm`, carrying the same
  highlights, injections, and (for C++) the C highlights it extends. Outline,
  definitions and indent queries are compiled into fe-syntax per language
  today (`include_str!` in `outline.rs`, `indent.rs`); for moved languages those
  must either stay built in (about 100 KB total across all languages, cheap) or
  become extension contributions in a later step. Start by keeping them.
- **Delivery:** (a) a separate "Tenmoku language pack" download bundling all
  eight, installed beside the app, for offline users; and (b) fetch on first
  open with consent, from the release assets of the same version, verified by
  hash recorded in the app. Recommended: (b) with (a) as the offline fallback.
- **What the user sees opening such a file:** it opens as plain text at once,
  never blocked, with a quiet banner in the editor: "Syntax highlighting for
  Kotlin isn't installed. Install (2.8 MB) · Not now · Never for .kt". Install
  fetches, registers, and re-highlights the open tabs. Plain-text editing,
  search and LSP work meanwhile. Nothing downloads without the click.
- **Cost of WebAssembly parsing:** unmeasured. `crates/fe-syntax/benches`
  (`syntax.rs`, `driver.rs`, `flat.rs`, `tree_ratio.rs`) have no wasm
  benchmark. WebAssembly grammars are generally expected to be slower
  than native; the first follow-up measures one moved grammar both ways on the
  same corpus before the set is committed. Initial load also pays wasmtime
  compilation of a multi-MB module (the crate's tests note it takes a while),
  which is why registration runs off the main thread.

## Missing popular formats

Not built in today (no grammar): which to add, and how.

| Format | Add | Why |
|---|---|---|
| Diff/patch, Git commit messages, INI/`.env`/`.properties`, JSONC (already maps to JSON), CSV | built in (tiny, config-like, seen in every repo) | each grammar is well under 100 KB |
| Zig, Elixir, Haskell, Dart, R, Julia, Svelte, Vue, GraphQL, Protobuf, Terraform/HCL (GitHub top-ten), Nix, CMake, LaTeX | on demand | each 0.2–2 MB or more, used by a minority |
| HCL | on demand first, built in if Terraform proves common | top ten on GitHub, but infra-only |

Sizes of these are not measured here; the grammars are not in the build.

## Result of card 4 (T427)

`fe-syntax`'s `default` list is now the day-one set: Rust, Python,
JavaScript, TypeScript, JSON, TOML, Go, C, Bash, HTML, CSS, YAML, Markdown,
Java, Lua, XML, Dockerfile, Make, and the small git/ini/just/diff/json5/csv
formats. Kotlin, C#, Scala, Swift, C++, SQL, PHP and Ruby are no longer in it,
so they load as the first-party wasm extensions; the eight feature definitions
stay, so `--features` and `scripts/check-syntax-features.sh` still build each.
Their outline, definition and indent queries stay compiled in (unused until the
language registers). Tests naming them are gated on their feature, and
`tests/default_built_in_set.rs` pins the list. Binary size was not measured
(the release builds are card 2).

## Proposed follow-up cards

Ordered; all paths are repository-relative.

1. **Measure WebAssembly vs native parse/highlight on one moved grammar**
   (chore, s). Files: `crates/fe-syntax/benches/wasm.rs` (new),
   `crates/fe-syntax/Cargo.toml` (`[[bench]]` entry), `PERF.md` (record the
   numbers). Deps: none. Decides whether the proposed set holds.
2. **Release cross-check** (chore, s). Run the two release builds in the
   section above and record the numbers. Files:
   `docs/grammar-size-audit/README.md`, temporary and uncommitted
   `crates/fe-syntax/Cargo.toml`. Deps: the user's go-ahead.
3. **Build first-party grammar wasm in CI** (feat, m). Files, per moved
   language (extension manifest names follow `crates/fastedit/tests/extension_grammars.rs`):
   - `extensions/grammars/kotlin/extension.toml`, `extensions/grammars/kotlin/grammar-source.toml` (pins `tree-sitter-kotlin-sg` 0.4 in the root `Cargo.toml` `[workspace.dependencies]`), `extensions/grammars/kotlin/highlights.scm`, `extensions/grammars/kotlin/injections.scm`, `extensions/grammars/kotlin/LICENSE`; the built `extensions/grammars/kotlin/tree-sitter-kotlin.wasm` is a CI asset, not committed.
   - `extensions/grammars/csharp/extension.toml`, `extensions/grammars/csharp/grammar-source.toml` (pins `tree-sitter-c-sharp` 0.23, whose export `tree_sitter_c_sharp` fixes the wasm file name `tree-sitter-c-sharp.wasm`, which `extension.toml` must reference in the root `Cargo.toml` `[workspace.dependencies]`), `extensions/grammars/csharp/highlights.scm`, `extensions/grammars/csharp/injections.scm`, `extensions/grammars/csharp/LICENSE`; the built `extensions/grammars/csharp/tree-sitter-c-sharp.wasm` is a CI asset, not committed.
   - `extensions/grammars/scala/extension.toml`, `extensions/grammars/scala/grammar-source.toml` (pins `tree-sitter-scala` 0.26 in the root `Cargo.toml` `[workspace.dependencies]`), `extensions/grammars/scala/highlights.scm`, `extensions/grammars/scala/injections.scm`, `extensions/grammars/scala/LICENSE`; the built `extensions/grammars/scala/tree-sitter-scala.wasm` is a CI asset, not committed.
   - `extensions/grammars/swift/extension.toml`, `extensions/grammars/swift/grammar-source.toml` (pins `tree-sitter-swift` 0.7 in the root `Cargo.toml` `[workspace.dependencies]`), `extensions/grammars/swift/highlights.scm`, `extensions/grammars/swift/injections.scm`, `extensions/grammars/swift/LICENSE`; the built `extensions/grammars/swift/tree-sitter-swift.wasm` is a CI asset, not committed.
   - `extensions/grammars/cpp/extension.toml`, `extensions/grammars/cpp/grammar-source.toml` (pins `tree-sitter-cpp` 0.23 in the root `Cargo.toml` `[workspace.dependencies]`), `extensions/grammars/cpp/highlights.scm`, `extensions/grammars/cpp/injections.scm`, `extensions/grammars/cpp/LICENSE`; the built `extensions/grammars/cpp/tree-sitter-cpp.wasm` is a CI asset, not committed.
   - `extensions/grammars/sql/extension.toml`, `extensions/grammars/sql/grammar-source.toml` (pins `tree-sitter-sequel` 0.3 in the root `Cargo.toml` `[workspace.dependencies]`), `extensions/grammars/sql/highlights.scm`, `extensions/grammars/sql/injections.scm`, `extensions/grammars/sql/LICENSE`; the built `extensions/grammars/sql/tree-sitter-sql.wasm` is a CI asset, not committed.
   - `extensions/grammars/php/extension.toml`, `extensions/grammars/php/grammar-source.toml` (pins `tree-sitter-php` 0.24 in the root `Cargo.toml` `[workspace.dependencies]`), `extensions/grammars/php/highlights.scm`, `extensions/grammars/php/injections.scm`, `extensions/grammars/php/LICENSE`; the built `extensions/grammars/php/tree-sitter-php.wasm` is a CI asset, not committed.
   - `extensions/grammars/ruby/extension.toml`, `extensions/grammars/ruby/grammar-source.toml` (pins `tree-sitter-ruby` 0.23 in the root `Cargo.toml` `[workspace.dependencies]`), `extensions/grammars/ruby/highlights.scm`, `extensions/grammars/ruby/injections.scm`, `extensions/grammars/ruby/LICENSE`; the built `extensions/grammars/ruby/tree-sitter-ruby.wasm` is a CI asset, not committed.
   - Shared: `.github/workflows/grammars.yml` (new: `tree-sitter build --wasm`,
     upload as release assets), `scripts/build-grammar-wasm.sh` (new),
     `THIRD_PARTY.md`.
   - Golden checks that the wasm grammar highlights as the native one does:
     `crates/fe-syntax/tests/wasm_parity.rs` (new), reusing the existing samples
     `crates/fe-syntax/tests/golden/kotlin.kt`, `crates/fe-syntax/tests/golden/kotlin.kt.golden`, `crates/fe-syntax/tests/golden/csharp.cs`, `crates/fe-syntax/tests/golden/csharp.cs.golden`, `crates/fe-syntax/tests/golden/scala.scala`, `crates/fe-syntax/tests/golden/scala.scala.golden`, `crates/fe-syntax/tests/golden/swift.swift`, `crates/fe-syntax/tests/golden/swift.swift.golden`, `crates/fe-syntax/tests/golden/cpp.cpp`, `crates/fe-syntax/tests/golden/cpp.cpp.golden`, `crates/fe-syntax/tests/golden/sql.sql`, `crates/fe-syntax/tests/golden/sql.sql.golden`, `crates/fe-syntax/tests/golden/php.php`, `crates/fe-syntax/tests/golden/php.php.golden`, `crates/fe-syntax/tests/golden/ruby.rb`, `crates/fe-syntax/tests/golden/ruby.rb.golden`.
   Deps: none.
4. **Make fastedit's default features the day-one set** (feat, s). Files:
   - `crates/fe-syntax/Cargo.toml`: delete the `"cpp"`, `"ruby"`, `"php"`,
     `"csharp"`, `"scala"`, `"swift"`, `"kotlin"`, `"sql"` lines of `default`
     (the eight feature definition lines stay).
   - `scripts/check-syntax-features.sh` (logic unchanged; run to confirm).
   - `crates/fe-syntax/src/language.rs`: a comment on each of the eight
     `Definition`s that the language is an extension by default.
   - Tests that name a moved language and must be gated on its feature (most
     already carry a `cfg(feature)` gate; check each):
     `crates/fe-syntax/src/tests.rs`, `crates/fe-syntax/src/tests/golden.rs`
     (skip the eight samples listed in card 3 when their feature is off), `crates/fe-syntax/src/tests/kotlin_sql_overrides.rs`,
     `crates/fe-syntax/src/tests/injected_clears.rs`,
     `crates/fe-syntax/tests/more_languages.rs`,
     `crates/fe-syntax/tests/more_languages_followups.rs`,
     `crates/fe-syntax/tests/more_languages_followups_2.rs`,
     `crates/fe-syntax/tests/symbols.rs`,
     `crates/fe-syntax/tests/outline_detail.rs`,
     `crates/fe-syntax/tests/outline_detail_suffix.rs`,
     `crates/fe-syntax/tests/outline_scripts.rs`,
     `crates/fe-syntax/tests/definition_objects.rs`,
     `crates/fe-syntax/tests/indent.rs`, `crates/fe-syntax/tests/brackets.rs`,
     `crates/fe-syntax/tests/auto_pairs.rs`,
     `crates/fe-syntax/tests/backtick_auto_pairs.rs`,
     `crates/fe-syntax/tests/comment_tokens.rs`,
     `crates/fe-syntax/tests/language_file_names.rs`,
     `crates/fe-syntax/tests/shebang_languages.rs`,
     `crates/fe-syntax/tests/parse_limit.rs`,
     `crates/fe-syntax/tests/fence_language_aliases.rs`,
     `crates/fe-syntax/tests/fence_language_vscode_names.rs`.
   - `docs/grammar-size-audit/README.md` (record the result).
   Deps: 3 (otherwise moved languages lose highlighting).
5. **"Install syntax highlighting" prompt and fetch with consent**
   (feat, m). Files: `crates/fastedit/src/config.rs` (register fetched
   grammars like extension grammars), `crates/fe-workbench/src/grammar_prompt.rs`
   (new: the banner), `crates/fe-workbench/src/lib.rs` (wire it in),
   `crates/fe-net/src/grammar_fetch.rs` (new: download plus hash check),
   `crates/fe-workbench/tests/grammar_prompt.rs` (new). Deps: 3, 4.
6. **Language pack download for offline installs** (chore, s). Files:
   `.github/workflows/release.yml`, `docs/release/language-pack.md` (new).
   Deps: 3.
7. **Add the tiny built-in formats** (feat, s): diff, INI/`.env`, CSV.
   Layout follows the gitignore grammar (T1282, commit `19eee3b29`). Per format:
   - `vendor/tree-sitter-diff/Cargo.toml`, `vendor/tree-sitter-diff/LICENSE`, `vendor/tree-sitter-diff/README.md`, `vendor/tree-sitter-diff/grammar.js`, `vendor/tree-sitter-diff/bindings/rust/build.rs`, `vendor/tree-sitter-diff/bindings/rust/lib.rs`, `vendor/tree-sitter-diff/queries/highlights.scm`, `vendor/tree-sitter-diff/src/grammar.json`, `vendor/tree-sitter-diff/src/node-types.json`, `vendor/tree-sitter-diff/src/parser.c`, `vendor/tree-sitter-diff/src/tree_sitter/parser.h`, `vendor/tree-sitter-diff/test/corpus/diff.txt`, `crates/fe-syntax/tests/golden/diff.diff`, `crates/fe-syntax/tests/golden/diff.diff.golden` (vendored only if crates.io has no upstream crate; otherwise just the dependency line below and the two golden files).
   - `vendor/tree-sitter-ini/Cargo.toml`, `vendor/tree-sitter-ini/LICENSE`, `vendor/tree-sitter-ini/README.md`, `vendor/tree-sitter-ini/grammar.js`, `vendor/tree-sitter-ini/bindings/rust/build.rs`, `vendor/tree-sitter-ini/bindings/rust/lib.rs`, `vendor/tree-sitter-ini/queries/highlights.scm`, `vendor/tree-sitter-ini/src/grammar.json`, `vendor/tree-sitter-ini/src/node-types.json`, `vendor/tree-sitter-ini/src/parser.c`, `vendor/tree-sitter-ini/src/tree_sitter/parser.h`, `vendor/tree-sitter-ini/test/corpus/ini.txt`, `crates/fe-syntax/tests/golden/ini.ini`, `crates/fe-syntax/tests/golden/ini.ini.golden` (vendored only if crates.io has no upstream crate; otherwise just the dependency line below and the two golden files).
   - `vendor/tree-sitter-csv/Cargo.toml`, `vendor/tree-sitter-csv/LICENSE`, `vendor/tree-sitter-csv/README.md`, `vendor/tree-sitter-csv/grammar.js`, `vendor/tree-sitter-csv/bindings/rust/build.rs`, `vendor/tree-sitter-csv/bindings/rust/lib.rs`, `vendor/tree-sitter-csv/queries/highlights.scm`, `vendor/tree-sitter-csv/src/grammar.json`, `vendor/tree-sitter-csv/src/node-types.json`, `vendor/tree-sitter-csv/src/parser.c`, `vendor/tree-sitter-csv/src/tree_sitter/parser.h`, `vendor/tree-sitter-csv/test/corpus/csv.txt`, `crates/fe-syntax/tests/golden/csv.csv`, `crates/fe-syntax/tests/golden/csv.csv.golden` (vendored only if crates.io has no upstream crate; otherwise just the dependency line below and the two golden files).
   Shared: `Cargo.toml` (`[workspace.dependencies]` lines next to
   `tree-sitter-gitignore`), `crates/fe-syntax/Cargo.toml` (one `default`
   entry, one feature line and one optional dependency per format),
   `crates/fe-syntax/src/language.rs` (one `Definition` each),
   `crates/fe-syntax/src/tests.rs`, `crates/fe-syntax/tests/auto_pairs.rs`,
   `crates/fe-syntax/src/comment.rs`, `crates/fe-syntax/src/fold/tests.rs`
   (the files the gitignore change touched), `scripts/check-syntax-features.sh`
   (no change expected), `THIRD_PARTY.md`, `Cargo.lock`. Deps: none.
