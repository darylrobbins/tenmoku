---
task_uid: "01m6c8n080y7y8f7v9dahs2xw4"
task: "T1287"
rev: 2
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/grammar-size-audit/grammar-sizes.sh prints each tree-sitter grammar's code and data size and its embedded query size, sorted, with a total, from an existing debug or release build-dir, without building"
    required: true
    manual: false
  - id: "C2"
    text: "docs/grammar-size-audit/README.md has the sections Per-grammar size (the script's table, with date and commit), Release cross-check (results, or 'not run: awaiting the user's go-ahead'), Day-one languages (sources cited), Proposed built-in set (size and savings), On-demand path (through crates/fe-syntax/src/runtime.rs register_language), Missing popular formats (which to add built in and which on demand), and Proposed follow-up cards; each follow-up card names every file it would change or create by its complete repo-relative path (no placeholder directories), found in the tree or, for a new file, placed as the existing grammars' files are"
    required: true
    manual: false
  - id: "C3"
    text: "The doc says whether a release build ran, and quotes the user's approval if it did"
    required: true
    manual: false
  - id: "C4"
    text: "git diff --stat lists only the two files under docs/grammar-size-audit/"
    required: true
    manual: false
---

# Measure each built-in tree-sitter grammar's share of the release binary, choose a day-one built-in set, and propose loading the rest on demand as WebAssembly grammars

## Problem

The stripped release binary is about 89 MB, and the 26 built-in tree-sitter grammars are about 35 MB of it
(`docs/specs/ffg5hm7557-strip-symbols-from-release-builds-strip.md`: "26 static tree-sitter grammars ~35 MB,
Rust code ~40 MB"). Tenmoku is meant to be lightweight and measured (user, 2026-10-31), and the first public
release is macOS-only and editor-first (2026-11-01). Every grammar is a default feature of fe-syntax
(`crates/fe-syntax/Cargo.toml` `[features] default`, one feature per language, checked by
`scripts/check-syntax-features.sh`), so the cost is opt-out already; nobody has measured it per language or
chosen which ones a new user needs on day one.

A first rough look (2026-11-01, `size -m` over each grammar's static library in a lane's debug build-dir,
`__TEXT` + `__DATA*` sections, parse tables being constant data) gives ~36.8 MB in all, of which Kotlin ~5.5,
C# ~5.1, Scala ~3.8, Swift ~3.6, C++ ~3.3, TypeScript+TSX ~2.8, SQL ~2.4, PHP ~2.0 and Ruby ~2.0 MB are ~30 MB;
Rust ~1.1, Python ~0.5, JavaScript ~0.4, Go ~0.2 and JSON/TOML/HTML/CSS under 0.2 each.

Extensions can already load grammars at run time as WebAssembly (T845: `crates/fe-syntax/src/runtime.rs`
`register_language`, epoch deadlines; extension grammar entries in `crates/fastedit/src/config.rs`, built with
`tree-sitter build --wasm`).

## Change

Write `docs/grammar-size-audit/README.md` and a measuring script `docs/grammar-size-audit/grammar-sizes.sh`:

1. **Per-grammar size.** The script finds each `tree-sitter-*` crate's static library under the build-dir cargo
   reports (`cargo metadata` / `CARGO_TARGET_DIR`, `debug` or `release`), sums its code and data sections with
   `size -m` (debug info excluded), adds the size of its embedded queries (`.scm` files fe-syntax
   `include_str!`s), and prints a table sorted by size with a total. The doc gives the table and says how close
   the static-library sum is to a real release binary's difference.
2. **Release cross-check, only with the user's go-ahead.** A release build of fastedit is slow and large here.
   Before running any `cargo build --release`, block the task with the question, the commands and the
   expected disk use, and wait. With approval: build `-p fastedit` release twice, with fe-syntax's default
   features and with the proposed built-in set only, compare stripped binary sizes, and clean the extra
   target afterwards.
3. **Day-one languages.** Which languages a typical developer opening Tenmoku needs without installing
   anything, with the sources cited (developer surveys, GitHub language statistics) and Tenmoku's own needs:
   Markdown (preview, chat), TOML/JSON (settings), and injections (HTML pulls in JavaScript and CSS; Markdown
   fences inject many).
4. **Proposed built-in set** with its size and what it saves, and **an on-demand path** for the rest built on
   the existing WebAssembly grammar loading: first-party grammar extensions built from the same grammar
   sources, how they're delivered (bundled in a separate download, or fetched on first open with consent), what
   the user sees when opening a file whose grammar isn't installed, and the cost of WebAssembly parsing against
   native (measured on one grammar if a bench covers it, else stated as unmeasured).
5. **Proposed follow-up cards**, ordered, each with title, type, size, exact files and dependencies.

No change to Cargo manifests, fe-syntax features or the release workflow here.
