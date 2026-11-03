# Fetching a first-party grammar (T425)

Card 5 of `docs/grammar-size-audit/README.md`, first half: the headless fetch and
install. Nothing here is user-visible and nothing downloads on its own: the only
caller will be the grammar prompt's Install action, the next card.

## Installed as an ordinary user extension

The installer writes `<extensions>/tenmoku.grammar_<language>/` with
`extension.toml`, `highlights.scm`, `injections.scm` and the wasm. The extension
loader in `crates/fastedit/src/config.rs` registers it as it does any other, so
there's no new registration path and that file doesn't change. The extensions
picker lists and disables it. The three text files are compiled in from
`extensions/grammars/<language>/`, the files the parity tests check, so only the
wasm travels.

`grammar_install_tests::installs_a_loadable_extension` goes through the real
loader (`with_extensions`, `kiln::init`, a batch as the watcher sends) instead of
calling `extension_grammars` directly. That function is private to `config.rs`,
and going through the loader also proves the part that matters: the installed
directory gets registered.

**Staging can't be seen.** The temporary directory is under the extensions
directory, so the rename stays on one filesystem however that directory is
linked. The loader looks for a manifest directly in each directory there
(`extension_dirs` in kiln-core), and the staged extension is one level further
down, so a half-written one is never loaded. A crash can leave the temporary
directory behind, where it stays ignored.

**Replacing** moves the old directory into the temporary one, renames the new one
into place, and deletes the old with the temporary directory. If the second rename
fails, the old one goes back. If that fails too, the temporary directory stays,
since it holds the only copy, and `InstallError::NotRestored` says where; it stays
ignored by the loader. The extension is briefly absent between the renames, which
the watcher's debounce covers. A symlink at the target is replaced, not followed.

## The hash is recorded in the app

`extensions/grammars/pack.toml` (`base_url`, and each language's file, size and
SHA-256) is compiled in. `fe_net::grammar_fetch` checks the hash as the body
arrives, and the installer checks again whatever its fetcher did, before anything
is written. An empty hash means not released: every fetch and every pack install
of it is refused without a request or a read. Its size is 0 then, since no size is
known, and `grammar_pack_manifest::pack_lists_all_eight` holds the two together.

The size also limits the download once it's recorded, and it's capped at
`fe_syntax::MAX_GRAMMAR_BYTES` because fe-syntax wouldn't load anything larger.

The limit holds at the read, not after it: checking frames as they come would let
one large chunk overshoot by whatever hyper had buffered. So `grammar_fetch` shares
a `ReadLimit` with its connection's `Stream`, lowered as each frame arrives, and no
read takes more than what may still be accepted. hyper reads only once it has handed
over its last frame, so at most `max_bytes + 1` body bytes are ever read. The limit
sits above TLS, because it counts what HTTP sees: TLS reads a record at a time beneath
it. Other `Client::request`s have no limit and read as they did.

## Where the assets live

A release tagged for the grammars, `grammars-1`, rather than the app's version. The
hash has to be in an app built before or with the tag, and grammar bytes change
only on a pin bump, so this avoids a chicken-and-egg release and re-hashing on every
app release. `scripts/record-grammar-hashes.sh` downloads a release's eight files
and rewrites `pack.toml`; the lead runs it after the first grammar release, then
commits. Given a tag, it also moves `base_url` to that release.

Three things found while building this, each needing action before the first
release works:

- **The repository is private.** An anonymous request for
  `github.com/darylrobbins/fastedit/releases/download/...` is a 404 (checked on
  2026-11-03), and the app sends no credentials. Until the assets are somewhere
  public, a recorded hash would give users `Status(404)`. Nothing breaks today,
  because the empty hashes refuse first.
- **`grammars.yml` doesn't build on any tag.** The spec assumed it did, but its
  push trigger is `v*`, which is also `release.yml`'s, the workflow that creates
  an app release. So a `grammars-1` tag needs its release created by hand, then a
  manual run of `grammars.yml` from the tag. T428 changes that workflow and could
  add a trigger for the grammar tags.
- **GitHub's redirect host has changed.** See below.

## Redirects

fe-net's `Client` doesn't follow redirects, and that stays. `grammar_fetch` follows
up to 3 itself, each an ordinary `Client::request`, so the grants decide each hop
before any DNS lookup or socket. A redirect to plain `http` is refused as any such
URL is. A `Location` relative to the current path isn't followed: GitHub sends
none, and refusing is simpler than resolving one.

The spec named `github.com` and `objects.githubusercontent.com`. GitHub's release
assets now redirect to **`release-assets.githubusercontent.com`**: a real public
release downloads with that host granted and is `HostNotAllowed` with only the
spec's two (checked on 2026-11-03). `grammar_install::GRANTED_HOSTS` has all three,
keeping `objects` in case an asset is still served from it. **T426's spec lists the
old two: it should use `grammar_install::grants()`.**

## The installer's shape

Plain functions taking a target directory and, for the download, a fetcher that
returns a future, so tests inject fakes and run on Linux with no network.
`install_first_party` is the app's real path. Being async, cancelling is dropping
the future: everything before the write is the fetch, and the write has no await
in it, so a cancelled install has written nothing. The write blocks briefly, so
callers run it on a background task.

The module carries `#[allow(dead_code)]` until the next card calls it.
