---
name: rust-tdd
description: Test-first in fastedit under the fastedit-task workflow - write failing tests that compile, lock them with a tdd-lock commit, then implement without touching them. Use for the write-tests, implement and fix steps.
---

# Rust TDD in fastedit-task

`scripts/paddock-check.sh` enforces this. The workflow runs it as a check after
your step, so run it yourself before you finish.

## Writing the tests (write-tests step)

1. Put new tests in new files named `*_tests.rs`, declared with a
   `#[cfg(test)] mod x_tests;` line in the owning module, or under the crate's
   `tests/` or `benches/`. Only those names are locked. Don't add tests to an
   inline `mod tests {}` in a source file.
2. Add the smallest stubs that make the tests compile: the new types and
   functions with their final signatures. A stub returns a plausible wrong
   value (`Default::default()`, empty, zero) so the test fails on an
   assertion. Use `todo!()` only when no value makes sense.
3. Each new test must fail now because the behaviour is missing, not because
   of setup. A test that passes against the stub tests nothing new: change it
   or drop it.
4. Commit tests and stubs together in one commit with the subject
   `tdd-lock: <item title>`. That commit locks its test files. It must be
   the **last** commit of this step: the red check fails if anything is
   committed after it. Put docs or other edits in the same commit, or leave
   them for the implement step. To fix the tests after a failed red check,
   amend that commit or make a new `tdd-lock:` commit.
5. Run `scripts/paddock-check.sh red <crate>...` and finish only on `RED OK`.

## Implementing (implement step)

- Make the tests pass **without changing them**. That means no edits to locked
  files, no `#[ignore]`, no removing or `cfg`-ing out their `mod` line, and no
  filtering. The green check compares locked files byte for byte and requires
  every test that failed at red to run and pass by name.
- Never make a `tdd-lock:` commit while implementing.
- If a locked test is wrong, don't work around it. Stop and say why in your
  final message: the step fails and a person decides.
- Commit your work (any subject but `tdd-lock:`) and leave the tree clean.
  Uncommitted work is not merged.
- Run `scripts/paddock-check.sh green <crate>...` and finish only on
  `GREEN OK`.

## Fixing CI or review findings (fix-ci, address-review steps)

- `scripts/paddock-check.sh ci` must print `CI OK`: fmt, clippy with
  `-D warnings`, the workspace nextest run, the no-GPUI and syntax-feature
  checks, and the test lock.
- Fix the implementation, not locked tests. If a review finding is about a
  test itself (missing case, weak assertion), add or strengthen tests in a
  separate `tdd-lock:` commit whose body names the finding. Never weaken or
  delete an existing assertion.

## Cargo

Build only in this worktree's own `target/`; the script pins
`CARGO_TARGET_DIR` for its own runs. Don't point cargo at a shared target
dir: parallel items would then run each other's test binaries.
