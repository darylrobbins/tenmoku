# First-party WebAssembly grammars

Each directory here is one language the app can install as an extension
(`tenmoku.grammar_<language>`), its grammar compiled to WebAssembly. A
directory is all a language takes: nothing lists the languages.

- `scripts/build-grammar-wasm.sh` builds each directory's module into
  `target/grammar-wasm/`, then runs `crates/fe-syntax/tests/wasm_parity.rs`'s
  `every_extension_highlights_its_sample`.
- `wasm_parity.rs`'s `every_extension_is_well_formed`, in the gate, checks each
  directory (below).
- `crates/fastedit/build.rs` compiles each directory's `extension.toml`,
  `highlights.scm` and `injections.scm` into the app, which offers the language
  for install (`crates/fastedit/src/grammar_install.rs`).
- `scripts/package-language-pack.sh` packs each into the language pack, and
  `scripts/record-grammar-hashes.sh` records each in `pack.toml` after a
  release.

`pack.toml` holds the release's URL and the entries that script recorded. A
language with no entry isn't released yet: it's offered, and every install of
it is refused, until the next grammar release is recorded. Don't add an entry
by hand.

## Adding a language

Make `extensions/grammars/<language>/`, where `<language>` is the name
fe-syntax and injections will call it (lowercase, such as `ocaml`; not a
built-in language's), holding:

| File | What it is |
|---|---|
| `grammar-source.toml` | Where the grammar's source is (below). |
| `extension.toml` | The extension's manifest (below). |
| `highlights.scm` | The highlight query. |
| `injections.scm` | The injection query; comments only if it has none. |
| `LICENSE` | The grammar's licence text. |
| `LICENSE-*`, `NOTICE` | Optional: another licence's text, such as a query's (Kotlin's `LICENSE-APACHE`). |
| `test/sample.<extension>` | A sample file, which should use most of the language. |
| `test/sample.<extension>.golden` | Its highlights, which the script writes. |

Nothing else: no module (`*.wasm`), and no other file or directory. Then
append the language's row to `THIRD_PARTY.md`'s "Grammars shipped as
WebAssembly", giving the source as `grammar-source.toml` does and the licence.
That's all: no script, test, workflow, Rust table or `pack.toml` changes.

One grammar may back several languages: each gets its own directory, with the
same `grammar-source.toml` source (`wasm_parity.rs` requires it) and its own
queries and sample. They share one module, which the build script builds once.
tree-sitter-embedded-template is the example: ERB and EJS are two directories
with `grammar = "embedded_template"`, differing in their injections.

### Injection-only languages

A language may exist only to be injected: tree-sitter-php's `php_only`, PHP
with no `<?php` tag, which Blade injects for `{{ $x }}`. Its directory is a
normal one, with `extensions = []` and no `file_names`, and its sample is
named for what it is (`test/sample.php`). `wasm_parity.rs` allows that only
when another directory's `injections.scm` sets it as `injection.language`
with `#set!` (`(#set! injection.language "php_only")`; the key may be quoted,
and a capture may come anywhere among its arguments), not by a capture's text, which isn't known
until a file is parsed; otherwise nothing would ever install it.

It's never offered on its own. Installing a first-party language, by the
prompt, `install_first_party` or the language pack, also installs each
injection-only first-party language its `injections.scm` names (and theirs in
turn) that's released and not yet installed, under the same consent; the
offer's size counts those still to install. If one fails, the install fails
with its error and writes nothing. The prompt's install finishes once every
one the language needs has registered, so its tabs highlight with them; past
a minute it warns and keeps waiting, and once they've all registered it
highlights the language's open tabs again. An injection-only
language counts as installed by its directory, not by having registered. Uninstalling the language leaves them installed. A language
injected but not installed stays plain, as any other.

### grammar-source.toml

One key per line, each value a plain string (or a one-line list) with
nothing after it on the line: a comment goes on a line of its own. It's in
one of these forms.

A commit of a GitHub repository whose tree has the generated `parser.c`:

```toml
git = "https://github.com/tree-sitter/tree-sitter-ocaml"
# The full 40-hex commit.
rev = "3b2e14e0697d405c9aa0beddfa09b71f45abc504"
# The directory holding parser.c, relative to the repository.
src = "grammars/ocaml/src"
# The export tree_sitter_<grammar>.
grammar = "ocaml"
```

The script fetches exactly that commit, whole, into
`target/grammar-src/<language>-<rev>/` and reuses it after: a scanner may
include a file outside `src/` (OCaml's includes `../../common/scanner.h`).

A crates.io archive, for a grammar whose repository has no generated
`parser.c`:

```toml
registry = "crates.io"
crate = "tree-sitter-zig"
version = "1.1.2"
# The .crate's SHA-256.
sha256 = "ab11fc124851b0db4dd5e55983bbd9631192e93238389dcd44521715e5d53e28"
# Relative to the crate's root.
src = "src"
grammar = "zig"
```

The script downloads `https://static.crates.io/crates/<crate>/<crate>-<version>.crate`
and checks its SHA-256 before it unpacks it. crates.io's API lists each
version's `checksum`, which is this hash; check it against your own download.

The first eight languages have a third form, `crate` and `version` with no
`registry`: the crate `Cargo.lock` pins, whose source cargo has, so their
wasm is the grammar fe-syntax links natively. Use it only for a language
fe-syntax also builds in.

Each form may add `parity_needs = ["sql"]`: other directories' languages
the sample injects. The script builds them too, and the test registers them
under their names, so that the injections resolve (PHP's heredoc `<<<SQL`).

The module is `tree-sitter-<grammar>.wasm`, with `-` for `_`: the loader
finds the export by the file's name. Only `parser.c` and a C `scanner.c` are
compiled; a grammar with a C++ `scanner.cc` isn't supported.

### extension.toml

```toml
id = "tenmoku.grammar_ocaml"
name = "OCaml syntax"
# MAJOR.MINOR.PATCH: the grammar's version, where it has one.
version = "1.0.0"
# The grammar's SPDX licence.
license = "MIT"

[engine]
tenmoku = "1.0"

[[contributes.grammars]]
language = "ocaml"
wasm = "tree-sitter-ocaml.wasm"
extensions = ["ml"]
highlights = "highlights.scm"
injections = "injections.scm"
```

`id` and `language` are the directory's name, and `highlights` and
`injections` are those file names, which the installer writes. The grammar
may also have `file_names` (whole names, such as `Gemfile`),
`earlier_patterns_win` and `license`, as any extension's may
(`crates/fastedit/src/config.rs`'s `ExtensionGrammar`); any other key fails
`grammar_install_tests::every_first_party_manifest_parses`. No extension or
file name may be a built-in language's or another directory's. An extension
may be compound, such as `blade.php` for `x.blade.php`: it's a claim of its
own, and wins over any language's plain `php`.

### The queries

Start from the grammar's own `queries/` or another editor's, and say in a
comment at the top where the file came from, at which commit, and under what
licence (Kotlin's `highlights.scm` is an example). If that licence isn't the
grammar's, add its text as `LICENSE-<name>`.

Every capture must be one of fe-syntax's highlight names, or a more specific
one (`@function.method.call` is `function.method`): see `NAMES` and `ALIASES`
in `crates/fe-syntax/src/highlight.rs`. A capture used only by a predicate
starts with `_` (`@_name`); rename others the editor has no colour for, such
as Neovim's `@spell`, the same way, or drop them.

### The sample and its golden file

```sh
UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh ocaml   # writes test/sample.ml.golden
scripts/build-grammar-wasm.sh ocaml                   # checks it
```

The golden file has one highlight span a line, in the format of
`crates/fe-syntax/src/tests/golden.rs`. Read it before committing: it's what
the language looks like in the editor. The sample must parse with no `ERROR`
or `MISSING` node.

Then the gate's checks:

```sh
cargo nextest run -p fe-syntax --test wasm_parity
cargo nextest run -p fastedit grammar_install
```

### Running the script

- It needs wasi-sdk's clang: `WASI_SDK`, else `~/.cache/tree-sitter/wasi-sdk`,
  where the tree-sitter CLI keeps it. With neither, it downloads the wasi-sdk
  `.github/workflows/grammars.yml` pins, for macOS (arm64 or x86_64) or Linux
  (x86_64 or arm64), and checks its SHA-256.
- `GRAMMAR_JOBS` caps the clang processes at once (default half the cores).
- Naming languages, as arguments or as `GRAMMAR_LANGUAGES=ocaml,zig`, builds
  and checks only those (and their `parity_needs`); with none, every one.
- A language fe-syntax also builds in (the first eight) is checked against the
  built-in's sample, `crates/fe-syntax/tests/golden/<language>.*`, so its wasm
  highlights as the native grammar does; `UPDATE_GOLDEN=1` doesn't touch
  those.
