---
task_uid: "01m5556c61p64xjx0zjxezw739"
task: "T785"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git::stage_decomposed_names discarding_a_decomposed_name_restores_the_tracked_file passes: Repo::discard of the NFD path of a committed, edited file restores its committed text, as git checkout -- \u003cNFD\u003e does; it fails at the base (the file is deleted)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-git::stage_decomposed_names staging_a_decomposed_name_stages_it and fe-git::stage_decomposed_names unstaging_a_decomposed_name_unstages_it pass: Repo::stage leaves status 'M  café.txt' and Repo::unstage ' M café.txt', as git add and git reset -- \u003cNFD\u003e do; both fail at the base (Ok, nothing changed)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-git::stage_decomposed_names a_decomposed_index_name_without_precompose_still_stages passes: with core.precomposeUnicode false the NFD name is staged by its own bytes; the precompose runs only on macOS and only when the repo's core.precomposeUnicode is true"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-git passes with no existing test edited; cargo clippy -p fe-git --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-git: staging, unstaging and discarding a file whose name is decomposed (NFD) on disk act on it, as git add, reset and checkout do with core.precomposeUnicode, instead of doing nothing or deleting the tracked file

## Problem

On macOS many apps write file names decomposed (NFD: `cafe` + U+0301 `.txt`), and APFS keeps the name as
written, so a path fastedit gets from the tree or an editor can be NFD. A repository made by `git init` on
macOS has `core.precomposeUnicode = true`, and git then keeps index names precomposed (NFC, `caf` U+00E9
`.txt`) and precomposes the paths given on its command line (`precompose_argv`), so `git add <NFD>`,
`git reset -- <NFD>` and `git checkout -- <NFD>` all act on the file. It does not precompose paths read from
`--pathspec-from-file`: piping the NFD name to `git add -A --pathspec-from-file=- --pathspec-file-nul` exits
0 and stages nothing, and `git checkout` that way fails with "pathspec 'café.txt' did not match" (git 2.x,
checked by hand on this Mac).

fe-git sends every path through stdin since the many-paths work (crates/fe-git/src/stage.rs `nul_joined`,
used by `Repo::stage`, `Repo::unstage` and `Repo::discard`), and `Repo::discard` decides tracked vs untracked
by comparing the asked name byte for byte with `ls-files -z` output (`tracked_names`), which comes back NFC.
Reproduced at Mac main 3e3eaa03 with a committed NFD-named file, edited:

- `Repo::stage(&[root.join(NFD)])` returns Ok and `git status` still shows ` M café.txt` (nothing staged);
- `Repo::unstage` of the staged edit returns Ok and it stays `M  café.txt`;
- `Repo::discard` treats the tracked file as untracked and **deletes it from disk** (the file is gone,
  where `git checkout -- <NFD>` restores the committed text). That is data loss from Discard Changes.

## Change

In crates/fe-git/src/stage.rs, `Repo::relative_one` (which `relative` and `set_index_text` go through)
precomposes the relative name with `gix::utils::str::precompose_os_string` when the build is macOS
(`cfg(target_os = "macos")`) and the repository's `core.precomposeUnicode` is true (read from the repo's
config snapshot, default false, as git reads it), matching what git does to its argv. Nothing else changes:
`discard` still deletes untracked paths through the caller's own `PathBuf`, which APFS resolves either way.
A repository with `core.precomposeUnicode = false` keeps the name's bytes as given.

New test file crates/fe-git/tests/stage_decomposed_names.rs, `#![cfg(target_os = "macos")]`, with its own
small `git` helper (config shut out as tests/stage.rs does, `-c core.precomposeUnicode=true`) and a fixture
that commits an NFD-named file, then:

- `discarding_a_decomposed_name_restores_the_tracked_file`: edit, `discard`, the file reads its committed
  text;
- `staging_a_decomposed_name_stages_it`: edit, `stage`, status is `M  café.txt`;
- `unstaging_a_decomposed_name_unstages_it`: edit, `git add -A`, `unstage`, status is ` M café.txt`;
- `a_decomposed_index_name_without_precompose_still_stages`: with `core.precomposeUnicode = false` the
  index keeps the NFD name and `stage` of the NFD path stages it (passes at base too).

A prototype of exactly these three failing tests fails all three at 3e3eaa03 (discard: file `None`), and an
unconditional precompose in `relative_one` makes them and tests/stage.rs pass.
