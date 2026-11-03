---
task_uid: "01m6ffyatw7whvbyc1pt00h80c"
task: "T425"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "grammar_pack_manifest::pack_lists_all_eight: extensions/grammars/pack.toml has an entry for each of the eight languages (kotlin, csharp, scala, swift, cpp, sql, php, ruby) with file name equal to the wasm name in that language's extension.toml, a size in bytes and a sha256 that is either 64 lowercase hex digits or empty (empty means not yet released and every fetch of it is refused); base_url is an https github.com release URL"
    required: true
    manual: false
  - id: "C2"
    text: "grammar_fetch::good_download_is_returned: fe_net::grammar_fetch against a local http server (127.0.0.1, allowed by fe-net for tests) returns the body when its SHA-256 equals the expected one"
    required: true
    manual: false
  - id: "C3"
    text: "grammar_fetch::wrong_hash_is_refused: a body whose SHA-256 differs yields GrammarFetchError::HashMismatch carrying both digests, and nothing is returned"
    required: true
    manual: false
  - id: "C4"
    text: "grammar_fetch::empty_hash_is_refused_without_a_request: an empty expected hash yields NotReleased and the server sees no request"
    required: true
    manual: false
  - id: "C5"
    text: "grammar_fetch::oversize_is_cut_off: a body longer than the caller's max_bytes (and a Content-Length over it) yields TooLarge after at most max_bytes+1 bytes are read"
    required: true
    manual: false
  - id: "C6"
    text: "grammar_fetch::follows_redirects_only_to_granted_hosts: a 302 to a host the Grants name is followed (at most 3 hops); a 302 to any other host yields HostNotAllowed with no request to it"
    required: true
    manual: false
  - id: "C7"
    text: "grammar_install_tests::installs_a_loadable_extension: installing a fixture grammar (the tiny valid wasm from crates/fe-syntax/tests/runtime_grammar.rs's fixtures, copied under crates/fastedit as a test string or built in the test) into a temp extensions dir writes \u003cdir\u003e/tenmoku.grammar_\u003clang\u003e/ with extension.toml, highlights.scm, injections.scm and the wasm, and extension_grammars() on that manifest yields a GrammarSpec whose fe_syntax::register_language succeeds"
    required: true
    manual: false
  - id: "C8"
    text: "grammar_install_tests::install_is_atomic: a fetch that fails (hash mismatch, offline, cancelled) leaves no \u003cdir\u003e/tenmoku.grammar_\u003clang\u003e/ and no temp directory behind; installing over an existing directory replaces it whole"
    required: true
    manual: false
  - id: "C9"
    text: "grammar_install_tests::offline_fallback_reads_a_pack_dir: with a language-pack directory (the layout of docs/release/language-pack.md, one tenmoku.grammar_\u003clang\u003e/ per language) installing from it copies the verified wasm without any network; a wasm whose hash differs from pack.toml is refused"
    required: true
    manual: false
  - id: "C10"
    text: "cargo nextest run -p fe-net -p fastedit passes; cargo clippy --workspace --all-targets -- -D warnings passes; fe-net still has no GPUI dependency"
    required: true
    manual: false
---

# Fetch a first-party wasm grammar with consent-ready plumbing: a recorded-hash pack manifest, a hash-checked download in fe-net, and an installer that writes the grammar as a user extension the loader already picks up (Mac T1320)

Card 5 of docs/grammar-size-audit/README.md is split: this card is the headless fetch and install; the banner is the next card. Nothing here is user-visible and nothing downloads on its own: the only caller is the banner card's Install click.

Design calls (record in docs/decisions/<lane>-*.md):
1. Install as an ordinary user extension. The installer writes <user extensions dir>/tenmoku.grammar_<lang>/ (extension.toml, highlights.scm, injections.scm and the wasm); fastedit's existing kiln loader watches that directory and registers `[[contributes.grammars]]` on its background thread (crates/fastedit/src/config.rs), so no new registration path and no change to config.rs. The three text files are embedded in the app with include_str! from extensions/grammars/<lang>/ (they are the files the parity tests use), so only the wasm travels. The extensions picker already lists and disables it.
2. Hash recorded in the app: extensions/grammars/pack.toml (base_url, and per language: wasm file name, size, sha256) is include_str!'d and parsed by grammar_install.rs. The wasm is hashed after download and before anything touches disk outside a temp dir; mismatch is refused.
3. Where assets live: a release tagged for the grammars (grammars.yml already builds on any tag), whose tag pack.toml names in base_url, rather than the app's own version. Reason: the hash must be in the app that is built before or with the tag, and grammar bytes change only on a pin bump, so decoupling avoids a chicken-and-egg release and re-hashing every app release. Until a release exists the sha256 fields are empty and fetching is refused (criterion). scripts/record-grammar-hashes.sh (new; needs `gh` and sha256sum/shasum, guards each with command -v) downloads the release's eight wasm and rewrites pack.toml; the lead runs it after the first grammar release, outside this card.
4. fe-net does not follow redirects (crates/fe-net/src/lib.rs) and a GitHub release asset answers 302 to objects.githubusercontent.com. grammar_fetch.rs follows up to 3 redirects itself, each only to a host in the caller's Grants (github.com and objects.githubusercontent.com in the app), so lib.rs's no-redirect rule for Client stays. It needs sha2 (already a workspace dependency, 0.10) in fe-net's Cargo.toml and a mod line in lib.rs.
5. Offline fallback is a directory install: grammar_install.rs can install from a language-pack directory (the layout the next-but-one card publishes), verifying against the same pack.toml hashes.
6. The installer is plain functions taking a fetcher closure/trait object and a target dir, so tests inject fakes and run on Linux with no network. No GPUI in fe-net or grammar_install.rs.
Mac-or-Linux: all tests are headless.
