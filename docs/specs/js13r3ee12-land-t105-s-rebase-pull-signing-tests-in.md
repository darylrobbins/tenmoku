---
task_uid: "01m4tcanpmkpebkgjs13r3ee12"
task: "T108"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-git tests pull_rebase_signing::rebase_pull_with_no_gpg_sign_merge_options_is_refused_unchanged, pull_rebase_signing::rebase_pull_ignores_merge_options_sign and pull_rebase_signing::merge_pull_with_merge_options_sign_is_refused pass on helium main, with the assertions described in the spec"
    required: true
    manual: false
  - id: "C2"
    text: "Only crates/fe-git/tests/pull_rebase_signing.rs (and the task's docs/specs file) change; no fe-git src/, tests/remote.rs, Cargo.toml or Cargo.lock change; cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Land T105's rebase-pull signing tests in fe-git as extra coverage of T79's fix, so a mergeOptions-driven signing rebase can't stop half done (Mac T638 tests)

## Why

Mac T638 (helium T105, abandoned) asked fe-git's pull to decide whether a **rebase** pull signs from `commit.gpgSign`
alone, so `branch.<name>.mergeOptions` never starts a signing rebase that stops half done (HEAD detached, rebase in
progress). Helium main already has that fix from T79 (529d9c8, helium-batch-17): `signs_commits` reads mergeOptions only
for Merge. The Mac lead dropped T638 and takes batch 17 instead
(~/coordination/from-mac-lead-2026-10-13-helium-t105-answer.md), but asked to keep T105's tests as extra coverage of the
half-done rebase case. This chore lands those tests. It changes no production code.

## Change

Add `crates/fe-git/tests/pull_rebase_signing.rs` exactly as T105's worker wrote it, from commit `0257af0` on branch
`saved/T105-tests`:

    git checkout saved/T105-tests -- crates/fe-git/tests/pull_rebase_signing.rs

It holds three `#[cfg(unix)]` top-level tests on a diverged clone, each checking that the signer never ran:

- `rebase_pull_with_no_gpg_sign_merge_options_is_refused_unchanged`: commit.gpgSign=true with
  branch.main.mergeOptions=--no-gpg-sign. `pull(PullMode::Rebase)` is the signing refusal. HEAD is unchanged on
  refs/heads/main, there is no .git/rebase-merge or .git/rebase-apply, and status is clean.
- `rebase_pull_ignores_merge_options_sign`: commit.gpgSign=false with mergeOptions=-S. A Rebase pull is Ok and HEAD^ is the
  upstream tip.
- `merge_pull_with_merge_options_sign_is_refused`: commit.gpgSign=false with mergeOptions=-S. A Merge pull is the signing
  refusal and HEAD is unchanged.

The tests already pass on helium main. Change the file only if they don't (for example, if an API they call has moved
since 0257af0), and keep the three test names. Do not touch fe-git src/ or tests/remote.rs.
