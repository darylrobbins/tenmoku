# A first-party wasm grammar is one directory (T297, Mac T1433)

After this card, adding a first-party WebAssembly language is adding
`extensions/grammars/<language>/` (and appending its `THIRD_PARTY.md` row). No
script, workflow, test table, Rust table or `pack.toml` lists the languages.
`extensions/grammars/README.md` is the guide the language cards follow.

It follows the card's design. Where it differs, or the card left a choice:

## What lists the languages now

- **wasm_parity.rs**: `every_extension_highlights_its_sample` (ignored, run by
  the script) replaces the eight per-language tests, and
  `every_extension_is_well_formed` (gate) replaces `every_extension_is_listed`,
  `each_extension_holds_its_files_and_no_module` and
  `each_wasm_file_is_named_after_its_export`. The checks are a function over a
  directory of extension directories (`problems`), so the tests that show each
  check failing build a temporary one.
- **The eight's samples aren't a fixed map.** The card allowed one, but C4 asks
  for no per-language list in the file, and the set is already written down:
  a language fe-syntax also builds in (behind a feature) has its sample in
  `crates/fe-syntax/tests/golden/<language>.*`, which `src/tests/golden.rs`
  requires of every built-in. A directory is checked against that sample when
  there is one, and must hold `test/sample.*` when there isn't. A language card
  can't add a native sample without editing outside its directory, so a new
  directory can't pass by leaving its samples out.
- **One process, so each language's registrations are withdrawn after it.** The
  eight tests ran in eight processes; one test runs every language in one. PHP
  registers `sql` under its name for its heredoc, which would otherwise resolve
  a later language's `SQL` injection and change its highlights. Each language's
  check unregisters what it registered (`unregister_language`).
- **Names.** The eight still register as `<language>_wasm`, as before, in case
  their feature is on. A new language registers under its own name, as the app
  registers it, so an injection of itself resolves.
- **`parity_needs` is transitive**, in the script and the test, and must name
  other directories. PHP's `["sql"]` replaces the script's and the test's
  special cases.
- **The load deadline is 120 s in the ignored test.** The full run failed swift
  at the app's 10 s, compiling a 3.8 MB module in a debug test binary while the
  machine was busy; the test is of highlights, not load time.
- **fastedit**: `build.rs` writes `FIRST_PARTY` into `OUT_DIR`, sorted, with
  `rerun-if-changed` on `extensions/grammars` and on each directory;
  `include_str!` tracks the files. The catalog is every compiled-in directory,
  with its `pack.toml` entry if it has one. The tests read the directories:
  `every_first_party_grammar_is_complete` checks the compiled-in list equals
  them (so `build.rs` can't miss one) and that every entry names a directory and
  its wasm.
- **`every_first_party_manifest_parses`** runs kiln's `load_manifest` with the
  grammars kind's real schema and requires no diagnostic at all (an unknown key
  is a warning there), then deserializes `contributes.grammars` as
  `config.rs`'s `ExtensionGrammar` (`deny_unknown_fields`). That needed
  `ExtensionGrammar` to be `pub(crate)`: a one-word change in `config.rs`,
  outside the card's scope.
- **pack.toml** is `base_url` and recorded entries only. A directory with no
  entry is offered with size 0 and no hash and refused before any fetch,
  exactly as an empty-hash entry is (`a_language_with_no_entry_is_not_released`
  runs both). The file name of an unrecorded language is its `extension.toml`'s
  wasm. `pack_holds_its_release_and_recorded_entries` requires an entry to be
  recorded (a hash and a size), so an empty one can't be committed again.
- **record-grammar-hashes.sh** keeps what's before the first entry (the
  comments and `base_url`, moved) and writes an entry for every directory,
  sorted, from each `extension.toml`'s wasm; it still writes nothing if the
  release lacks one. Recording twice gives the same file. grammars-1 had no
  recorded hashes when this landed, so there were none to keep.
- **package-language-pack.sh** needs at least one directory; it already copied
  only `extension.toml`, the two queries and the wasm, so `test/` and
  `grammar-source.toml` stay out (the layout test counts four files).
- **Claims** (review round 1): every directory's extensions and file names are
  checked against the built-in languages, a native directory being allowed only
  its own built-in's (with its feature on); and against the other directories',
  by extension, by name, and a file name with another's extension, ignoring
  case, since the loader gives such a file to whichever registered first.
- **The README's templates are tested**: `the_readmes_templates_are_well_formed`
  makes directories from its TOML blocks as written. Values take no trailing
  comment, in the script's parser and the test's alike.
- **An extension list is required.** `every_extension_is_well_formed` refuses a
  grammar with no `extensions`, as the install offer goes by them and the
  catalog test already required them.

## The script

- **Sources**: the Cargo.lock form as before; `git` + `rev` fetched with `git
  init` + `git fetch --depth 1 <url> <rev>`, checked out, its `HEAD` checked
  against `rev`, kept whole under `target/grammar-src/<language>-<rev>/` and
  reused when complete; `registry = "crates.io"` downloaded from
  static.crates.io, its SHA-256 checked before it's unpacked into
  `target/grammar-src/crates.io/<crate>-<version>/`. A partial fetch is
  never reused: each lands in a temporary directory, moved into place last.
  `src` must stay inside the source. Sources are fetched one at a time before
  any compile.
- **wasi-sdk**: `wasi_sdk_version=34.0` and a SHA-256 per host archive, the
  hashes those GitHub lists for the release's assets (the macOS arm64 one is
  `grammars.yml`'s). `the_script_downloads_the_workflows_wasi_sdk` checks the
  version against `WASI_SDK_VERSION` and the macOS arm64 hash against
  `WASI_SDK_SHA256`. It downloads only into the default directory, and only when
  it doesn't exist: an explicit `WASI_SDK` without a clang, or a default
  directory without one, is an error, not overwritten. It also has macOS x86_64,
  beyond the card's three, as the release has it and it costs a line.
- **Parallel clang**: at most `GRAMMAR_JOBS` (default half the cores, at least
  one), by polling `jobs -pr`, not `wait -n`, which macOS's bash 3.2 lacks.
- **Filtering**: languages named as arguments or in `GRAMMAR_LANGUAGES` are
  passed to the test in `GRAMMAR_LANGUAGES`; the test checks those and fails on a
  name with no directory. A whole build passes it empty.
- `.github/workflows/grammars.yml` is unchanged: it runs the script with
  `WASI_SDK` set, and uploads `target/grammar-wasm/*.wasm`.

## C++'s injections had drifted (T1391)

The first full run failed C++ against `cpp.cpp.golden`: T1391 gave the built-in
C++ an injection query (`queries/cpp-injections.scm`, printf verbs) and
updated the golden file, but not `extensions/grammars/cpp/injections.scm`.
`cpp_extension_is_as_built_in` still expected no injections and didn't fail,
because the `*_extension_is_as_built_in` tests need their language's feature,
which the gate doesn't turn on. The extension now has that query verbatim and
the test expects it; with all eight features on, every `*_as_built_in` test
passes (`cargo nextest run -p fe-syntax --test wasm_parity --features
cpp,csharp,kotlin,scala,swift,sql,php,ruby`). `extensions/grammars/cpp/injections.scm`
was outside the card's scope. Running those tests with the features in the
gate would catch the next drift; that's a follow-up.

## The gate

`scripts/ci-test-affected.sh` maps `extensions/grammars/*` to fe-syntax and
fastedit, added after the reverse-dependency closure: they read those files
(tests, benches, `build.rs`), and their dependents don't, so a language card
runs two packages' tests instead of the workspace (fe-syntax's dependents are
most of it). `scripts/test-ci-test-affected.sh` has the case, and that another
directory under `extensions/` still runs the workspace. That self-test needs a
default rustup toolchain; here it ran with `RUSTUP_TOOLCHAIN=1.98.1`.

## How it was checked

On argon (Linux x86_64), from no wasi-sdk at all: `scripts/build-grammar-wasm.sh`
downloaded wasi-sdk 34.0's x86_64 Linux archive, checked it, built the eight,
and `every_extension_highlights_its_sample` passed for all eight (after the C++
fix) against the unchanged `crates/fe-syntax/tests/golden`. The Mac's run of
C1 is the macOS arm64 path, the same script with the workflow's archive.

The proof of C2 used two throwaway directories, not committed:

- `ocaml`, git: `https://github.com/tree-sitter/tree-sitter-ocaml` at
  `3b2e14e0697d405c9aa0beddfa09b71f45abc504`, `src = "grammars/ocaml/src"`,
  whose scanner includes `../../common/scanner.h`, with upstream's
  `queries/highlights.scm` and a 10-line `test/sample.ml`.
- `zig`, crates.io: `tree-sitter-zig` 1.1.2, sha256
  `ab11fc124851b0db4dd5e55983bbd9631192e93238389dcd44521715e5d53e28` (crates.io's
  checksum, and that of the download), with upstream's `highlights.scm` and a
  `test/sample.zig`. The card's example, tree-sitter-latex, has one crate
  version, 0.1.0, whose `parser.c` needs an external scanner the crate doesn't
  include, so it can't be built from crates.io at all.

With only those two directories added: `every_extension_is_well_formed` said
both lacked golden files, and that zig's `highlights.scm` captures `@spell`;
after renaming it `@_spell`, `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh
ocaml zig` fetched the commit and the crate (checked), built both and wrote the
golden files (68 and 95 lines, no parse errors). A run with OCaml's golden file
stale failed naming its first differing line; rewritten, `scripts/build-grammar-wasm.sh
ocaml zig` passed. fastedit's generated table then had ten languages, and
`grammar_install_tests` (`every_first_party_grammar_is_complete`,
`every_first_party_manifest_parses` among them), `language_pack` and
`grammar_pack_manifest` passed; `scripts/package-language-pack.sh` packed
`tenmoku.grammar_ocaml/` and `tenmoku.grammar_zig/` with their four files each.
The directories, `dist/` and the fetched sources were then removed.

## Follow-ups

- `README.md`'s language list will need the new languages (T1426 claims the
  file, so this card doesn't touch it).
- Run the `*_extension_is_as_built_in` tests with their features in the gate.
- Distribution (per-language artifacts and an index): each language's pieces
  are already per directory, its file set in `wasm_parity.rs`'s `REQUIRED` and
  the four files the installer and the pack copy, its wasm named by its
  `extension.toml`, and its recorded hash its own `pack.toml` entry.
