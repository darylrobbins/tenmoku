---
task_uid: "01m6sp82cmcce3c0k00t981803"
task: "T297"
rev: 1
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "scripts/build-grammar-wasm.sh with no arguments passes on this Mac: the eight build, and wasm_parity's ignored every_extension_highlights_its_sample passes for all eight against crates/fe-syntax/tests/golden, those goldens unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "A git-sourced (git + 40-hex rev) and a crates.io-tarball-sourced (crate + version + sha256) grammar directory each build with scripts/build-grammar-wasm.sh \u003clanguage\u003e, pass every_extension_highlights_its_sample, and are compiled into fastedit and packed by package-language-pack.sh with no edit outside that directory; the decision file shows the run (the fixture directories aren't committed)"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's every_extension_is_well_formed and fastedit's grammar_install_tests::every_first_party_manifest_parses and grammar_install_tests::every_first_party_grammar_is_complete pass, and tests show each failing for a directory missing its sample or golden, with a non-40-hex rev, with an unmapped highlights capture, with an extension a built-in claims, and with an unknown extension.toml key"
    required: true
    manual: false
  - id: "C4"
    text: "No per-language list or count remains in wasm_parity.rs, grammar_install.rs (crates/fastedit/build.rs generates the table from the directories), grammar_install_tests.rs, grammar_pack_manifest.rs, language_pack.rs, package-language-pack.sh or build-grammar-wasm.sh (php's sql comes from parity_needs); pack.toml holds base_url and recorded entries only, and a directory without an entry is offered and refused as an empty-hash entry is today"
    required: true
    manual: false
  - id: "C5"
    text: "scripts/build-grammar-wasm.sh downloads the pinned wasi-sdk for macOS arm64 and Linux x86_64/arm64 when none is present (SHA-256 checked; a gate test checks its version equals grammars.yml's WASI_SDK_VERSION), bounds parallel clang jobs, and filters the parity test by GRAMMAR_LANGUAGES; .github/workflows/grammars.yml is unchanged"
    required: true
    manual: false
  - id: "C6"
    text: "extensions/grammars/README.md documents adding a language; docs/release/language-pack.md names no count; cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Make adding a first-party wasm grammar touch only extensions/grammars/<language>/: discovered build, git and crates.io grammar sources, per-directory samples and goldens, a generated install table, no per-language lists (Mac T1433)

# Adding a wasm grammar touches only extensions/grammars/<language>/

Goal: after this card, a new first-party WebAssembly language is one new
directory `extensions/grammars/<language>/` (plus appended `THIRD_PARTY.md`
rows, which `regatta.toml` already lets any task append). No script, workflow,
test table, Rust table or `pack.toml` edit. Then the ~45 language cards that
depend on this one can run in parallel on every host.

Read first: commits c0c7ff01 (T1304) and e834f394 (T1305),
`docs/decisions/jd5c0dwjt7-grammar-wasm.md`, `docs/decisions/vkgx7c7far-grammar-wasm.md`,
`docs/decisions/c1pt00h80c-grammar-fetch.md`, `docs/release/language-pack.md`.

## What lists the languages today (each must stop needing an edit)

1. `crates/fe-syntax/tests/wasm_parity.rs`: one `#[ignore]` test per language,
   the `LANGUAGES` table, and php's hard-coded `load_injected("sql")`.
2. `scripts/build-grammar-wasm.sh`: already discovers
   `extensions/grammars/*/grammar-source.toml`, but (a) only knows the
   Cargo.lock crate source, which a language with no native crate has not got,
   (b) hard-codes php -> sql, (c) filters tests by `test(=<language>)`,
   (d) starts every clang at once.
3. `crates/fastedit/src/grammar_install.rs`: `FIRST_PARTY` / `first_party!`
   `include_str!`s each language's files; `[_; 8]`.
4. `extensions/grammars/pack.toml`: one `[languages.<l>]` entry each.
5. `crates/fastedit/tests/grammar_pack_manifest.rs`,
   `crates/fastedit/tests/language_pack.rs`,
   `crates/fastedit/src/grammar_install_tests.rs`: `LANGUAGES`/`EIGHT` consts.
6. `scripts/package-language-pack.sh`: dies unless there are exactly 8.
7. `docs/release/language-pack.md`: says "eight" and names them.

`.github/workflows/grammars.yml` names no language (it runs the script and
uploads `target/grammar-wasm/*.wasm`), so **this card does not edit it** and
needs no workflow risk approval. If you find a workflow edit is unavoidable,
stop and block the card saying what and why: `.github/workflows/**` needs the
user's risk approval.

## Design (deviate only with the reason in the decision file)

- **grammar-source.toml, two new source forms** beside today's Cargo.lock
  crate (kept for the eight):
  - git: `git = "https://github.com/<org>/<repo>"`, `rev = "<40-hex commit>"`,
    `src = "<dir with parser.c>"` (relative to the repository root, e.g.
    `grammars/ocaml/src`), `grammar = "<export>"`. The script fetches exactly
    that commit (`git init` + `git fetch --depth 1 origin <rev>` works on
    GitHub) into `target/grammar-src/<language>-<rev>/`, reusing it when present.
    Keep the whole checkout, not just `src/`: tree-sitter-ocaml and
    tree-sitter-fsharp scanners `#include "../../common/scanner.h"`.
  - crates.io tarball, for a grammar whose git tree has no generated
    `parser.c` (tree-sitter-latex): `crate`, `version`, `sha256` of the
    `.crate`, `src`, `grammar`, plus `registry = "crates.io"` (or another
    explicit marker) so it is never confused with a Cargo.lock pin. Download
    from `https://static.crates.io/crates/<c>/<c>-<v>.crate`, check the
    SHA-256 before unpacking.
  - Optional `parity_needs = ["sql"]`: other first-party languages this one's
    sample injects, which the script also builds and the test registers under
    their real names. php gets `parity_needs = ["sql"]`, replacing both
    special cases.
- **Script**: bound parallel clang jobs (`GRAMMAR_JOBS`, default half the
  cores, min 1); per-language filtering by an env var the test reads
  (`GRAMMAR_LANGUAGES=dart,zig`), not test names; on a missing wasi-sdk,
  download the pinned wasi-sdk for this host (macOS arm64, Linux x86_64 and
  arm64) into `~/.cache/tree-sitter/wasi-sdk` with a SHA-256 per archive, the
  same version as `grammars.yml`'s `WASI_SDK_VERSION` (a gate test reads both
  files and checks they agree). Linux hosts must be able to run
  `scripts/build-grammar-wasm.sh <language>` from a clean checkout.
- **Samples and snapshots per directory**: a language with no built-in
  (every new one) holds `test/sample.<ext>` and `test/sample.<ext>.golden`, the
  golden in `src/tests/golden.rs`'s span format. The eight keep using
  `crates/fe-syntax/tests/golden/<sample>` (a fixed map in the test, which
  never grows), so wasm-vs-native parity is unchanged for them.
- **wasm_parity.rs**:
  - `every_extension_highlights_its_sample` (`#[ignore]`, run by the script):
    for every directory (or those in `GRAMMAR_LANGUAGES`), load the module
    through `register_language`, highlight the sample, require no ERROR or
    MISSING node, compare with the golden; `UPDATE_GOLDEN=1` writes it. Report
    every failing language, not just the first. Replaces the eight
    per-language ignored tests.
  - `every_extension_is_well_formed` (gate, no module needed): for every
    directory: the file set (required: `LICENSE`, `extension.toml`,
    `grammar-source.toml`, `highlights.scm`, `injections.scm`; allowed:
    `LICENSE-*`, `NOTICE`, `test/sample.*` + its `.golden`; no `.wasm`); the
    sample and golden exist for a non-built-in language; grammar-source.toml
    is exactly one valid form (40-hex rev, https GitHub URL, 64-hex sha256);
    the wasm is named after the export; `language` equals the directory name;
    no file extension or file name is a built-in's or another directory's;
    every `parity_needs` entry is a directory; every highlights.scm capture not
    starting with `_` maps through `Highlight::from_capture` (non-built-in
    languages only; the eight are the built-ins' queries verbatim).
  - Keep `*_extension_is_as_built_in`, `each_pin_is_cargo_locks` (for
    Cargo.lock sources), `a_missing_module_fails_naming_the_script` and the
    `publishing` module.
- **fastedit**: a `crates/fastedit/build.rs` that lists
  `extensions/grammars/*/` (sorted) and writes the `include_str!` table into
  `OUT_DIR`, with `cargo:rerun-if-changed` on `extensions/grammars` and each
  directory, so a new directory is compiled in with no source edit. The
  tests' `LANGUAGES`/`EIGHT` consts read the directories instead. Add
  `grammar_install_tests::every_first_party_manifest_parses`: each compiled-in
  `extension.toml` parses as the extension loader's schema (`config.rs`'s
  `ExtensionGrammar`, `deny_unknown_fields`), so a language card that writes
  an unknown key fails in the gate.
- **pack.toml** holds `base_url` and *recorded* entries only. A first-party
  directory with no entry is "not released": offered and refused exactly as an
  entry with `size = 0, sha256 = ""` is today (keep that behaviour and its
  tests). `scripts/record-grammar-hashes.sh` writes an entry for every
  directory, from the directories, not pack.toml's `file =` lines, and still
  refuses if the release lacks one. A test checks every entry names a
  directory and its `extension.toml` wasm. If entries are already recorded
  when you land (see "grammars-1" below), keep them byte for byte.
- **Language pack**: `package-language-pack.sh` counts directories (at least
  one), copies only `extension.toml`, `highlights.scm`, `injections.scm` and the
  wasm (never `test/`), and `language_pack.rs` reads the directories.
- **Gate cost** (optional, recommended): in `scripts/ci-test-affected.sh` map
  `extensions/grammars/*` to the readers `fe-syntax fastedit` instead of the
  whole workspace (it is outside every package today, so each language card
  would run the whole workspace); extend `scripts/test-ci-test-affected.sh`.
- **Docs**: `docs/release/language-pack.md` stops counting/naming languages.
  Add `extensions/grammars/README.md`: how to add a language (source forms,
  files, sample/golden, `UPDATE_GOLDEN=1 scripts/build-grammar-wasm.sh <l>`,
  query provenance and licences, THIRD_PARTY rows). Language cards follow it.
  Do not edit README.md (T1426 claims it); note in the decision file that its
  language list will need a docs follow-up.
- `THIRD_PARTY.md`: append only (a note on the git and crates.io source forms
  under "Grammars shipped as WebAssembly"); don't rewrite existing lines.

## Distribution is about to change: stay compatible, don't build it

The likely direction (user deciding now): per-language artifacts (wasm +
queries + LICENSE, each hashed) in a public repo, a generated index, and
"packs" as metadata only (a named list of languages; a language may be in
several). That is a separate follow-up card. Here:

- Each directory's `extension.toml` (plus `grammar-source.toml`) stays the
  single source of a language's metadata; don't add per-language data to
  `pack.toml` or to any script or table beyond the recorded hashes above.
- Don't add a `packs` key or an index, and don't grow the all-in-one tarball
  beyond the directory count fix. Make the per-language pieces (file set,
  wasm, recorded hash) easy to iterate so the follow-up can emit one artifact
  per directory.

## grammars-1

The grammars-1 release (tag at 16f85127) has no assets yet and pack.toml has
no recorded hashes. Re-doing that release is the user's call and a human
step; this card neither does it nor waits for it. Whichever lands first, the
other keeps working: record-grammar-hashes.sh before this card writes the old
format, which this card preserves; after it, the new script records every
directory.

## Proof

Add one throwaway git-sourced language in a test fixture or a scratch run (not
committed) to show that a new directory alone builds, tests and packs; say in
the decision file how you checked.

## Implementation notes

- Decisions and the proof run: `docs/decisions/k00t981803-grammar-discovery.md`.
- The eight's samples are found, not mapped: a language with a sample in
  `crates/fe-syntax/tests/golden/<language>.*` is held to it; any other must
  hold `test/sample.*`.
- Outside the scope: `crates/fastedit/src/config.rs` (`ExtensionGrammar` made
  `pub(crate)` for `every_first_party_manifest_parses`) and
  `extensions/grammars/cpp/injections.scm` (T1391's printf injections, which the
  extension had missed, so C++ failed C1).
