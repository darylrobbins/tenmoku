---
task_uid: "01m6sq8hf0xb4q4xyw5ahyq2bj"
task: "T1483"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/test-publish-grammar-registry.sh passes: an existing matching release is skipped, a mismatching one fails naming the language, a new tag gets exactly its files, the index release uploads come last with the sig before the index, a bad signature publishes nothing, and --dry-run runs no gh command"
    required: true
    manual: false
  - id: "C2"
    text: "fe-registry's tests/signing.rs passes: sign_then_verify_round_trips, a_key_not_matching_registry_toml_is_refused_before_writing, tampered_index_fails_verify, missing_or_malformed_sig_is_an_error_not_a_panic, private_key_is_read_from_stdin_only"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's publishing::grammars_workflow_has_no_app_release_step and publishing::only_the_publish_job_names_the_secrets pass; actionlint .github/workflows/grammars.yml is clean; the build job has no secrets, and the publish job runs only on main in the registry environment"
    required: true
    manual: false
  - id: "C4"
    text: "extensions/grammars/registry.toml's public_key equals the repository variable TENMOKU_REGISTRY_PUBLIC_KEY; fe-registry includes crates/fastedit/src/update_signature.rs by #[path] rather than a second Ed25519 implementation"
    required: true
    manual: false
  - id: "C5"
    text: "Evidence in the decision file: build-grammar-registry.sh, then sign with a throwaway key against a temporary registry.toml, then publish-grammar-registry.sh --dry-run, on this Mac"
    required: true
    manual: false
  - id: "C6"
    text: "docs/release/grammar-registry.md documents every index field, the tag and digest rules, the URLs, the upload order, verifying a published index and key rotation"
    required: true
    manual: false
  - id: "C7"
    text: "Finish raises the expected human risk exception for .github/workflows/**, and nothing else is flagged; cargo nextest run -p fe-registry -p fe-syntax passes"
    required: true
    manual: false
---

# Sign the grammar index with Ed25519 (the update feed scheme) and publish per-language artifacts, the index and the pack tarballs to tenmoku-editor/registry from grammars.yml

# Sign the grammar index and publish to tenmoku-editor/registry

The card before this one builds `target/registry/` locally: per-language
artifacts, `index.json` and per-pack tarballs. This card signs the index with
Ed25519 and publishes everything to the public repo `tenmoku-editor/registry`
from CI, so new grammars reach users with no app release. The human card this
depends on has created the repo, a write token for it and the signing key.

**This card edits `.github/workflows/**`, so finishing it raises a human risk
exception. That is expected: leave it for the user.** Nothing secret goes in
the repo, in argv or in logs.

Read first: the decision file from the index card
(`docs/decisions/*-grammar-registry-index.md`), T1427's
`crates/fastedit/src/update_signature.rs` and
`crates/fastedit/examples/sign_update_feed.rs`, `.github/workflows/grammars.yml`,
and `crates/fe-syntax/tests/wasm_parity.rs`'s `publishing` module.

## Signing (same scheme as the update feed)

- `crates/fe-registry` includes T1427's module unchanged:
  `#[path = "../../fastedit/src/update_signature.rs"] mod update_signature;`
  (`ring` and `base64` are already in Cargo.lock; add them to fe-registry's
  Cargo.toml). The scheme is T1427's: Ed25519 over the exact bytes of
  `index.json`. The signature is base64 of the 64 raw bytes in
  `index.json.sig`, and the public key is base64 of the 32 raw bytes. Don't fork
  the code. If T1427 changed the module's API, follow it.
- `grammar-registry sign --index FILE` reads the base64 private key from **stdin
  only** and writes `FILE.sig`. Before writing, it checks that the key's public
  half equals `registry.toml`'s `public_key`, and refuses otherwise, so CI can
  never publish an index the app would refuse.
- `grammar-registry verify --index FILE [--sig FILE]` checks against
  `registry.toml`'s `public_key`. Card 5 and the docs use it to check what's
  published.
- Add `public_key = "<base64>"` to `extensions/grammars/registry.toml`, using
  the value the human card stored as the repository variable
  `TENMOKU_REGISTRY_PUBLIC_KEY` (`gh variable get TENMOKU_REGISTRY_PUBLIC_KEY -R
  darylrobbins/tenmoku`; ask the lead if you can't read it). A public key isn't
  a secret. It's committed so every build, dev builds and tests included, can
  verify. This is a different key from the update feed's (T1428). The human
  card records why.

Both subcommands take `--registry-toml FILE` (default
`extensions/grammars/registry.toml`), so tests and local dry runs can use a
throwaway key.

Tests go in the new file `crates/fe-registry/tests/signing.rs` (flat functions):
`sign_then_verify_round_trips`,
`a_key_not_matching_registry_toml_is_refused_before_writing`,
`tampered_index_fails_verify`, `missing_or_malformed_sig_is_an_error_not_a_panic`
and `private_key_is_read_from_stdin_only` (an argv key is refused).

## Publishing: `scripts/publish-grammar-registry.sh` (new, bash, shellcheck-clean)

Usage: `--dist target/registry [--repo tenmoku-editor/registry] [--dry-run]`.
It uses `gh` with `GH_TOKEN` from the environment.

1. Verify `index.json` + `.sig` with `grammar-registry verify` first, and stop
   on failure.
2. For each language in the index: if release `<tag>` exists, check that its
   asset names and sizes match the entry, skip it when they do, and fail
   (naming the language) when they don't. Otherwise run `gh release create
   <tag> --repo R --target main --latest=false --title "<Name> syntax
   <version>" --notes "<source pin from grammar-source.toml, tenmoku commit>"`
   with the artifact files. Language releases are never deleted or clobbered:
   older cached indexes still point at them.
3. Then the fixed release `index`: create it once (`--latest=false`), then
   `gh release upload index --clobber` the pack tarballs and their `.sha256`,
   then `index.json.sig`, then `index.json`. Its stable URL is `index_url` in
   `registry.toml`
   (`https://github.com/tenmoku-editor/registry/releases/download/index/index.json`),
   on the same hosts the app already grants (`github.com`,
   `release-assets.githubusercontent.com`). Between two uploads a reader can
   briefly see a new index with an old signature. Card 3 retries once and
   keeps its cache, so say this in the schema doc rather than working around it
   here.
4. `--dry-run` prints every `gh` command and runs none.

`scripts/test-publish-grammar-registry.sh` (new): runs the script against a
fake `gh` on PATH (as `wasm_parity`'s publishing tests fake `gh`) and checks
that an existing matching release is skipped, a mismatching one fails, a new
tag is created with exactly its files, index uploads come last with the sig
before the index, a bad signature publishes nothing, and `--dry-run` runs
nothing.

## Workflow: `.github/workflows/grammars.yml`

Rework it. Grammars no longer ride on app `v*` releases:

- Triggers: `push` to `main` with `paths` (`extensions/grammars/**`, the
  `scripts/build-grammar-*.sh`, `package-language-pack.sh` and
  `publish-grammar-registry.sh` scripts, `crates/fe-registry/**`, this
  workflow), plus `workflow_dispatch`. Drop the `v*` tag trigger and the
  "Attach them to the GitHub Release" step. Set `concurrency: { group:
  grammar-registry, cancel-in-progress: false }`.
- Job `build` (`contents: read`, **no secrets**): it compiles third-party
  grammar C, so it must never see a secret. It runs
  `scripts/build-grammar-wasm.sh` and `scripts/build-grammar-registry.sh` with
  `REGISTRY_SERIAL` = HEAD's committer time, then uploads `target/registry` as
  a workflow artifact.
- Job `publish` (`needs: build`, `if: github.ref == 'refs/heads/main'`,
  `environment: registry`): download the artifact, then
  `cargo run -p fe-registry --bin grammar-registry -- sign` with
  `TENMOKU_REGISTRY_PRIVATE_KEY` piped to stdin (`printenv ... |`, never
  `echo` and never argv). Then run `scripts/publish-grammar-registry.sh` with
  `GH_TOKEN` set to the registry token. The `registry` environment holds the
  secrets and allows only `main`, so a dispatch from another branch can't sign.
  If the human card chose a GitHub App instead of a fine-grained token, mint the
  token with `actions/create-github-app-token` (app id in
  `vars.TENMOKU_REGISTRY_APP_ID`, key in `secrets.TENMOKU_REGISTRY_APP_KEY`,
  `owner: tenmoku-editor`, `repositories: registry`). Otherwise use
  `secrets.TENMOKU_REGISTRY_TOKEN`.
- Keep the wasi-sdk pin and checksum env lines exactly as they are. T1433's gate
  test compares them with the script's.
- `actionlint .github/workflows/grammars.yml` is clean.

`crates/fe-syntax/tests/wasm_parity.rs` `publishing`: the race with
`release.yml` is gone, so replace those tests with checks that grammars.yml
has no `v*` trigger and no step uploading to the app's release, and that the
`publish` job alone names the secrets and the `registry` environment
(`publishing::grammars_workflow_has_no_app_release_step` and
`publishing::only_the_publish_job_names_the_secrets`).

## Docs

`docs/release/grammar-registry.md` (new): the index schema v1, field by field
(this is the schema doc); the tag and digest rules; the URL layout; the
signature and the committed key; the compatibility fields and what an app does
with them; the publish flow and its upload order; how to check a published
index (`grammar-registry verify`); and key rotation (a new key needs an app
release that carries both keys first). Add a short "third-party entries,
later" section: authors keep grammars in their own repos and a PR to the
registry adds an index entry (Zed's model). No code for that here.

Decision file `docs/decisions/<lane>-grammar-registry-publish.md`.

## Not here

Card 5 deletes the `grammars-1` release, `pack.toml`'s hashes and their
script, and moves the docs over (`docs/release/language-pack.md`, README). The
app doesn't read the index until card 3.
