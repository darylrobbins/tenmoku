# Fuzzing

Two [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets, run by
hand before milestones. They're never part of `cargo test` or the gate: this
directory is its own cargo workspace (see the `[workspace]` table in
`Cargo.toml`), with its own `Cargo.lock`, so the root workspace doesn't build
it.

| Target | What it checks |
|---|---|
| `syntax_edits` | Arbitrary text in each language from `fe_syntax::language_names()`, edited in up to 32 batches with a full reparse and highlight after each, ends with the same tree s-expression and highlights as a fresh parse. Its real job is running the grammars' C parsers and external scanners under AddressSanitizer. It builds fe-syntax with `no-query-budget`, so highlight queries always complete however busy the machine is |
| `buffer_edits` | Edit, undo and redo streams from `User` and `Ai` match a `String`: edits apply exactly, and an undo or redo refused with `Conflict` leaves the text unchanged. With one origin, nothing is refused, undoing everything restores the original text and redoing it restores the final text |

## Running

cargo-fuzz needs nightly, which the `rust-toolchain.toml` pin doesn't cover,
so pass `+nightly`. From the repository root:

```zsh
rustup toolchain install nightly   # once
cargo install cargo-fuzz           # once

# Instrument the grammars' C code too, not just the Rust (see below).
export CFLAGS="-fsanitize=address,fuzzer-no-link -mllvm -asan-guard-against-version-mismatch=0"
cargo +nightly fuzz run syntax_edits --fuzz-dir fuzz -s address -- -max_total_time=600
cargo +nightly fuzz run buffer_edits --fuzz-dir fuzz -s address -- -max_total_time=600
```

libFuzzer grows inputs slowly: in ten minutes `syntax_edits` gets only to
about 40 bytes, too short for most grammar states. Add
`-len_control=0 -max_len=4096` to start at full length. A run stops at the
first crash; to keep going past a known one and collect the rest, add
`-fork=4 -ignore_crashes=1`.

`-s address` only instruments Rust. The grammars and the tree-sitter runtime
are C built by the `cc` crate, which reads `CFLAGS`: `address` finds their
memory errors and `fuzzer-no-link` gives libFuzzer coverage of them, without
which it can't steer into scanner code. Apple clang's ASan expects its own
runtime version, while the runtime linked is rustc's, so
`-asan-guard-against-version-mismatch=0` drops the check that would otherwise
fail the link.

The build (about 600 MB with ASan) goes to the cargo target directory; set
`CARGO_TARGET_DIR` to keep it out of a shared one. Corpora collect under
`fuzz/corpus/` and crashes under `fuzz/artifacts/`; both are ignored.

## Known findings

`fuzz/regressions/syntax_edits/` holds a minimized input for every crash
found so far, all upstream bugs and all still open. The decision file maps
each one to the crash it came from.

- `tsx-incremental-parse-hang*`: a TSX parse that never finishes, and a
  fresh parse of the same text doesn't either. They no longer time out,
  because fe-syntax bounds the parse and falls back to plain text, but the
  upstream hang is still open.
- `bash-heredoc-*`, `markdown-inline-*`, `java-*`, `python-*`, `cpp-*` and
  `javascript-*`:
  incremental reparses that disagree with a fresh parse.

An input's first byte picks its language: an index into
`fe_syntax::language_names()`, modulo the number of languages. The saved
inputs use an index below 16, the languages there when they were found, so
they keep their language as long as new languages are added after those.

syntax_edits runs into some of them within minutes, so use `-fork=4
-ignore_crashes=1` to find anything else, and replay each new crash alone
before triaging it. See `docs/decisions/khgdxezget-fuzzing.md`.

## When it finds a crash

```zsh
cargo +nightly fuzz tmin <target> --fuzz-dir fuzz -s address fuzz/artifacts/<target>/crash-…
cargo +nightly fuzz fmt  <target> --fuzz-dir fuzz fuzz/artifacts/<target>/minimized-from-…
```

Save the minimized input as `fuzz/regressions/<target>/<short-name>`, find
the grammar or fe-buffer code at fault, and for a grammar, draft the upstream
issue and a card to pin or patch around it. To replay a saved input:

```zsh
cargo +nightly fuzz run <target> --fuzz-dir fuzz -s address fuzz/regressions/<target>/<file>
```
