# Grammar registry

The first-party WebAssembly grammars are published apart from the app, to the
GitHub releases of the public repository
[`tenmoku-editor/registry`](https://github.com/tenmoku-editor/registry). A
new or changed grammar reaches users with no app release. The source stays in
`extensions/grammars/<language>/`. `crates/fe-registry` builds the index,
signs it and checks it, and `.github/workflows/grammars.yml` publishes it.

This file is the index's schema (version 1) and the publishing contract. The
design records are `docs/decisions/xb65tqark9-grammar-registry-index.md`
(the index and the build) and
`docs/decisions/yw5ahyq2bj-grammar-registry-publish.md` (signing and
publishing).

## URLs

`extensions/grammars/registry.toml` holds the URLs, and the app compiles them
in:

| What | URL |
| --- | --- |
| The index | `index_url`: `https://github.com/tenmoku-editor/registry/releases/download/index/index.json` |
| Its signature | the same URL with `.sig` appended: `…/download/index/index.json.sig` |
| A pack's tarball | `…/download/index/Tenmoku-language-pack-<pack>.tar.gz`, and its `.sha256` |
| Every language in one tarball | `…/download/index/Tenmoku-language-pack.tar.gz`, and its `.sha256` (not in the index) |
| A language's file | `url_template` with `{tag}` and `{file}` replaced: `https://github.com/tenmoku-editor/registry/releases/download/<tag>/<file>` |

A download URL on `github.com` redirects to
`release-assets.githubusercontent.com`. Those are the two hosts the app
already allows, and nothing else is needed.

There are two kinds of release:

- **One per language version**, tagged with the language's `tag` (below). It
  holds exactly the files in the language's `files`. It's never replaced or
  deleted, because an older cached index can still point at it.
- **The fixed release `index`**, whose files are replaced on each publish:
  `index.json`, `index.json.sig`, and the pack tarballs with their `.sha256`
  files.

Every release is created with `--latest=false`, so the repository's "latest
release" means nothing.

## Index schema 1

`index.json` is UTF-8 JSON, pretty-printed, ending in a newline. Map keys are
sorted. A struct's own keys come in the order below. The types are in
`fe_registry::grammars` (`Index`, `Pack`, `Language`, `Engine`, `FileEntry`).
A reader must refuse:

- an index whose `schema` isn't the one it reads. `Index::parse` checks
  `schema` before anything else, so a schema 2 index is refused as such;
- any key not listed here (`deny_unknown_fields`, at every level);
- an entry that doesn't hold together (`Index::check`): a file name that isn't
  plain, a hash that isn't SHA-256 hex, files not sorted by name or listed
  twice, a `digest` or `tag` that isn't its files', or a pack and a language
  that don't list each other.

### Top level

| Field | Type | Meaning |
| --- | --- | --- |
| `schema` | integer | `1` (`fe_registry::grammars::SCHEMA`). |
| `serial` | integer | When the index was built: CI uses the publishing commit's committer time, in seconds since the epoch. An app refuses an index whose serial is lower than the one it has cached, so an old signed index can't be replayed to roll it back. |
| `url_template` | string | A language file's URL, with `{tag}` and `{file}` placeholders. It's `registry.toml`'s `url_template`. |
| `packs` | object | Each pack that at least one language lists, keyed by the pack's name. |
| `languages` | object | Each language, keyed by its directory name under `extensions/grammars/` (for example `kotlin`). |

### `packs.<pack>`

A pack is a named list of languages offered together, with an offline
tarball. The key is lowercase letters, digits and `-`. `registry.toml`
defines the packs, and each language's `extension.toml` says which ones it's
in.

| Field | Type | Meaning |
| --- | --- | --- |
| `name` | string | Shown to users, for example `JVM`. |
| `description` | string | Shown to users. |
| `languages` | array of strings | The keys of the languages in the pack, sorted. Each one lists the pack in its `packs`. |
| `tarball` | file entry | `Tenmoku-language-pack-<pack>.tar.gz` on the `index` release. A published index always has it. The type allows it to be missing so the index can be built before the tarballs exist, and an app treats a missing tarball as "no offline pack". |

### `languages.<language>`

| Field | Type | Meaning |
| --- | --- | --- |
| `name` | string | The manifest's `name` without a trailing " syntax", for example `Kotlin`. |
| `version` | string | The manifest's `version`. It's shown to users and nothing else depends on it: what's installed is decided by `tag`. |
| `tag` | string | The release holding the files, `<language>-<first 16 hex digits of digest>`. |
| `digest` | string | SHA-256 in lowercase hex over the files (see below). |
| `extensions` | array of strings | File extensions without the dot, in the manifest's order. |
| `file_names` | array of strings | Whole file names, in the manifest's order. |
| `shebangs` | array of strings | Interpreter names for `#!` lines, in the manifest's order. |
| `packs` | array of strings | The packs it's in, in the manifest's order. The first is its main pack. |
| `license` | string | The extension's SPDX licence expression. |
| `engine` | object | What an app needs to load it (below). |
| `files` | array of file entries | Every file of the artifact, sorted by name in byte order. |

The artifact is the module (`tree-sitter-<grammar>.wasm`), `extension.toml`,
the highlights and injections queries the manifest names, `LICENSE` (which
must exist), any `LICENSE-*` and any `NOTICE`. It never includes
`grammar-source.toml`, `test/` or anything else.

### `engine`

These are the compatibility fields. An app installs a language only when all
three match what it supports. It shows a language that doesn't match as
needing a newer app, and doesn't install it.

| Field | Type | An app installs the language only if |
| --- | --- | --- |
| `tenmoku` | string | the manifest's `[engine] tenmoku`, the kiln extension API version, is one it implements. |
| `abi` | integer | the module's tree-sitter ABI (`LANGUAGE_VERSION` in the grammar's `parser.c`) is within the range its tree-sitter loads. |
| `captures` | integer | it's the highlight capture names' version it colours with (`fe_registry::grammars::CAPTURES`, now 1). This is bumped only when an existing capture name's meaning changes. |

### File entries

| Field | Type | Meaning |
| --- | --- | --- |
| `name` | string | A plain file name: not empty, no `/` or `\`, no leading `.`. |
| `size` | integer | Bytes. |
| `sha256` | string | SHA-256 of the file, 64 lowercase hex digits. |

An app checks every file it downloads against its `size` and `sha256` before
using it.

## Tags and digests

`digest` is SHA-256, in lowercase hex, over one line per file in `files`
order:

```text
<name>\t<size>\t<sha256>\n
```

`tag` is `<language>-` followed by the first 16 hex digits of `digest`.

So the tag is content-addressed. Any change to any file of the artifact (the
module, a query, the manifest or a licence file) makes a new tag, and so a new
release. An unchanged language keeps its tag, and a publish uploads nothing
for it. A release's files never change under a tag an app has already seen.
For this to work, the build has to be reproducible: the same sources must give
the same bytes. The index card's decision file records how that's checked.

## Signature and the committed key

The scheme is the update feed's (T1427, `crates/fastedit/src/update_signature.rs`,
which `crates/fe-registry` includes by `#[path]` rather than copying):

- the signature is Ed25519 over the exact bytes of `index.json`;
- `index.json.sig` holds the base64 of the 64-byte signature and a newline
  (a reader ignores trailing whitespace);
- the public key is the base64 of its 32 bytes, and a private key is the
  base64 of a 32-byte seed.

The public key is `registry.toml`'s `public_key`, committed so that every
build, including dev builds and tests, can verify. It's also the repository
variable `TENMOKU_REGISTRY_PUBLIC_KEY`. The private key is the secret
`TENMOKU_REGISTRY_PRIVATE_KEY` in the `registry` environment of
`darylrobbins/tenmoku`, and nowhere else. This isn't the update feed's key:
each key signs one thing.

`grammar-registry sign --index FILE [--registry-toml FILE]` reads the private
key from stdin only: an argument or environment variable holding it is
refused. Before it writes `FILE.sig`, it checks that the index parses and
that the key's public half is `registry.toml`'s `public_key`. That way CI
can't publish an index the app would refuse.

## Publishing

`grammars.yml` runs on a push to `main` that touches the grammars, their
scripts, `crates/fe-registry` or the workflow itself, and it can also be run
by hand. One run at a time (`concurrency: grammar-registry`), and a run is
never cancelled partway.

1. **`build`** (`contents: read`, no secrets, because it compiles
   third-party C): `scripts/build-grammar-wasm.sh`, then
   `scripts/build-grammar-registry.sh` with `REGISTRY_SERIAL` set to HEAD's
   committer time. It uploads `target/registry/` as the workflow artifact
   `grammar-registry`.
2. **`publish`** (`needs: build`, only on `refs/heads/main`, environment
   `registry`, which only `main` may deploy to): it builds `grammar-registry`,
   downloads the artifact, and signs the index with the private key piped to
   the binary's stdin. It then mints a token for `tenmoku-editor/registry`
   from the GitHub App (`vars.TENMOKU_REGISTRY_APP_ID`,
   `secrets.TENMOKU_REGISTRY_APP_KEY`, contents write on that one repository)
   and runs `scripts/publish-grammar-registry.sh --dist target/registry`.

`scripts/publish-grammar-registry.sh` publishes in this order and stops at
the first failure:

1. It checks everything before publishing anything. `grammar-registry verify`
   checks the index against its signature, the index's `url_template` must be
   the target repository's, and every file the index names must be in the
   dist directory with that size and SHA-256.
2. **Freshness.** If the `index` release already holds an `index.json`, the
   script downloads it, and that index's `serial` must not be higher than the
   new one's. A rerun of an older workflow run keeps its old commit, and so
   its old serial, so it's refused and the newer index stays. An equal serial
   is a retry of the same commit, and it goes ahead. The published serial
   isn't checked against the signature: only the registry's App can write to
   the release, and after a key rotation the published index is still signed
   with the old key.
3. It checks each language's release `<tag>`. A release that exists must hold
   exactly the entry's file names and sizes, and is skipped. One that doesn't
   match fails the run, naming the language, before anything is created.
   **A draft release fails the run too**, whatever it holds, and so does a
   draft `index` release (below).
4. It creates each missing language release:
   `gh release create <tag> --target main --latest=false --title "<Name>
   syntax <version>"`, with notes giving the grammar source pin from
   `grammar-source.toml` and the commit, and exactly the entry's files.
5. It creates the `index` release if it doesn't exist, then uploads to it with
   `--clobber`, in this order: the pack tarballs and their `.sha256` files,
   then `index.json.sig`, then `index.json`.

`index.json` goes last, so the files an index names are always up before the
index is. Between the last two uploads, a reader can briefly see the new
signature with the old index, or the new index with the old signature. While
`--clobber` replaces a file, that file can briefly be missing. An app that
fails to verify a fresh index, or can't fetch it, retries once and otherwise
keeps its cached index (card 3). A failed check is never a reason to drop
grammars that are already installed.

`--dry-run` runs the checks in step 1 (not the freshness check, which needs
`gh`), then prints every `gh` command it would
run and runs none, treating every release as missing.
`scripts/test-publish-grammar-registry.sh` tests the script against a fake
`gh`.

### When a publish fails

- **A release exists with other files.** The same tag should never get
  different bytes, so find out why before changing anything. If an earlier
  run was cut off partway through creating it, delete that release and its
  tag by hand and rerun.
- **A release is a draft.** `gh release create` with files makes a draft,
  uploads the files, then publishes it, and `gh release view` finds drafts as
  well. A run cut off in between leaves a draft that may hold every file but
  that nobody can download. The script fails and names it rather than taking
  it as published. Check it, delete it (`gh release delete <tag> --repo
  tenmoku-editor/registry`), and rerun.
- **The published index is newer.** A rerun of an older run is refused. Run
  the workflow on the current `main` instead.
- **The `index` release has the new tarballs but the old index.** Rerun the
  workflow: the uploads are idempotent.

## Checking a published index

```sh
base=https://github.com/tenmoku-editor/registry/releases/download/index
curl -fsSLO "$base/index.json"
curl -fsSLO "$base/index.json.sig"
cargo run -q -p fe-registry --bin grammar-registry -- verify --index index.json
```

`verify` checks `index.json.sig` (or `--sig FILE`) against the committed
`extensions/grammars/registry.toml`'s `public_key` (or `--registry-toml
FILE`), then checks that the index parses. It prints
`index.json verifies against …'s public_key`, or fails and exits 1. To check
a language's files, download them from `url_template` and compare them with
`grammar-registry languages --index index.json <language>`, which prints each
file's name, size and SHA-256.

## Key rotation

The app trusts only the key it was built with, so a new key needs an app
release first:

1. On a trusted machine, run `grammar-registry generate-key --out-private
   new.key`. It writes the private key at mode 600, refuses to overwrite a
   file, and prints the public key.
2. Ship an app release that trusts both the old public key and the new one.
   `registry.toml` will need a second key for this (a list, or a `next` key),
   and card 3's verifier must accept either.
3. Once that release is the oldest one in use (an older app would refuse an
   index signed with the new key, and keep its cache), switch to the new key:
   put it in the `registry` environment's `TENMOKU_REGISTRY_PRIVATE_KEY`,
   and its public half in the `TENMOKU_REGISTRY_PUBLIC_KEY` variable and in
   `registry.toml`'s `public_key`, in one push to `main`. That push's
   publish signs with the new key, and `sign` refuses any mix of old and new.
4. A later app release drops the old key. Destroy `new.key` and the old
   private key once they're stored only where they belong.

If the private key leaks, do the same thing urgently. Until the app release
in step 2 is out, the leaked key can sign an index that apps accept, and a
high `serial` would also stop them taking a real index. Step 2's release has
to stop trusting the leaked key, not just add the new one.

## Third-party entries, later

Third-party grammars will work the way Zed's extension registry does.
Authors keep a grammar in their own repository, and a pull request to the
registry adds an index entry for it: its URL, version and hashes, under the
same schema and checks. CI builds and signs the index as it does now. None of
this exists yet. It will sit beside `fe_registry::grammars`.
