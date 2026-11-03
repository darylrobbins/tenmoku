# Language pack (T428)

Card 6 of `docs/grammar-size-audit/README.md`: an offline download of the eight
first-party grammars.

1. **Built by `grammars.yml`, not `release.yml`.** That workflow already has the
   eight wasm on the grammar tag; `release.yml` is a macOS app build with no
   wasi-sdk. This avoids a second wasm build whose hashes could differ from
   `pack.toml`'s.
2. **A `.tar.gz` of the eight extension directories.** Finder, Linux and WSL open
   it, it needs no zip tool, and installing is unpacking into the extensions
   directory, which the loader's watcher picks up without a restart.
3. **No in-app pack UI here.** The installer can already install from an unpacked
   pack directory (`install_from_pack_dir`); the banner card's failure toast points
   offline users at `docs/release/language-pack.md`.
4. **Stub-wasm tests.** `scripts/package-language-pack.sh` takes
   `GRAMMAR_WASM_DIR` and `DIST_DIR`, so `crates/fastedit/tests/language_pack.rs`
   needs no wasi-sdk and runs on Linux. The script guards `sha256sum`/`shasum`
   with `command -v`. Missing `tar` or a checksum tool fails the tests with a
   message unless `TENMOKU_SKIP_LANGUAGE_PACK=1` is set, which prints SKIPPED and
   returns: nextest can't report a run-time skip, and a silent return reads as a
   pass.
5. **Layout check.** `grammar_install` is crate-private, so the integration test
   compares the unpacked files with `extensions/grammars/<language>/`, which the
   installer compiles in, and with the wasm name the manifest gives.
