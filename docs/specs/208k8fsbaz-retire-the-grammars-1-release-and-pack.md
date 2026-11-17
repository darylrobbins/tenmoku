---
task_uid: "01m6sq8twmm7wr96208k8fsbaz"
task: "T1486"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Evidence: grammar-registry verify passes on the live index.json and index.json.sig, fetched from registry.toml's index_url; the transcript is in the finish evidence"
    required: true
    manual: false
  - id: "C2"
    text: "gh release view grammars-1 --repo darylrobbins/tenmoku fails, and git ls-remote --tags origin grammars-1 prints nothing"
    required: true
    manual: false
  - id: "C3"
    text: "rg -n 'pack\\.toml|record-grammar-hashes|grammars-1' --glob '!docs/decisions/**' --glob '!docs/specs/**' prints only lines the decision file lists as kept on purpose"
    required: true
    manual: false
  - id: "C4"
    text: "docs/release/language-pack.md covers the per-pack and all-languages tarballs on the registry's index release, with verify and install steps; README.md's Features links https://github.com/tenmoku-editor/registry and no longer names the eight as downloads"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fe-registry -p fastedit passes; cargo clippy -p fastedit --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Retire the grammars-1 release and pack.toml's recorded hashes, and document installing languages and offline packs from tenmoku-editor/registry

# Retire grammars-1 and pack.toml's hashes; document the registry

The registry now publishes (card 2) and the app installs from its signed index
(cards 3 and 4). The old path's leftovers go, and the docs move over. This card
changes nothing in the app's behaviour.

## Check the published registry first

Run `cargo run -p fe-registry --bin grammar-registry -- verify` against the live
`index.json` and `index.json.sig`, downloaded from `index_url` in
`extensions/grammars/registry.toml` with `curl -fsSL`. Quote the output in the
finish evidence. If nothing is published yet, or it doesn't verify, block the
card with what you saw. Don't retire anything before this passes.

## Retire grammars-1 (user-approved 2026-11-06)

The `grammars-1` release in darylrobbins/tenmoku (tag at 16f85127) has no
assets. Delete the release and its tag:
`gh release delete grammars-1 --repo darylrobbins/tenmoku --cleanup-tag --yes`,
then check with `gh release view grammars-1` (fails) and `git ls-remote --tags
origin grammars-1` (empty). Older app builds point their `pack.toml` there, but
with empty hashes they refuse every download already, so nothing breaks. If
`gh` lacks permission, block the card and say so. The lead or user does it.

## Remove what's unused

Run `rg -n 'pack\.toml|record-grammar-hashes|grammars-1'` (outside
`docs/decisions/**` and `docs/specs/**`, which are history). For each hit,
remove the code or file if nothing but its own tests uses it any more:
`extensions/grammars/pack.toml`, `scripts/record-grammar-hashes.sh`,
`crates/fastedit/tests/grammar_pack_manifest.rs`, `grammar_install.rs`'s `Pack`,
`PACK_TOML` and `install_from_pack_dir`, and T1433's compiled-in table in
`crates/fastedit/build.rs` (with its tests) if card 4 left it test-only. Keep
`stage_extension`, `publish` and the fetch path. If something is still live,
keep it and say why in the decision file.

## Docs

- `docs/release/language-pack.md`: the offline packs are now
  `Tenmoku-language-pack-<pack>.tar.gz` and the all-languages
  `Tenmoku-language-pack.tar.gz`, on the registry's `index` release
  (`https://github.com/tenmoku-editor/registry/releases/download/index/<file>`).
  Show how to verify a tarball (its `.sha256`, which the signed index also
  records, and `grammar-registry verify` for the index), and how to install. The
  directory table stays. List the packs from `registry.toml` without counting
  languages, and link `docs/release/grammar-registry.md`.
- `README.md` Features: replace "Kotlin, C#, Scala, Swift, C++, SQL, PHP and Ruby
  are WebAssembly grammars, downloaded the first time a file of that language
  opens" with a line saying that more languages, grouped in packs (name a few
  packs), install from the Tenmoku registry the first time a file of one
  opens, with a link to https://github.com/tenmoku-editor/registry. Leave the
  built-in list alone.
- `extensions/grammars/README.md`: drop any mention of recording hashes in
  `pack.toml`, and say that publishing happens on merge to main
  (`grammars.yml`).

Decision file `docs/decisions/<lane>-grammar-registry-retire.md`: what was
removed, what was kept and why, and the verify transcript.
