---
task_uid: "01m6wfeyxfk07vmhxb65tqark9"
task: "T325"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-registry's tests/grammar_index.rs passes: entry_lists_every_artifact_file_with_its_hash, digest_and_tag_change_with_any_file, same_inputs_give_identical_bytes, unknown_pack_is_refused, language_without_packs_is_listed_in_no_pack, pack_lists_its_languages_sorted, missing_or_bad_abi_is_refused, grammar_source_and_test_files_are_not_in_the_artifact, index_round_trips_through_its_types, every_first_party_pack_is_defined"
    required: true
    manual: false
  - id: "C2"
    text: "fastedit's tests/language_pack_per_pack.rs passes: --pack packs only that pack's languages with their LICENSE files, an unknown pack fails naming it, no --pack packs every language"
    required: true
    manual: false
  - id: "C3"
    text: "Evidence (Linux determinism): scripts/build-grammar-wasm.sh then scripts/build-grammar-registry.sh, run twice on this lane, write target/registry/ with one artifact directory per language, index.json (schema 1; the eight with packs, engine {tenmoku, abi, captures} and hashed files) and the per-pack and all-languages tarballs, byte-identical across the two runs (the shasum transcript is in the decision file, and the same_inputs_give_identical_tarballs test passes). The macOS BSD-tar two-run is a follow-up on the Mac, not part of this card."
    required: true
    manual: false
  - id: "C4"
    text: "The eight extension.toml carry the spec's packs; grammar_install_tests::every_first_party_manifest_parses passes with packs in ExtensionGrammar"
    required: true
    manual: false
  - id: "C5"
    text: "git diff main --stat touches no .github/workflows/**, extensions/grammars/pack.toml or crates/fastedit/src/grammar_install.rs; cargo nextest run -p fe-registry -p fastedit passes; cargo clippy -p fe-registry -p fastedit --all-targets -- -D warnings is clean; cargo deny check bans passes (no crate new to Cargo.lock)"
    required: true
    manual: false
---

# Emit one hashed artifact per first-party grammar, a generated unsigned index.json and per-pack offline tarballs, with a packs key in extension.toml (new fe-registry crate) (Mac T1481)

# Per-language grammar artifacts, a generated index and pack metadata

User decisions (2026-11-06): grammar **source** stays in `extensions/grammars/<language>/`.
Built artifacts and a signed index go to a new public repo `tenmoku-editor/registry`
(the signing and publishing card follows this one). The download unit is **one
language**: its wasm, its queries, its manifest and its licence files, all hashed.
**Packs are metadata only**: named lists of languages in the index. A language
may be in several packs. Each language's `extension.toml` says which. The
offline tarball stays, now one per pack.

This card builds everything locally and signs nothing. It publishes nothing and
doesn't touch `.github/workflows/**`. The app doesn't read the index yet.

Read first: `rgt task show T1433` (discovery; `extensions/grammars/README.md`,
the `build.rs` table, `pack.toml`'s recorded-only entries), `rgt task show T1434`
(the `line_comment`/`block_comment`/`shebangs` keys), `docs/release/language-pack.md`,
`scripts/package-language-pack.sh`, `crates/fastedit/src/grammar_install.rs`,
`docs/decisions/c1pt00h80c-grammar-fetch.md`.

## New crate `crates/fe-registry` (lib + bin `grammar-registry`)

`members = ["crates/*"]` picks it up with no edit to the members list. Append
`fe-registry = { path = "crates/fe-registry" }` to `[workspace.dependencies]`
(root `Cargo.toml` is shared append-only). Dependencies come only from crates
already in Cargo.lock: serde, serde_json, toml, sha2 and thiserror. No GPUI, no
fe-syntax, no network. CI can then build the bin in seconds, not the whole app.
The app (card 3) and the signer (card 2) share these types. It's a new crate, so
its own `src/lib.rs` is in scope. Name it for plugins too: put the grammar types
in a `grammars` module so plugin entries can sit beside them later.

### Index schema v1 (`index.json`)

Emit exactly this shape. Deviate only with the reason in the decision file.

```json
{
  "schema": 1,
  "serial": 1762430400,
  "url_template": "https://github.com/tenmoku-editor/registry/releases/download/{tag}/{file}",
  "packs": {
    "jvm": { "name": "JVM", "description": "Kotlin, Scala, Groovy and friends",
             "languages": ["kotlin", "scala"],
             "tarball": { "name": "Tenmoku-language-pack-jvm.tar.gz", "size": 123, "sha256": "<64 hex>" } }
  },
  "languages": {
    "kotlin": {
      "name": "Kotlin", "version": "0.4.1",
      "tag": "kotlin-<first 16 hex of digest>", "digest": "<64 hex>",
      "extensions": ["kt", "kts"], "file_names": [], "shebangs": [],
      "packs": ["jvm", "mobile"],
      "license": "MIT AND Apache-2.0",
      "engine": { "tenmoku": "1.0", "abi": 14, "captures": 1 },
      "files": [ { "name": "tree-sitter-kotlin.wasm", "size": 1, "sha256": "<64 hex>" },
                 { "name": "extension.toml", "size": 1, "sha256": "<64 hex>" } ]
    }
  }
}
```

- `serial`: passed in by the caller (`--serial N`; card 2 uses HEAD's committer
  time). The app (card 3) refuses an index older than the one it has cached.
- `files`: every file of the language's artifact, sorted by name: the wasm,
  `extension.toml`, `highlights.scm`, `injections.scm`, `LICENSE`, any
  `LICENSE-*` and `NOTICE`. Never `grammar-source.toml` or `test/`. File names
  are plain names, with no `/`, `\` or leading `.`.
- `digest`: SHA-256 over the lines `<name>\t<size>\t<sha256>\n` of `files`, in
  that order. `tag` = `<language>-<digest[..16]>`. A tag is content-addressed,
  so a published language release is never replaced, and an unchanged language
  republishes nothing. Nobody has to bump a version.
- `name`: the manifest's `name` minus a trailing " syntax" (as
  `grammar_install::Pack::catalog` does today). `version`, `license`,
  `extensions`, `file_names` and `shebangs` come from `extension.toml`.
- `engine.tenmoku`: the manifest's `[engine] tenmoku` (the kiln extension API
  version). `engine.abi`: the module's tree-sitter ABI (below). `engine.captures`:
  `fe_registry::grammars::CAPTURES` (= 1). Its doc comment says to bump it only
  when an existing capture name's meaning changes in
  `crates/fe-syntax/src/highlight.rs`. An app that doesn't know a new name just
  doesn't colour it. Card 3 compares all three with what the app loads, so an
  older app never installs a grammar it can't load.
- Output is deterministic: sorted keys and arrays, no timestamps but `serial`,
  and a trailing newline. The same inputs give byte-identical output.
- Make the types `Deserialize` too, with `deny_unknown_fields` and a
  `schema` check, so card 3 parses with the same types.

### ABI

`scripts/build-grammar-wasm.sh` already has each grammar's `parser.c`. Record
its `#define LANGUAGE_VERSION <n>` as `target/grammar-wasm/<wasm>.abi`, one line
holding the number. That is the ABI the module reports. The generator refuses a
language with no `.abi` file or one that doesn't parse. Say in the decision file
how you checked that the number matches the loaded `Language::abi_version()` for
the eight.

### Pack definitions: `extensions/grammars/registry.toml` (new)

```toml
schema = 1
url_template = "https://github.com/tenmoku-editor/registry/releases/download/{tag}/{file}"
index_url = "https://github.com/tenmoku-editor/registry/releases/download/index/index.json"

[packs.shells]
name = "Shells"
description = "..."
```

Define these packs: shells, web, infra, functional, jvm, dotnet, systems,
mobile, data, docs, shaders, hardware, config, scripting. Card 2 adds
`public_key` here. Card 3 compiles this file in, which is why the URLs live
here and not in a script. Membership lives only in each `extension.toml`, never
in this file.

## The `packs` key

- `[[contributes.grammars]]` gains `packs = ["jvm", "mobile"]` (optional, default
  empty). Add the field to `ExtensionGrammar` in `crates/fastedit/src/config.rs`.
  The loader ignores it beyond the schema, so the struct keeps
  `deny_unknown_fields`, and T1433's `every_first_party_manifest_parses` keeps
  passing. It goes in `[[contributes.grammars]]`, not at top level, because kiln
  warns on unknown top-level keys.
- The first pack listed is the language's main pack: card 4 offers that one.
- Give the eight theirs: kotlin `["jvm", "mobile"]`, scala `["jvm", "functional"]`,
  csharp `["dotnet"]`, swift `["mobile"]`, cpp `["systems"]`, sql `["data"]`,
  php `["web"]`, ruby `["web", "scripting"]`.
- The generator refuses a pack name `registry.toml` doesn't define. A language
  with no packs gets a warning, not an error, because the ~46 language cards
  landing beside this one have no `packs` key (their specs forbid new keys). A
  later sweep card gives them packs.
- Document the key in `extensions/grammars/README.md`: what it is, the defined
  packs, and that the first one is the main pack.

## Build outputs

`scripts/build-grammar-registry.sh` (new, bash like its neighbours,
shellcheck-clean). Run it after `scripts/build-grammar-wasm.sh`:

1. `cargo run -p fe-registry --bin grammar-registry -- generate --grammars extensions/grammars --wasm target/grammar-wasm --out target/registry --serial "${REGISTRY_SERIAL:-$(git log -1 --format=%ct)}"`
   writes `target/registry/languages/<language>/<files>` (each language's artifact,
   exactly the files its index entry lists) and `target/registry/index.json`.
2. Per-pack offline tarballs: `scripts/package-language-pack.sh` gains an
   optional `--pack <name>`, which packs only that pack's languages, as listed in
   the generated index (`grammar-registry packs --index <file>` prints them). It
   writes `Tenmoku-language-pack-<pack>.tar.gz` and its `.sha256`. With no
   `--pack` it packs every language into `Tenmoku-language-pack.tar.gz`, as
   today, so an offline user can still get everything. Each tarball unpacks to
   `tenmoku.grammar_<language>/`, holding that language's artifact files
   (licences now included). Keep today's fixed order, owner and times.
3. The generator then fills each pack's `tarball` entry and rewrites
   `index.json`. A `--finalize` step is fine; pick the simplest that keeps the
   output deterministic.

Everything goes under `target/registry/` (already git-ignored; `dist/` is not). The tarballs go to `target/registry/packs/` via `DIST_DIR`.

## Tests (new files only, flat functions)

`crates/fe-registry/tests/grammar_index.rs`, run against fixture directories in
a temp dir (fake wasm bytes and `.abi` files, so the gate needs no wasm build):

- `entry_lists_every_artifact_file_with_its_hash`
- `digest_and_tag_change_with_any_file`
- `same_inputs_give_identical_bytes`
- `unknown_pack_is_refused`
- `language_without_packs_is_listed_in_no_pack`
- `pack_lists_its_languages_sorted`
- `missing_or_bad_abi_is_refused`
- `grammar_source_and_test_files_are_not_in_the_artifact`
- `index_round_trips_through_its_types` (generate, then parse with the
  `Deserialize` types)
- `every_first_party_pack_is_defined`: reads the repository's
  `extensions/grammars/*/extension.toml` and `registry.toml`. It is cheap, and
  it runs in the gate.

`crates/fastedit/tests/language_pack_per_pack.rs`: `package-language-pack.sh
--pack jvm` on a fixture packs only kotlin and scala, with their LICENSE files;
an unknown pack fails with a message naming it; no `--pack` packs every
language.

## Scope notes

- Don't change `extensions/grammars/pack.toml` or `grammar_install.rs`'s
  behaviour. The app keeps working from `pack.toml` until card 4. Card 5 retires
  it.
- No workflow edit. Card 2 runs this script in CI.
- Decision file `docs/decisions/<lane>-grammar-registry-index.md`: the schema,
  why the tags are content-addressed, the ABI source, and the pack list.
