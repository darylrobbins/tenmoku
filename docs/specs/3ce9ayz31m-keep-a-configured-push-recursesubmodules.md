---
task_uid: "01m4sr2wa4n5zkxs3ce9ayz31m"
task: "T92"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "test merge_option_values_and_dashdash_like_git (cargo test -p fe-git --test pull_signing_policy): with commit.gpgSign=false, each of \"-S -m --no-gpg\", \"-S -- --no-gpg\", \"-S --message --no-gpg-sign\" and \"-S -nm --no-gpg-sign\" makes a divergent pull(Merge) Failed with \"signing\" and changes nothing (assert_refused), and \"-mS\" merges; with commit.gpgSign=true, \"-m msg --no-gpg-sign\" merges (HEAD^2 is B HEAD)"
    required: true
    manual: false
  - id: "C2"
    text: "test merge_options_split_on_git_whitespace (cargo test -p fe-git --test pull_signing_policy): with commit.gpgSign=false, --gpg-sign=key then U+00A0, and --gpg-sign=key then U+000B, each followed by --no-gpg-sign with no ASCII separator, make pull(Merge) refused with nothing changed; with commit.gpgSign=true, --gpg-sign=key\u003cTAB\u003e--no-gpg-sign merges"
    required: true
    manual: false
  - id: "C3"
    text: "split_cmdline in crates/fe-git/src/remote.rs splits only on space, tab, LF and CR; rebase_ignores_merge_options, merge_options_parse_like_git and every test in crates/fe-git/tests/remote.rs pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "test push_keeps_submodule_check (cargo test -p fe-git --test remote_signing): with push.recurseSubmodules=check, and with submodule.recurse=false then push.recurseSubmodules=check, push(false) and publish(\"origin\") are Err(RemoteError::Failed) while sub and plain hold commits their remotes lack; sub.git and plain.git refs are unchanged and the sentinel never ran (F3)"
    required: true
    manual: false
  - id: "C5"
    text: "test push_keeps_submodule_check: with push.recurseSubmodules=check then submodule.recurse=false, push and publish are Ok, submodule refs are unchanged and the sentinel never ran; push_args passes only --recurse-submodules=check or =no, never on-demand or only"
    required: true
    manual: false
  - id: "C6"
    text: "recursive_push_runs_nothing_in_submodules, recursive_push_obeys_env_config and every test in crates/fe-git/tests/remote.rs pass unchanged"
    required: true
    manual: false
  - id: "C7"
    text: "cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Refuse every divergent pull that git would sign in fe-git: option values, -- and non-ASCII whitespace in mergeOptions cannot turn signing off, and pin push.recurseSubmodules=check in push and publish (T79 review F1, F2; T78 F3) (Mac T572 follow-up)

{
  "mode": "inline",
  "body": "Follow-up from T79 (Mac T572 follow-up, landed 529d9c8): review findings F1 and F2 (codex, medium) of review\n01m4stsg953gamw3t86fxwt3g3 (`rgt review show T79`).\n\n## Problem\n\n`pull` (`crates/fe-git/src/remote.rs`) refuses a divergent pull that would sign, so fe-git never makes a commit\nthe user's config says to sign. `Repo::signs_commits` (:200-219) decides that. For `PullMode::Merge` it runs\n`branch.<name>.mergeOptions` through `split_cmdline` (:413) and lets each token's `merge_option_signs` (:466)\noverride signing. When that says \"doesn't sign\" but git would sign, `pull` doesn't downgrade to `--ff-only`: it\nruns `git -c commit.gpgSign=false pull --no-gpg-sign --no-rebase --ff`, git makes an **unsigned merge commit**\non the branch, and `pull` returns `Ok`. No signing prompt appears (T78's `--no-gpg-sign` sees to that), but the\nuser's signing policy is silently broken. It should be `RemoteError::Failed` (\"commit signing: …\") with HEAD,\nindex and worktree unchanged and no MERGE_HEAD. Two ways it happens:\n\n- **F1** (remote.rs:213). Every token is treated as an option. git's `parse_options` (builtin/merge.c\n  `builtin_merge_options`, git v2.53.0) gives option arguments and everything after `--` no option meaning.\n  `-S -m --no-gpg` (`--no-gpg` is the message) and `-S -- --no-gpg` sign in git; here `--no-gpg` clears\n  signing.\n- **F2** (remote.rs:420). `split_cmdline` splits on `char::is_whitespace` (Unicode: also U+000B, U+000C,\n  U+0085, U+00A0, …). git's `split_cmdline` (alias.c) splits on `isspace`, and git's ctype table (ctype.c)\n  counts only space, tab, LF and CR. `--gpg-sign=key` + U+00A0 + `--no-gpg-sign` is one token in git (a\n  signing key), so git signs; here it's two tokens and the second clears signing.\n\nChecked against git 2.53.0: each mergeOptions value in the Tests section behaves in `git merge` as the test\nexpects.\n\n## Change\n\nIn `crates/fe-git/src/remote.rs`:\n\n- `split_cmdline`: whitespace is exactly `' '`, `'\\t'`, `'\\n'`, `'\\r'`. Any other character, U+000B, U+000C\n  and U+00A0 included, is part of the word. Fix its doc comment to say so.\n- `signs_commits`, `PullMode::Merge`: walk the tokens in order with an index, as git's `parse_options` reads\n  `builtin_merge_options`:\n  - `--` ends options: ignore it and every token after it.\n  - A long token `--<name>` with no `=`, where `<name>` is a non-empty prefix of `strategy`,\n    `strategy-option`, `message`, `file`, `cleanup` or `into-name` (the long options that take a required\n    argument), takes the next token as its value: skip that token. (An ambiguous prefix such as `--s` makes\n    git fail the merge, so how it's read doesn't matter.) `--<name>=<value>` takes nothing more.\n  - A single-dash token (`-x…`, not `-` or `--`): walk its characters after the `-`. `S` signs, and the rest\n    of the token is its key: stop. `s`, `X`, `m` or `F` takes the rest of the token as its value, or, when\n    it is the last character, the next token: skip that token and stop. Any other character takes nothing\n    (`n`, `e`, `v`, `q`). This replaces \"contains `S` anywhere\": `-mS` is the message `S` and doesn't sign,\n    as in git; `-nS` still signs.\n  - Otherwise as now: `--gpg-sign` and its abbreviations from `--g` (with or without `=<key>`) sign;\n    `--no-gpg-sign` and its abbreviations from `--no-g` don't; `-S` and `--gpg-sign` never take the next\n    token (their key is optional and only attached). Other tokens say nothing.\n  - An unterminated quote or trailing `\\` still counts as signing.\n- Fold the per-token logic into whatever shape fits (`merge_option_signs` may become the walk itself); keep\n  it to this table. No general option parser. Update the doc comments of `signs_commits` and the helper.\n- `PullMode::Rebase` and `PullMode::FastForwardOnly`, `pull`, and the push code are unchanged.\n\n## Tests\n\nIn `crates/fe-git/tests/pull_signing_policy.rs` (exists at base), two new tests using the file's `isolated`,\n`remotes`, `diverge`, `set` and `assert_refused` helpers, one diverge per case as `merge_options_parse_like_git`\ndoes. \"Refused\" is `assert_refused` on `pull(PullMode::Merge)`; \"merges\" is `pull(PullMode::Merge)` `Ok` with\n`HEAD^2` equal to B's HEAD.\n\n- `merge_option_values_and_dashdash_like_git` (F1): with `commit.gpgSign=false`, each of `-S -m --no-gpg`,\n  `-S -- --no-gpg`, `-S --message --no-gpg-sign` and `-S -nm --no-gpg-sign` is refused, and `-mS` merges;\n  with `commit.gpgSign=true`, `-m msg --no-gpg-sign` merges.\n- `merge_options_split_on_git_whitespace` (F2): with `commit.gpgSign=false`, `--gpg-sign=key\\u{a0}--no-gpg-sign`\n  and `--gpg-sign=key\\u{b}--no-gpg-sign` are refused; with `commit.gpgSign=true`,\n  `--gpg-sign=key\\t--no-gpg-sign` merges.\n\nBoth fail at base (the base merges unsigned where these expect a refusal). The existing\n`rebase_ignores_merge_options` and `merge_options_parse_like_git` stay as they are and pass.",
  "ref": "",
  "ref_blob": ""
}

## Also: pin push.recurseSubmodules=check in push and publish (T78 review F3, coverage only)

This task first carried T78 review F3 (filed as T92's own spec): that a configured `push.recurseSubmodules=check`
was dropped by `push` and `publish`. Its worker found the premise false at base: `push_recurses`
(crates/fe-git/src/remote.rs) already counts `check` as recursing (`value == "check" || parse_bool(value)`), so
push and publish already pass `--recurse-submodules=check` and refuse unpublished submodule commits. There is no
bug to fix there, so `push_keeps_submodule_check` stays as a coverage test, not a regression test. Renaming
`push_recurses` to say what it decides (for example `push_checks_submodules`) and correcting its doc comment are
in scope; nothing else about push changes.

The lead folded T94 (T79 review F1, F2) into this task on 2026-10-13 because the worker had already made those
changes in the same file. T94 is abandoned in favour of this one. The F1/F2 work above is the bugfix; its two
tests are this task's regression tests and must fail at base.

### Push test (from the original T92)

`push_keeps_submodule_check` in `crates/fe-git/tests/remote_signing.rs`: with `push.recurseSubmodules=check`, and
with `submodule.recurse=false` then `push.recurseSubmodules=check`, `push(false)` and `publish("origin")` fail
while a submodule holds commits its remote lacks, with no remote ref changed and no submodule command run; with
`push.recurseSubmodules=check` then `submodule.recurse=false`, both succeed.
