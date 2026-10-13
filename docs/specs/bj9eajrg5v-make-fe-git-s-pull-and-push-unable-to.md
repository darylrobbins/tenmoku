---
task_uid: "01m4sjhajgcyzvrdbj9eajrg5v"
task: "T78"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "test recursive_push_runs_nothing_in_submodules (cargo test -p fe-git --test remote_signing): with push.recurseSubmodules=on-demand, and with submodule.recurse=true, push and publish are Err(RemoteError::Failed), sub.git's refs are unchanged, and the submodule's gpg.ssh.defaultKeyCommand, gpg.ssh.program and gpg.program sentinel never ran (F1)"
    required: true
    manual: false
  - id: "C2"
    text: "test pull_verifies_signatures (cargo test -p fe-git --test remote_signing): with merge.verifySignatures=true and B's commits SSH-signed by a key in A's allowed signers, pull(FastForwardOnly) and pull(Merge) are Ok and bring in B's commits (F9)"
    required: true
    manual: false
  - id: "C3"
    text: "test pull_never_signs (cargo test -p fe-git --test remote_signing): a divergent pull(Merge) with mergeOptions '\"-S\"' never runs gpg.program and leaves no MERGE_HEAD"
    required: true
    manual: false
  - id: "C4"
    text: "remote.rs sets no gpg.*.program override; pull passes --no-gpg-sign; every test in crates/fe-git/tests/remote.rs passes unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Make fe-git's pull and push unable to sign, so no signing prompt can appear, submodules included, while signature verification keeps working (T66 review F1, F9) (Mac T572 follow-up)

Follow-up from T66 (Mac T572, landed f756927), filed on helium. Review findings F1 (high, overridden at landing)
and F9 (medium) of review 01m4sa73xby2a8sc3twaesbh7g (`rgt review show T66`). The user's ruling on F1: fix it so a
signing prompt is impossible, including in submodules. F8 and F10 (the pull signing preflight) are the next card,
which depends on this one.

## Problem

`crates/fe-git/src/remote.rs` keeps signers from prompting by handing every git it runs
`-c gpg.program=false -c gpg.openpgp.program=false -c gpg.x509.program=false -c gpg.ssh.program=false`
(`NO_SIGNERS`, :80-89, added in `run_remote` :331). Two defects:

- F1 (remote.rs:88; also :149, :182, :192, :331). Git resolves a signing key before it runs the signer. A
  submodule with `push.gpgSign=true`, `gpg.format=ssh`, no `user.signingKey` and `gpg.ssh.defaultKeyCommand` set
  runs that key command first when `push.recurseSubmodules=on-demand` (or `submodule.recurse`) makes `push` /
  `publish` push the submodule. A key command such as `ssh-keygen -y -f <encrypted key>` can open SSH_ASKPASS
  and block the push. Our push.gpgSign preflight (`push_args`, :215) reads only the superproject's config.
  Review rounds on T66 patched this hole by hole; this card closes it by construction.
- F9 (remote.rs:80-88, :331). Git uses the same programs to verify signatures, so with
  `merge.verifySignatures=true` a pull of a valid, trusted signed upstream commit now fails, fast-forwards
  included.

## Change

All in `crates/fe-git/src/remote.rs`. Make git structurally unable to sign, rather than pointing the signers at
`false`:

- Pushes never run git in a submodule. `push` and `publish` pass `--recurse-submodules=check` whenever the
  effective config would push submodules: `push.recurseSubmodules` is `on-demand` or `only`, or it is unset and
  `submodule.recurse` is true (as git reads them; `-c`/env config counts because `config --get` sees it). A
  submodule commit not on its remote then makes the push `RemoteError::Failed` with git's message, and nothing in
  the submodule runs. Otherwise no `--recurse-submodules` flag is passed (git then doesn't recurse).
- The superproject's push keeps `--no-signed` and the `push.gpgSign` refusal in `push_args`, unchanged.
- Pulls never sign: `pull` always passes `--no-gpg-sign` (git pull hands it to the merge or rebase it runs, and a
  command-line `--no-gpg-sign` beats both `commit.gpgSign` and `branch.<name>.mergeOptions`). It keeps
  `--no-recurse-submodules`. The `signs_commits` preflight and its refusal stay as they are (the next card fixes
  their accuracy); from here on a preflight miss makes an unsigned commit, never a signer run.
- Delete `NO_SIGNERS` and stop prepending it in `run_remote`, so verification (`merge.verifySignatures`,
  `pull --verify-signatures`) uses the user's real programs.
- Rewrite the module doc (:6-13) and the doc comments of `pull`, `push`, `publish`, `push_args` to say this:
  nothing here signs because pulls pass `--no-gpg-sign`, pushes `--no-signed`, and pushes never recurse into a
  submodule; verification works as configured. Hooks are the user's and out of scope.

## Tests

New file `crates/fe-git/tests/remote_signing.rs`, self-contained (copy the isolated-config `git`/`remotes`
helpers and the `sentinel_signer` pattern from `crates/fe-git/tests/remote.rs`; don't edit that file). Unix only
(`#[cfg(unix)]`) where a sentinel script is used.

- `recursive_push_runs_nothing_in_submodules`: superproject A with a submodule `sub` (bare `sub.git` with
  `receive.certNonceSeed` set, as `submodules_never_sign_or_stash` builds it). In A's submodule:
  `push.gpgSign=true`, `gpg.format=ssh`, no `user.signingKey`, `gpg.ssh.defaultKeyCommand` and `gpg.ssh.program`
  and `gpg.program` all a sentinel script that records it ran. A commits in the submodule and records it in the
  superproject. With `push.recurseSubmodules=on-demand`, then (unset) with `submodule.recurse=true`: `push(false)`
  on main and `publish("origin")` on a new branch are each `Err(RemoteError::Failed { .. })`, `sub.git`'s refs are
  unchanged, and the sentinel never ran. A second submodule with no signing config at all is also not pushed.
- `pull_verifies_signatures`: skip with a printed message when `ssh-keygen` is missing. B signs a commit with a
  passphrase-less ed25519 key made by `ssh-keygen -N ''` (`gpg.format=ssh`, `user.signingKey`,
  `commit.gpgSign=true`) and pushes; A has `gpg.format=ssh`, `gpg.ssh.allowedSignersFile` listing that key, and
  `merge.verifySignatures=true`. `A.pull(PullMode::FastForwardOnly)` is `Ok` and A's HEAD is B's commit; after a
  second signed commit from B, `A.pull(PullMode::Merge)` is `Ok` too.
- `pull_never_signs`: A's `gpg.program` is the sentinel, `commit.gpgSign=false` and
  `branch.main.mergeOptions` is `"-S"` with the quotes (which today's preflight misses). A and B diverge;
  `A.pull(PullMode::Merge)`: whatever it returns, the sentinel never ran and `.git/MERGE_HEAD` doesn't exist.
  (Assert nothing about Ok vs Err: the next card makes this case a refusal.)

## Implementation notes

- `push_recurses` reads `push.recurseSubmodules` with `config --get` (last value wins, `-c`/env included) and
  matches `on-demand`/`only` exactly, as git's parser does; when it's unset, `submodule.recurse` decides, read as
  a bool. `check` and `no` pass no flag. With `check`, git only lists submodule commits in each submodule
  (`rev-list`), which reads no signing config.
- All three new tests fail against the previous `remote.rs`; `recursive_push_runs_nothing_in_submodules` also
  fails its `submodule.recurse=true` half when only `push.recurseSubmodules` is consulted.
- Review round 1 (amspm8): F1, git applies `push.recurseSubmodules` and `submodule.recurse` in config order, so
  `push_recurses` reads both with `config -z --get-regexp` (all scopes, env and `-c` last) and lets the last decide;
  every push now passes an explicit `--recurse-submodules=check` or `=no`, so a misread can only choose between
  checking and not recursing, never let git recurse. F2, `pull` also runs as `git -c commit.gpgSign=false pull`, so
  the `git am` of an apply-backend rebase (which reads `commit.gpgSign` itself) can't sign. Regressions:
  `recursive_push_runs_nothing_in_submodules` (both orders), `recursive_push_obeys_env_config`,
  `pull_rebase_apply_never_signs`.
