---
task_uid: "01m6sq8mq4znfnn2xdwbnpg9a4"
task: "T1484"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "grammar_registry_tests passes: good_signature_loads_the_index, tampered_index_is_refused_and_the_cache_is_kept, wrong_key_is_refused, missing_signature_is_refused, tampered_cache_is_ignored, lower_serial_is_refused, unknown_schema_is_refused, signature_race_retries_once, offline_keeps_the_cache, incompatible_entries_are_dropped, detects_by_file_name_extension_and_shebang, installed_built_in_or_claimed_languages_are_not_detected, policy_off_for_perf_wait_tests_and_without_automatic_checks, baked_registry_toml_parses_with_a_32_byte_key"
    required: true
    manual: false
  - id: "C2"
    text: "fe-net's tests/bounded_fetch.rs passes (bounded_fetch_returns_the_body, bounded_fetch_refuses_over_limit_by_header_and_stream, bounded_fetch_refuses_an_ungranted_redirect), and tests/grammar_fetch.rs passes unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Evidence: the ignored grammar_registry_tests::live_index_verifies passes against the published index_url, and the decision file quotes its run"
    required: true
    manual: false
  - id: "C4"
    text: "The index signature is checked with the update_signature code that fe-registry includes, not a second Ed25519 implementation; the ABI range comes from tree_sitter's LANGUAGE_VERSION and MIN_COMPATIBLE_LANGUAGE_VERSION; the extension API version is one const shared with config::with_extensions"
    required: true
    manual: false
  - id: "C5"
    text: "git diff main --stat touches no crates/fe-workbench/** and no grammar_install.rs; cargo nextest run -p fe-net -p fe-registry -p fastedit passes; cargo clippy -p fe-net -p fastedit --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Fetch, verify and cache the signed grammar index in the app: refuse unsigned, tampered or rolled-back indexes, drop entries this engine cannot load, and detect languages that are not installed

# The app fetches, verifies and caches the signed grammar index

Once the registry publishes, the app needs the signed `index.json` to know
which languages it can install, what they hash to and whether it can load
them. Until now that came from `pack.toml` compiled into the app. This card is
the data layer only. The prompt and the install (card 4) build on it, and
nothing user-visible changes here.

Read first: `docs/release/grammar-registry.md` (the schema, upload order and
key), `crates/fe-registry` (the index types, `grammars::CAPTURES`, the
`update_signature` include), `crates/fastedit/src/grammar_install.rs` (hosts,
grants, fetch), and `crates/fe-net/src/grammar_fetch.rs`.

## fe-net: a fetch with no recorded hash

The index can't be hash-pinned, since its signature is what vouches for it.
Add `pub async fn fetch_bounded(client, url, max_bytes) -> Result<Vec<u8>,
GrammarFetchError>` to `crates/fe-net/src/grammar_fetch.rs` (the module is
`pub`, so no lib.rs edit). It shares `follow` and the read limit with
`grammar_fetch`, follows redirects only to granted hosts, honours
`max_bytes`, and checks no hash. Refactor `fetch_limited` so the two don't
duplicate the body loop. Tests go in the new file
`crates/fe-net/tests/bounded_fetch.rs`, on the local server
`tests/grammar_fetch.rs` uses: `bounded_fetch_returns_the_body`,
`bounded_fetch_refuses_over_limit_by_header_and_stream` and
`bounded_fetch_refuses_an_ungranted_redirect`. `tests/grammar_fetch.rs` passes
unchanged.

## fastedit: `crates/fastedit/src/grammar_registry.rs` (new)

Declare it from main.rs (`mod grammar_registry;` and `#[cfg(test)] mod
grammar_registry_tests;`). Add `fe-registry.workspace = true` and
`tree-sitter.workspace = true` to `crates/fastedit/Cargo.toml` (shared
append-only; the same tree-sitter is already in the lock).

- **Baked in:** `include_str!` of `extensions/grammars/registry.toml`, giving
  `index_url`, `url_template` and `public_key`. A test checks that it parses
  and that the key decodes to 32 bytes.
- **Verify before parse:** `verify_index(bytes, sig_text, key) ->
  Result<Index, IndexError>` checks the signature through fe-registry's
  `update_signature` (no second Ed25519 implementation), then parses with
  fe-registry's types and refuses an unknown `schema`. It is a pure function,
  and the key is a parameter, so tests use throwaway keys.
- **Cache:** `<state dir>/registry/index.json` and `index.json.sig`
  (`fe_config::paths::state_dir()`). Every load from the cache verifies again,
  so a cache edited on disk is ignored just as a bad download is. Writes go to
  temporary files renamed into place, the signature first.
- **Refresh:** fetch the `.sig` (at most 4 KiB) and then the index (at most
  4 MiB) with `fetch_bounded` under `grammar_install::grants()`. Verify, and
  refuse a `serial` lower than the cached one (rollback), keeping the cache. On
  a signature failure, wait about 5 s and try once more, because the publisher
  uploads the sig and the index one after the other. Any failure keeps the
  cached copy, is logged, and is never shown to the user.
- **When:** in the background at launch, if the cache is missing or older than
  24 h. Never under `--perf`, `--wait` or tests. Only when the existing
  `update.automatic_checks` setting is on: it's the same consent class, the app
  reaching GitHub on its own, and with it off nothing is fetched until card 4's
  explicit install. Never block startup. Model the gating as a pure `Policy`
  like `sparkle::Policy`.
- **Compatibility:** `EngineSupport { api, abi: MIN_COMPATIBLE..=LANGUAGE_VERSION,
  captures }` from the running app. `api` is the extension API version
  `config::with_extensions` passes kiln: hoist it to a `pub(crate) const` in
  config.rs and use it there and here. `abi` comes from
  `tree_sitter::{MIN_COMPATIBLE_LANGUAGE_VERSION, LANGUAGE_VERSION}`, `captures`
  from `fe_registry::grammars::CAPTURES`. An entry is compatible only when
  `engine.tenmoku` has the same major and a minor no higher, `engine.abi` is in
  range, and `engine.captures` is no higher. Incompatible entries are dropped
  from everything below (not offered, not detected) and logged once.
- **Detection:** `language_for(index, path, first_line) -> Option<&Entry>`, for
  a language that isn't built in and isn't installed (`grammar_install::is_installed`).
  It matches the whole file name first, then the extension (case-insensitive),
  then the `#!` interpreter. For the shebang, use fe-syntax's interpreter
  parsing if T1434 made it public. Otherwise write a small local parser with the
  same rules (`env`, `env -S`, version suffixes such as `python3.12`), tested.
  When several entries match, take the lowest language name and log it. The
  generator should prevent that case. A language a registered extension already
  claims (`fe_syntax::language_name_for_path` says another name) is never
  detected.
- **Pack lookup:** `packs_of(index, language)` in the order the entry lists them
  (the first is the main pack), and `pack_languages(index, pack)` filtered to
  compatible entries.

## Tests

In the new file `crates/fastedit/src/grammar_registry_tests.rs`, as flat
functions, with an injected fetcher (a closure over a map of URL to bytes) and
throwaway keys:

- `good_signature_loads_the_index`
- `tampered_index_is_refused_and_the_cache_is_kept` (one byte changed after
  signing)
- `wrong_key_is_refused`
- `missing_signature_is_refused`
- `tampered_cache_is_ignored`
- `lower_serial_is_refused`
- `unknown_schema_is_refused`
- `signature_race_retries_once`
- `offline_keeps_the_cache`
- `incompatible_entries_are_dropped`: ABI above `LANGUAGE_VERSION` and below
  `MIN_COMPATIBLE_LANGUAGE_VERSION`, captures above, tenmoku minor above, and
  another major
- `detects_by_file_name_extension_and_shebang`
- `installed_built_in_or_claimed_languages_are_not_detected`
- `policy_off_for_perf_wait_tests_and_without_automatic_checks`
- `baked_registry_toml_parses_with_a_32_byte_key`
- `live_index_verifies` (`#[ignore]`, network): fetches the real `index_url`
  through the real client and verifies it with the committed key. Quote its
  run (`--run-ignored only`) in the decision file.

## Scope notes

- Don't change `grammar_install.rs`'s behaviour or `fe-workbench`. Card 4 moves
  the offers and the install onto this. Expose what card 4 needs as
  `pub(crate)` functions. Until card 4 lands, nothing calls them but the
  launch refresh, so mark them `#[cfg_attr(not(test), allow(dead_code, reason =
  "card 4 offers from it"))]` as grammar_install does.
- Decision file `docs/decisions/<lane>-grammar-registry-client.md`: the cache,
  the rollback rule, the refresh policy and why it follows
  `update.automatic_checks`, and the compatibility rule.
