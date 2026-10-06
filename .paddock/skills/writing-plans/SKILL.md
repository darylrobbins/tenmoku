---
name: writing-plans
description: Split an approved fastedit design into independent items that are built test-first in parallel worktrees and merged without conflicts. Use for the plan step of the fastedit-task workflow.
---

# Writing plans for fastedit-task

Each item you emit runs on its own: a fresh worktree branched from the run, its
own failing tests, its own implementation, then a merge back with the others.
Up to three run at once. Plan for that.

## Rules

1. **Independent.** No item may need another item's code to compile or pass.
   If B uses an API that A adds, A and B are one item. There is no ordering.
2. **Disjoint files.** No path appears in two items. Shared touch points
   (`lib.rs`/`mod.rs` `mod` lines, a crate's `Cargo.toml`, the workspace
   `Cargo.toml`) belong to exactly one item. If two items both need one, merge
   them.
3. **Few items.** One item is a fine plan. Split only along real seams, such as
   different crates or unrelated modules. Never more than six.
4. **Cover the task.** Every acceptance criterion maps to at least one item.
   Name the criteria each item covers in its `spec`.
5. **Tests live in lockable files.** New tests go in new files named
   `*_tests.rs` (declared `#[cfg(test)] mod x_tests;` in the owning module) or
   under the crate's `tests/` or `benches/`. The workflow locks only those
   names. List the test files in `paths`. A test that needs an item that
   doesn't exist yet goes in a new file, never in an existing `*_tests.rs`.
6. **Human-owned paths.** `Cargo.lock`, `deny.toml`, `rust-toolchain.toml`
   and `.github/workflows/**` need a person's sign-off in this repo. If an
   item must touch them (a new dependency, say), say so in its `spec`.

## Fields

- `title`: an imperative sentence; it becomes the `tdd-lock:` commit subject.
- `spec`: behaviour, edge cases, the acceptance criteria covered, and the
  design decisions from the brief that apply.
- `crates`: the cargo package names (from `crates/*/Cargo.toml` `name`) whose
  tests prove the item. The checks run `cargo nextest -p` on exactly these.
- `paths`: every file the item may create or edit, test files included.
- `tests`: the behaviours the failing tests must pin down, one per line.
