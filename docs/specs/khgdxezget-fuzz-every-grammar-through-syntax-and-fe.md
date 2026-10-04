---
task_uid: "01m417z3ynf6y6exkhgdxezget"
task: "T12"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "fuzz/ is a standalone cargo workspace; gate stages build and deny pass with the root Cargo.toml and Cargo.lock unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "fuzz target syntax_edits covers every language in fe_syntax::language_names() and asserts the incremental tree s-expression and highlights equal a fresh parse; ran at least 10 min under ASan (manual: log)"
    required: true
    manual: false
  - id: "C3"
    text: "fuzz target buffer_edits checks edit/undo/redo streams over two origins against a String model, with Conflict leaving text unchanged; ran at least 10 min under ASan (manual: log)"
    required: true
    manual: false
  - id: "C4"
    text: "TESTING.md and fuzz/README.md document the by-hand nightly cargo-fuzz run, kept out of cargo test and the gate"
    required: true
    manual: false
  - id: "C5"
    text: "Each crash found has a minimized input under fuzz/regressions/\u003ctarget\u003e/ and drafted upstream-issue text or a follow-up card (manual)"
    required: true
    manual: false
---

# Fuzz every grammar through Syntax and fe-buffer edit/undo streams with cargo-fuzz, run by hand

## Problem

fastedit links 15 tree-sitter grammars, all C. Several have hand-written
external scanners: Python, YAML, Markdown, Bash, HTML and C++ raw strings. A
memory error or abort in one takes down the whole editor. fe-syntax's
fail-soft parsing (T-08: a parse or query that panics leaves plain text) uses
`catch_unwind`, which can't stop a segfault or abort in C. fe-buffer's
undo/redo is covered by proptest reference models (TESTING.md), but those never
see arbitrary byte input under a sanitizer.

## Change

Add a standalone cargo-fuzz crate at `fuzz/`. Its `Cargo.toml` has its own
`[workspace]` table (what `cargo fuzz init` writes), so it stays out of the
root workspace: no change to the root `Cargo.toml` or `Cargo.lock`, and the
gate never builds it. It depends on `fe-buffer` and `fe-syntax` (default
features, all languages) by path, plus `libfuzzer-sys` and `arbitrary`
(`derive`). Commit `fuzz/Cargo.lock`.

Targets:

1. `fuzz/fuzz_targets/syntax_edits.rs`. An `arbitrary` input picks a language
   from `fe_syntax::language_names()` (resolved with `language_by_name`), an
   initial text (bytes, lossily decoded to UTF-8) and up to about 32 edit
   batches. Each edit is a byte range plus inserted text, with offsets clamped
   to char boundaries. For each batch it applies the edit with
   `Buffer::edit(.., Origin::User)`, calls `Syntax::edit(snapshot)`, parses to
   completion (`parse_job().run()` then `finish`) and calls
   `highlights(0..len)`. At the end it builds a fresh `Syntax` over the final
   snapshot and asserts the same `tree().root_node().to_sexp()` and the same
   `highlights(0..len)`, unless either side reports `failed()`. Cap the input
   text at 64 KB.
2. `fuzz/fuzz_targets/buffer_edits.rs`. A stream of `Edit`, `Undo` and `Redo`
   ops over two origins (`User`, `Ai`), checked against a `String` model:
   - after each edit, `buffer.text()` equals the model;
   - an undo or redo that returns `Err(UndoError::Conflict)` leaves the text
     unchanged;
   - undoing every `User` transaction in a single-origin run restores the
     original text, and redoing them all restores the final text.

Add `fuzz/README.md` with the run commands. cargo-fuzz needs nightly, which the
1.98.1 pin in `rust-toolchain.toml` doesn't cover, so use
`cargo +nightly fuzz run <target> --fuzz-dir fuzz -s address --
-max_total_time=600` and don't add a toolchain file. Add a "Fuzzing" section to
TESTING.md: run by hand before milestones, never in `cargo test` or the gate.

Run each target for at least 10 minutes with AddressSanitizer. For every crash:

- save the minimized input under `fuzz/regressions/<target>/`;
- find the grammar or fe-buffer code at fault;
- for a grammar, draft the upstream issue text and a follow-up card to pin or
  patch around it.

Fixing crashes is not part of this card.

## Notes

- Fuzzing runs by hand, so the evidence for C2, C3 and C5 is manual: put the
  run logs (target, duration, executions, crashes) in the decision file.
- `cargo deny` only checks the root workspace. `libfuzzer-sys` (MIT/Apache-2.0
  with NCSA for the bundled libFuzzer) and `arbitrary` (MIT/Apache-2.0) are
  already in the root lock through GPUI. A person still approves them for
  `fuzz/` under the licensing policy.
- Disk is nearly full. A nightly toolchain plus an ASan build of the grammars
  is several GB.

## Open questions

- Should there be an on-demand CI job, like `soak`? The source card allowed
  either a by-hand run or an on-demand job. This card does by-hand only,
  because CI workflows are a human path.
