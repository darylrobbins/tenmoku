# Retire grammars-1 and pack.toml's recorded hashes (T1486)

The registry publishes (card 2) and the app installs from its signed index
(cards 3 and 4), so the old release-and-`pack.toml` route is removed and the
docs point at the registry.

## Verify transcript

`index.json` and `index.json.sig` fetched with `curl -fsSL` from `index_url` in
`extensions/grammars/registry.toml`, on 2026-11-17:

```
$ cargo run -q -p fe-registry --bin grammar-registry -- verify --index index.json --sig index.json.sig --registry-toml extensions/grammars/registry.toml
index.json verifies against extensions/grammars/registry.toml's public_key
```

Exit status 0.

## Removed

- `extensions/grammars/pack.toml`, `scripts/record-grammar-hashes.sh` and
  `crates/fastedit/tests/grammar_pack_manifest.rs`.
- `crates/fastedit/tests/grammar_pack_recorded.rs`, which was outside the
  card's scope: it only read `pack.toml`, so it can't outlive it (self-respec).
- In `grammar_install.rs`: `Pack`, `PackEntry`, `PACK_TOML`, `PackGrammar`,
  `ExtensionFiles`, `install_from_pack_dir`, `install_first_party(_with)`,
  `install_fetched`, `fetch_set`, `fetch_from_release`, the bundle helpers of
  the old route, `OfferHooks`, `offers_with`, `Download`, `Republish`,
  `write_set`, the manifest-parsing `offer_grammar`, and `InstallError`'s
  `Manifest` variant.
- T1433's compiled-in table in `crates/fastedit/build.rs` (`FIRST_PARTY`) and
  `first_party_languages`: card 4 left it test-only. `build.rs` keeps the
  Windows icon.
- The old route's tests in `grammar_install_tests.rs` that only exercised the
  pack: its offers, `with_injected`, fetch and bundle steps.
- Ported, as review round 1 asked, because the code they cover is live:
  rollback of injected extensions when the host fails, serialised installs not
  relying on what another takes back, and a dangling link being replaced (now
  through `write_staged` in `grammar_install_tests.rs`); and highlighting open
  tabs once an injected language registers past the deadline (now through
  `index_offers_with` in `grammar_install_index_tests.rs`).

## Kept

- `stage_extension`, `publish`, `write_staged` and `Staged`, and the index
  route's fetch path (`fetch_file`, `fetch_entry_set`, `write_entry_bundle`,
  `offers`, `await_injected`): live. Their tests now install through
  `write_staged` with a fixture grammar.
- `injected_languages`, `is_wasm_file_name` and `InstallError::UnknownLanguage`
  and `Bundle`: the index route uses them.
- Tests that read `extensions/grammars/*/extension.toml` from disk: every
  directory is complete and its manifest loads.
- The `grammars-1` release's own deletion is done outside the repo with
  `gh release delete grammars-1 --repo darylrobbins/tenmoku --cleanup-tag --yes`.
  Older app builds point at it, but with empty hashes they already refuse every
  download.

No `rg 'pack\.toml|record-grammar-hashes|grammars-1'` hit remains outside
`docs/decisions/**` and `docs/specs/**`, so nothing is kept on purpose.

## Left for the lead

Fluent keys now unused by `grammar_install.rs`: `fastedit-grammar-manifest`,
`-file-not-wasm`, `-manifest-no-grammar` and
`-wasm-not-wasm` (`crates/fastedit/l10n/`, outside the card's scope).
