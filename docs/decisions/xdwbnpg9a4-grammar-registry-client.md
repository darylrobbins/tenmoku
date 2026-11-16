# Grammar registry: the app's client for the signed index (T1484)

Card 3 of the grammar registry. Card 1 (`xb65tqark9-grammar-registry-index.md`)
builds the index, card 2 (`yw5ahyq2bj-grammar-registry-publish.md`) signs and
publishes it, and this card has the app fetch, verify and cache it. It's the
data layer only: nothing the user sees changes, and `grammar_install.rs` and
fe-workbench are untouched. Card 4 moves the offers and the install onto it.
The code is `crates/fastedit/src/grammar_registry.rs`, and
`docs/release/grammar-registry.md` is the schema it reads.

## Fetching with no hash

The index can't be hash-pinned: its signature is what vouches for it.
`fe_net::grammar_fetch::fetch_bounded(client, url, max_bytes)` is
`grammar_fetch` without the hash. Both now go through one `read_bounded`,
which follows redirects only to granted hosts (the client refuses any other
before a DNS lookup), refuses a `Content-Length` over the limit before reading,
and stops reading one byte past it. The hash is computed over the finished
body rather than chunk by chunk, so the loop is shared and the bytes held are
the same. The app fetches under `grammar_install::grants()`: `github.com`
and GitHub's two asset hosts, as the doc says. The signature is read to at
most 4 KiB (it's 89 bytes) and the index to at most 4 MiB.

## Verify, then parse

`verify_index(bytes, sig_text, key)` checks the Ed25519 signature over the
exact bytes with `crate::update_signature`, the same file fe-registry
includes by `#[path]`, so there's one implementation of the scheme. Only then
does it parse, with `fe_registry::grammars::Index::parse`, which reads
`schema` before anything else and refuses any other, refuses unknown keys at
every level, and runs `Index::check`. The key is a parameter, so the tests use
throwaway keys. The app's key is `registry.toml`'s `public_key`, compiled in
with `include_str!`.

## The cache

`<state dir>/registry/index.json` and `index.json.sig`, the bytes exactly as
fetched. Every load verifies them again with the compiled-in key, so an index
edited on disk is ignored just like a bad download, and its serial doesn't
count for the rollback rule either.

A commit (`Cache::commit`) runs under an exclusive lock on
`registry/index.json.lock` (`std::fs::File::lock`), because two launches can
refresh at once: on Windows every launch is its own process. Under the lock
it reads the cached serial again and refuses a lower one, so a launch that
read serial 5, fetched 6 and finds 7 committed meanwhile doesn't roll the
cache back. The lock is held only for the commit, never across the network.

The commit copies the cached pair aside as `index.json.kept` and
`index.json.sig.kept`, writes the new pair to temporary files in the same
directory, syncs them, and renames the signature into place, then the index,
then removes the kept pair. A reader that catches the moment between the two
renames, or a launch after a failure or a crash there, sees a main pair that
doesn't verify and loads the kept one instead. Its serial still guards the
rollback rule, and the next commit replaces the main pair and drops it. A
cache whose main pair doesn't verify (missing, edited, or only the kept pair
left) counts as missing for the 24 h freshness check (`Cache::usable_age`),
so the next launch refreshes it.

## Refresh and rollback

A refresh fetches the signature, then the index, verifies them, and refuses
an index whose `serial` is lower than the cached one's (`Rollback`). An old
index stays validly signed forever, so without this, anyone in the path
could replay one and hide newer grammars, or bring back an old one. An equal
serial is accepted and rewritten. That's the same commit, and rewriting it
renews the cache's age.

The publisher uploads `index.json.sig` and then `index.json`, each with
`--clobber`. So a reader can briefly see a new signature with the old index,
or find one of them missing. A signature that doesn't verify, or a fetch that
ends in an HTTP status (a 404 while a file is replaced), waits 5 s and tries
the whole fetch once more. Anything else isn't retried. Being offline isn't
fixed by waiting, and a signed index that doesn't parse or rolls back won't
change in 5 s. Any failure keeps the cache, and is logged at `warn` under the
target `fastedit::grammar_registry`. Nothing is shown to the user: the cached
index, or the compiled-in `pack.toml` until card 4, is still what's used.

## When it runs

`grammar_registry::start` runs once at launch, after the updater, and spawns
the refresh in the background, so it never blocks startup. It refreshes only
when the cache is missing or its `index.json` was written 24 h ago or more.
The gate is a pure `Policy::decide(perf, wait_launch, under_test,
automatic_checks)`, the same shape as `sparkle::Policy`:

- **Off** under `--perf` (measurements stay offline and repeatable), for a
  `--wait` launch and its detached server (a `git commit` editor shouldn't
  reach the network), and under test.
- **Off when `update.automatic_checks` is off.** Fetching the index is the
  app reaching GitHub on its own, the same kind of request as the daily
  update check, and the same setting already covers that. A second setting
  would be another switch someone who turned off automatic checks could
  miss. With it off, nothing is fetched until card 4's explicit install,
  which the user asks for.

## Compatibility

`EngineSupport::running()` is the running app's:

- `api`: `config::EXTENSION_API`, the extension API version
  `config::with_extensions` passes to kiln. It's hoisted to one
  `pub(crate) const` so the two can't drift apart.
- `abi`: `tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION ..=
  tree_sitter::LANGUAGE_VERSION`, from the tree-sitter the app links.
- `captures`: `fe_registry::grammars::CAPTURES`.

An entry is compatible when `engine.tenmoku` has the same major as `api`
and a minor no higher (the rule kiln's manifest loader applies, so anything
offered would also load), `engine.abi` is in range, and `engine.captures` is
no higher. A `tenmoku` that isn't `MAJOR.MINOR` is incompatible.
`Catalog::new` drops incompatible entries from the languages and from every
pack's list, drops a pack left with none, and logs each one once, at
`info`. Everything after that (detection, `packs_of`, `pack_languages`)
sees only what this app can load. Showing an incompatible language as
"needs a newer app" is card 4's call, and would read the uncompiled index.

## Detection

`Catalog::language_for(path, first_line, extensions)` returns a language that
isn't built in or installed (`grammar_install::is_installed`). It tries the
whole file name (exact case), then the extension (ASCII case-insensitive),
then the `#!` interpreter. An extension is matched as a suffix after a dot,
so a compound extension such as Blade's `blade.php` works, and the longest
match wins. Within one step, a tie goes to the lowest language name, and is
logged at `warn`. The generator should never produce one.

A file a registered language already claims, per
`fe_syntax::language_name_for_path` or (for the shebang step)
`fe_syntax::language_name_for_shebang`, is never offered another language.
One consequence: `x.blade.php` is PHP's while PHP is built in, so Blade isn't
detected from it. Card 4 can let a longer registry extension outrank a
built-in's shorter one if it wants to.

T1434 exported only `language_name_for_shebang`, which resolves to
registered languages. A local copy of its `env` rules diverged (review round
1: `env -S FOO="a b" perl`, `env -- FOO=1 perl`, `env --null perl`), so this
card exports the part before the lookup as `fe_syntax::shebang_program`,
which `language_name_for_shebang` now calls too. Detection uses it, with
GNU `env`'s options, clusters, operands, `-S` quoting and escapes, and
assignments. A version suffix (`python3.12`) then matches the bare name.

## Live run

`live_index_verifies` (ignored: it needs the network) fetches the published
`index_url` and its signature through the real `fe_net::Client` under the
app's grants, and verifies them with the committed key:

Run on 2026-11-16 at the lane head, after the first publish (Grammars run
43892965762, 15:23Z):

```text
$ cargo nextest run -p fastedit --run-ignored only live_index_verifies --no-capture
    Starting 1 test across 30 binaries (368 tests skipped)
       START [         ] (1/1) fastedit::bin/tenmoku grammar_registry_tests::live_index_verifies

running 1 test
index serial 1794839167: 69 languages, 69 this app loads, 14 packs
test grammar_registry_tests::live_index_verifies ... ok

        PASS [   0.824s] (1/1) fastedit::bin/tenmoku grammar_registry_tests::live_index_verifies
     Summary [   0.839s] 1 test run: 1 passed, 368 skipped
```

Before that publish, the same test reached GitHub through the real client and
the grants and failed as it should (2026-11-10): `.../index/index.json.sig: the
server answered 404`.
