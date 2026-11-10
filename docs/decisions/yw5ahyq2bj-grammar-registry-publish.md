# Grammar registry: signing and publishing (T1483)

Card 2 of the grammar registry. Card 1 (`xb65tqark9-grammar-registry-index.md`)
builds `target/registry/` locally. This card signs `index.json` with the update
feed's Ed25519 scheme and publishes the artifacts, the index and the pack
tarballs from CI to `tenmoku-editor/registry`. The app doesn't read the index
until card 3. `docs/release/grammar-registry.md` is the schema and the
publishing contract. This file records the choices.

## What the human card set up

I checked this with `gh` on 2026-11-10:

- The repository variables `TENMOKU_REGISTRY_PUBLIC_KEY`
  (`LCH1AxJ47pBJJzrmMAr44WRCVP0cl0qVNrGc/imuwfg=`) and
  `TENMOKU_REGISTRY_APP_ID`. **A GitHub App, not a fine-grained token**, so
  the publish job mints its token with `actions/create-github-app-token`.
- The environment `registry`, holding the secrets
  `TENMOKU_REGISTRY_PRIVATE_KEY` and `TENMOKU_REGISTRY_APP_KEY`. Its
  deployment branch policy is custom, with one branch: `main`.
- `tenmoku-editor/registry` is public, its default branch `main` has a
  commit (which `--target main` needs), and it has no releases yet.

`registry.toml`'s `public_key` is the variable's value.

## Signing

- `crates/fe-registry/src/lib.rs` includes
  `crates/fastedit/src/update_signature.rs` unchanged, through `#[path]`, as a
  private module under `#[allow(dead_code)]` (the app's copy may grow helpers
  this crate doesn't call). `ring` and `base64` come from the workspace. The
  public API is `fe_registry::signing`: `sign_index`, `verify_index`,
  `sig_path` and `generate_key`. Its errors are its own `signing::Error`,
  because `update_signature`'s types are `pub(crate)`.
- **`Registry::public_key` is an `Option`.** The committed `registry.toml`
  has it, and `sign` and `verify` refuse a registry without one
  (`NoPublicKey`). `generate` doesn't need it, and
  `crates/fastedit/tests/language_pack_per_pack.rs`, which is outside this
  card's scope, writes a fixture `registry.toml` without one. A required field
  would break that test. `the_committed_registry_toml_has_a_public_key` checks
  that the committed key parses as a 32-byte key.
- `sign` also checks that the index parses (`Index::parse`) before it signs,
  then checks its own signature with the registry's key before writing. It
  writes `FILE.sig` to a temporary file and renames it over. A key mismatch
  error names the key's public half (not a secret) so an operator can see
  which key the secret holds.
- **Stdin only.** `sign` parses its arguments in its own function. Any option
  but `--index` and `--registry-toml`, any positional argument, or a
  `--key=…` form is refused with one fixed message that never echoes what was
  given, so a key pasted onto the command line by mistake doesn't end up in a
  log as well. It never reads an environment variable.
  `private_key_is_read_from_stdin_only` checks four argv forms and the
  environment.
- `verify` checks the signature over the exact bytes first, then checks that
  the index parses.

I added three subcommands the spec didn't name:

- `languages --index F [<language>]` and `tarballs --index F` print what the
  publish script needs, one tab-separated line each. The script reads these
  instead of using `jq`, the same way card 1's `packs` works, so the script
  needs nothing beyond `gh`, coreutils and the binary.
- `generate-key --out-private F` calls `update_signature::generate()`, writes
  the file with `create_new` at mode 600, and prints the public half. The
  rotation steps in the docs need it, and so do the throwaway keys in the
  tests and in the evidence below. Making a key no longer needs a build of the
  whole `fastedit` crate for `examples/sign_update_feed.rs`.

## The publish script

`scripts/publish-grammar-registry.sh` follows the spec, with these choices:

- **Everything is checked before anything is published.** First the
  signature, then that the index's `url_template` is
  `https://github.com/<repo>/releases/download/{tag}/{file}` (so a `--repo`
  typo can't publish an index whose URLs point somewhere else), then that the
  published index isn't newer (review round 1), then every local file's size
  and SHA-256 against the index, then each existing release (a draft fails). A release that doesn't match fails the run before any release is
  created, so the registry never gets some of a run's languages and not
  others.
- "Exists" means `gh release view` succeeded. A failure whose message says
  "not found" means the release is missing, and any other failure (network,
  authentication) stops the run.
- It also uploads `Tenmoku-language-pack.tar.gz`, the tarball with every
  language, to the `index` release. It isn't in the index, but card 1 builds
  it for offline users, and the release is where they'll find it.
- `--dry-run` still runs `grammar-registry verify` and the local checks,
  because they aren't `gh` commands. It treats every release as missing, since
  finding out which exist would mean running `gh`.
- `--registry-toml FILE` was added for the throwaway key in C5.
  `GRAMMARS_DIR` and `TENMOKU_COMMIT` exist for the fixture test.
  `LC_ALL=C` makes the glob and sort orders the same in any locale.
- The script runs on bash 3.2 (macOS's `/bin/bash`): it uses no associative
  arrays and no `${var^}`.

The fixture test `scripts/test-publish-grammar-registry.sh` builds a
two-language registry with the real binary (`generate`, `finalize`, and
`sign` with a throwaway key) and publishes it against a fake `gh` that keeps
its releases in a directory. It covers each case in C1, plus a changed local
file, an index meant for another repository, and the release notes.
`crates/fe-registry/tests/publish_script.rs`
(`test_publish_grammar_registry_passes`, Unix only) runs it with
`CARGO_BIN_EXE_grammar-registry`, so the gate runs it whenever fe-registry is
tested. I mutation-checked it once by swapping the sig and index uploads, and
the test failed.

## The workflow

`grammars.yml` follows the spec, with these deviations:

- **The signer runs as a built binary, not through `cargo run`.** A step with
  no secrets runs `cargo build --locked -p fe-registry --bin grammar-registry`,
  and the signing step runs `printenv TENMOKU_REGISTRY_PRIVATE_KEY |
  target/debug/grammar-registry sign …`. The secret is in the environment
  of the step that uses it, so with `cargo run`, cargo would also have it, and
  so would any build script if anything needed rebuilding. The publish step
  gets `GRAMMAR_REGISTRY=target/debug/grammar-registry` for the same reason.
  `GH_TOKEN` never reaches cargo.
- **No `rust-cache` in the publish job.** Any job in the repository can write
  an Actions cache, so a cache restored into the job that holds the secrets
  could have been poisoned. fe-registry has few dependencies (serde, toml,
  sha2, ring, base64), so building it without a cache is cheap. The build job
  keeps `rust-cache`.
- The publish job runs on `ubuntu-latest`, because the binary is plain Rust
  and needs no wasi-sdk. Both checkouts set `persist-credentials: false`.
  Both jobs have `contents: read`. The workflow's own token never writes,
  because only the App token can write, and only to the registry
  repository.
- `actions/create-github-app-token@v3` with `app-id`, which the human card
  stored. v3 marks `app-id` as deprecated in favour of `client-id`. That only
  prints a warning, and switching would need the App's client ID as another
  variable. `permission-contents: write` narrows the token to what releases
  need.
- The wasi-sdk `env` lines are unchanged (`the_script_downloads_the_workflows_wasi_sdk`
  passes). The build job's timeout is 60 minutes, down from 165, since there's
  no longer a 120-minute wait for `release.yml`'s release.

**What the signature attests.** The publish job signs whatever the build job
uploaded. The build job compiles third-party C, so a compromised grammar
build could change the artifact before it's signed. The signature says "this
is what main's CI built", not "this build was clean". This is the spec's
split, and it's still better than one job that does both: a compromised
build can't take the key or the token, so it can't publish outside a run on
`main`, and it can't keep its access. The reproducible build from card 1
lets anyone rebuild a tag and compare it. Modules also run sandboxed in the
app.

`wasm_parity.rs`'s `publishing` module no longer fakes the `release.yml`
race, because that race is gone. `grammars_workflow_has_no_app_release_step`
checks the triggers (push to `main` and `workflow_dispatch`, no tag), and that
nothing in the workflow names `gh release`, `GITHUB_REF_NAME`,
`github.ref_type`, `github.token` or a `contents: write` permission. It also
checks that the publish step names `tenmoku-editor/registry`.
`only_the_publish_job_names_the_secrets` splits the jobs by indentation and
checks four things:

- neither the top level nor `build` names `secrets.`, `TENMOKU_REGISTRY`,
  `environment:` or `GH_TOKEN`;
- `build` has `contents: read`;
- `publish` has `needs: build`, the `main` condition and `environment:
  registry`, and the secrets it names are exactly the two;
- the key is piped with `printenv`, with no `echo` and no `--key`.

`release.yml` is unchanged. It still has its own steps, and the
`language-pack.md` docs and `pack.toml` still describe the `v*` releases.
Card 5 moves them over.

## Review round 1 (codex, jhzyg9)

- **F1, drafts.** `gh release create` with files creates a draft, uploads
  the files, then publishes it, and `gh release view` finds drafts too. A run
  cut off in between left a draft that matched its entry, so the next run
  skipped it and published an index pointing at files nobody could download.
  Now every lookup asks for `isDraft`, and a draft fails the run, naming the
  language, before anything is published. A draft `index` release fails it
  too. I chose failing over publishing the draft automatically: a draft can be
  cut off partway through its uploads, and a person should look at it first.
  The fake `gh` now has a draft state, and the tests cover a draft holding
  every matching file and a draft `index` release.
- **F2, a stale rerun.** A rerun of an older `main` run keeps its commit, so
  it passed the branch condition and replaced a newer index with its own. Now
  the script downloads the published `index.json` before publishing anything
  and refuses when that index's `serial` is higher (`grammar-registry
  serial`, a new subcommand that parses the index first). An equal serial is a
  retry of the same commit, and it goes ahead. The test publishes B (serial
  8), then A (serial 7). A is refused before any create or upload, B's index
  and signature stay byte for byte, and then B's retry succeeds. The
  published serial is read without checking its signature: only the App can
  write to the release, and after a key rotation the published index is still
  signed with the old key, so checking it would block the first publish with
  the new key. The concurrency group stops another run from changing the
  index between this check and the uploads.
- Every `gh` call now reads `/dev/null` for stdin, because the lookups run
  inside a `while read` loop over a here-string.

Each fix was mutation-checked. With the draft case taken as missing, the
draft test failed. With the serial comparison removed, the stale-rerun test
failed.

## Follow-ups and notes

- `scripts/ci-test-affected.sh` maps a changed file to its package, so a
  change to `crates/fastedit/src/update_signature.rs` reruns fastedit's tests
  but not fe-registry's, even though fe-registry includes the file. That's a
  small gap: the file belongs to fastedit, and any API change breaks the
  fe-registry build, which every gate builds.
- The first real publish creates the `index` release and the 62 language
  releases. Later publishes create only the languages whose files changed.

## Evidence (C5)

This run was on this Mac (macOS 27.0, arm64, wasi-sdk 34 arm64-macos) on
2026-11-10, at lane commit `b519ce17e` (committer time 1794290438, which
became the serial), from a clean `target/grammar-wasm/`. `$C5` is a scratch
directory outside the worktree, and `grammar-registry` is
`target/debug/grammar-registry`.

**1. `scripts/build-grammar-wasm.sh`** built 61 modules for the 62
languages (ERB and EJS share one). Its parity test then failed on one
language, liquid, whose golden predates main's T1649 change
(`property "layout"` is now `string.special.key`). That failure is already
at base: this card changes no grammar files and no fe-syntax code. Card 1
saw the same kind of failure for astro, ejs, svelte and vue, which have
since been fixed. The test runs after every module is built, so the registry
build went ahead. Until liquid's golden is updated, **`grammars.yml`'s build
job fails on main, and nothing is published.** That's the right outcome for
a failing parity check, but it means the first real publish waits for that
fix.

**2. `scripts/build-grammar-registry.sh`** (its serial defaults to HEAD's
committer time, which is what CI sets `REGISTRY_SERIAL` to) exited 0. It
wrote 345 files to `target/registry/`: 62 languages and 8 packs in the
index, plus 9 tarballs (8 packs and the one with every language), each with
its `.sha256`. The eight packed languages' tags are **the same as card 1's
Linux x86_64 build**, so this Mac's arm64 wasi-sdk produced the same bytes:

| Language | tag (this Mac) | card 1's Linux run |
|---|---|---|
| kotlin | kotlin-7777d185c16b0777 | kotlin-7777d185c16b0777 |
| scala | scala-edfdcf8c0848825a | scala-edfdcf8c0848825a |
| csharp | csharp-e9148480c1cabd7a | csharp-e9148480c1cabd7a |
| swift | swift-8d8a29a580a573c4 | swift-8d8a29a580a573c4 |
| cpp | cpp-4242143bd2449232 | cpp-4242143bd2449232 |
| sql | sql-dcba292b48d5ce4c | sql-dcba292b48d5ce4c |
| php | php-1d10df8cbe5d4376 | php-1d10df8cbe5d4376 |
| ruby | ruby-c182b2ec54ced7a1 | ruby-c182b2ec54ced7a1 |

**3. Signing with a throwaway key against a temporary `registry.toml`.**
The temporary file is the committed one with the throwaway public key in
place of `public_key`:

```text
$ grammar-registry generate-key --out-private $C5/throwaway.key
OBc32CWFBHk8fxUGgtSkAz1srrae6lavi+vwfn1AEEk=
-rw------- $C5/throwaway.key
$ diff extensions/grammars/registry.toml $C5/registry.toml
23c23
< public_key = "LCH1AxJ47pBJJzrmMAr44WRCVP0cl0qVNrGc/imuwfg="
---
> public_key = "OBc32CWFBHk8fxUGgtSkAz1srrae6lavi+vwfn1AEEk="
$ grammar-registry sign --index target/registry/index.json   # committed registry.toml, throwaway key
grammar-registry: the private key's public half is OBc32CWFBHk8fxUGgtSkAz1srrae6lavi+vwfn1AEEk=, not extensions/grammars/registry.toml's public_key LCH1AxJ47pBJJzrmMAr44WRCVP0cl0qVNrGc/imuwfg=: the app would refuse what it signs, so nothing was written
exit 1
ls: target/registry/index.json.sig: No such file or directory
$ grammar-registry sign --index target/registry/index.json --registry-toml $C5/registry.toml < $C5/throwaway.key
wrote target/registry/index.json.sig
exit 0
$ grammar-registry verify --index target/registry/index.json --registry-toml $C5/registry.toml
target/registry/index.json verifies against $C5/registry.toml's public_key
exit 0
$ grammar-registry verify --index target/registry/index.json   # the committed key
grammar-registry: target/registry/index.json doesn't verify with target/registry/index.json.sig against extensions/grammars/registry.toml's public_key: the signature does not verify against the public key (changed bytes or a different key)
exit 1
```

The first `verify`'s `exit 0` above is from the `sed` it was piped
through, so I reran it without the pipe, and it exited 0 on its own. Signing against the committed
`registry.toml` with the throwaway key was refused before anything was
written. The index signed with the throwaway key doesn't verify against the
committed key.

**4. `scripts/publish-grammar-registry.sh --dist target/registry
--registry-toml $C5/registry.toml --dry-run`**, run with a trap `gh` first on
`PATH` (it logs any call and fails), exited 0, and the trap never ran (it
wrote no log). The script printed 131 lines:

- the `verify` line;
- 63 `gh release view` commands (62 languages and `index`; in the final
  script, `index` comes first, for the freshness check);
- 63 `gh release create` commands (62 languages and `index`);
- 3 `gh release upload index --clobber`: the 18 pack files first, then
  `index.json.sig`, then `index.json`, last.
- `dry run: would publish 62 language releases, if none exist yet, and the
  index to tenmoku-editor/registry`.

After this run I reworded two lines of the script: the final dry-run
message (it had said "published") and the notes' commit line. Then review
round 1 (below) changed the release lookups. I reran the same dry run after
those fixes, at `2ca22fc86` plus the round 1 changes, with the same trap `gh`.
It exited 0, the trap never ran, and it printed 131 lines: 63 views (`index`
first), 63 creates and 3 uploads, with the sig before the index, last. Kotlin's
create command, from that rerun (with the worktree prefix removed), differs
from the first run's only in the commit it names:

```text
gh release create kotlin-7777d185c16b0777 --repo tenmoku-editor/registry --target main --latest=false --title Kotlin\ syntax\ 0.4.1 --notes $'Grammar source: the crate tree-sitter-kotlin-sg 0.4.1.\nBuilt from Tenmoku commit 2ca22fc86f9c0426e23fc8cda69294a7f5e7cc80.' target/registry/languages/kotlin/LICENSE target/registry/languages/kotlin/LICENSE-APACHE target/registry/languages/kotlin/extension.toml target/registry/languages/kotlin/highlights.scm target/registry/languages/kotlin/injections.scm target/registry/languages/kotlin/tree-sitter-kotlin.wasm
```

`scripts/test-publish-grammar-registry.sh` passed on this Mac with
`GRAMMAR_REGISTRY=target/debug/grammar-registry`, and so did
`test_publish_grammar_registry_passes`. `actionlint 1.7.12
.github/workflows/grammars.yml` and `shellcheck` on both scripts were clean.
