---
task_uid: "01m4sjheeevkagstesf3gezpzz"
task: "T79"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "test rebase_ignores_merge_options (cargo test -p fe-git --test pull_signing_policy): commit.gpgSign=true with mergeOptions=--no-gpg-sign refuses pull(Rebase) with HEAD, worktree and operation state unchanged; commit.gpgSign=false with mergeOptions=-S rebases (F8)"
    required: true
    manual: false
  - id: "C2"
    text: "test merge_options_parse_like_git (cargo test -p fe-git --test pull_signing_policy): quoted, abbreviated, clustered, =key and unterminated signing options refuse pull(Merge) with nothing changed; --no-gpg, \"--no-gpg-sign\" and '-S --no-gpg-sign' under commit.gpgSign=true merge (F10)"
    required: true
    manual: false
  - id: "C3"
    text: "signing_is_refused in crates/fe-git/tests/remote.rs expects the mergeOptions refusals for PullMode::Merge only, and passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Refuse exactly the pulls that would sign in fe-git: a rebase reads only commit.gpgSign, and branch mergeOptions are parsed as git parses them (T66 review F8, F10) (Mac T572 follow-up)

Follow-up from T66 (Mac T572, landed f756927), filed on helium: review findings F8 and F10 (medium) of review
01m4sa73xby2a8sc3twaesbh7g (`rgt review show T66`). Depends on the F1/F9 card, which makes every pull pass
`--no-gpg-sign`; this card makes the refusal that honours the user's signing config accurate.

## Problem

`Repo::signs_commits` (`crates/fe-git/src/remote.rs` :196-209) decides whether a divergent pull would sign, so
`pull` can refuse it before anything changes (:132, :142-147, :158-162). It is wrong both ways:

- F8 (remote.rs:132, :198; tests/remote.rs:470). It applies `branch.<name>.mergeOptions` to `PullMode::Rebase`
  too, but git's rebase ignores mergeOptions and reads `commit.gpgSign` itself. `commit.gpgSign=true` with
  `mergeOptions=--no-gpg-sign` lets a rebase through that should be refused (it ran into the disabled signer
  and left a rebase in progress at T66; after the F1/F9 card it would rebase unsigned against the user's
  config). `commit.gpgSign=false` with `mergeOptions=-S` refuses an unsigned rebase for no reason, and
  `signing_is_refused` in `crates/fe-git/tests/remote.rs` asserts that wrong refusal (the "mergeOptions -S" case
  looped over `[PullMode::Merge, PullMode::Rebase]`).
- F10 (remote.rs:201-202). `split_whitespace` and prefix checks don't parse mergeOptions as git does (git uses
  `split_cmdline` then `parse_options`, builtin/merge.c): a quoted `'-S'` or `"--gpg-sign"`, an abbreviation
  such as `--gpg`, and a cluster such as `-nS` all sign in git but are missed here.

## Change

In `crates/fe-git/src/remote.rs`, `signs_commits(&self, branch, mode)`:

- `PullMode::Rebase`: `commit.gpgSign` (as git reads a boolean) and nothing else.
- `PullMode::Merge`: start from `commit.gpgSign`, then walk `branch.<name>.mergeOptions` tokens in order, later
  ones overriding earlier ones, as git does:
  - Tokenize like git's `split_cmdline`: whitespace separates; single quotes take everything literally up to the
    next single quote; double quotes allow `\` escapes; `\` outside quotes escapes the next character. An
    unterminated quote means the policy can't be determined: count it as signing.
  - `--gpg-sign`, `--gpg-sign=<key>`, or any abbreviation of `--gpg-sign` at least `--g` long (with or without
    `=<key>`): signs. `--no-gpg-sign` or any abbreviation of it at least `--no-g` long: doesn't sign.
  - A single-dash token (`-x...`, not `--`) containing `S` anywhere: signs. This is conservative on purpose: the
    `S` may be the value of `-s`, `-X` or `-m`, and refusing then is acceptable.
- `PullMode::FastForwardOnly`: never signs (unchanged).
- A refused pull changes nothing: HEAD, index and worktree as before, no `.git/MERGE_HEAD`, no
  `.git/rebase-merge` or `.git/rebase-apply` (the existing `--ff-only` downgrade gives this; keep it).
- Doc comment of `signs_commits` and `pull` say which config each mode reads.

In `crates/fe-git/tests/remote.rs`, `signing_is_refused`: the "mergeOptions -S" and "mergeOptions --gpg-sign" cases
expect refusal for `PullMode::Merge` only; nothing else in that file changes.

## Tests

New file `crates/fe-git/tests/pull_signing_policy.rs`, self-contained (copy the isolated-config helpers from
`crates/fe-git/tests/remote.rs`). A and B are clones of a bare origin and diverge before each pull; after each
refusal assert `Err(RemoteError::Failed { output })` with "signing" in `output`, HEAD unchanged, `git status
--porcelain` empty, and no MERGE_HEAD, rebase-merge or rebase-apply.

- `rebase_ignores_merge_options`: `commit.gpgSign=true` + `mergeOptions=--no-gpg-sign`: `pull(Rebase)` refused;
  `commit.gpgSign=false` + `mergeOptions=-S`: `pull(Rebase)` is `Ok` and A's commit is rebased onto B's.
- `merge_options_parse_like_git`: with `commit.gpgSign=false`, each of `'-S'`, `"--gpg-sign"`, `--gpg`,
  `--gpg-sign=key`, `-nS`, `--no-ff '-S` (unterminated) makes `pull(Merge)` refused; with `commit.gpgSign=true`,
  each of `--no-gpg`, `"--no-gpg-sign"`, and `-S --no-gpg-sign` makes `pull(Merge)` `Ok` with a merge commit
  whose second parent is B's HEAD.
