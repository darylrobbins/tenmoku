---
task_uid: "01m6c9qjz51167715yv5cbh9r9"
task: "T1297"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "vendor/tree-sitter-git-commit, -git-rebase, -git-config and -gitattributes each hold the upstream MIT grammar at the commit the body pins, with its LICENSE and a README saying how to update it; Cargo.lock has no tree-sitter 0.20 and no gbprod or konomanoasa grammar crate"
    required: true
    manual: false
  - id: "C2"
    text: "crates/fe-syntax/tests/git_file_names.rs: COMMIT_EDITMSG, MERGE_MSG, TAG_EDITMSG, SQUASH_MSG, NOTES_EDITMSG and EDIT_DESCRIPTION are gitcommit; git-rebase-todo is gitrebase; .gitconfig, .gitmodules, repo/.git/config and home/.config/git/config are gitconfig; .gitattributes, repo/.git/info/attributes and home/.config/git/attributes are gitattributes; repo/.git/info/exclude is gitignore; proj/config is None"
    required: true
    manual: false
  - id: "C3"
    text: "tests/golden/gitcommit, gitrebase, gitconfig and gitattributes have .golden files showing a commit's subject, comments, trailers and change list, each rebase command, hash and message, gitconfig sections, subsections, keys, booleans and includeIf, and gitattributes patterns, set/unset/valued attributes and macros highlighted"
    required: true
    manual: false
  - id: "C4"
    text: "each_language_highlights_a_snippet, candidates_per_language, every_listed_language_has_tokens and every_language_has_an_entry pass with the four languages"
    required: true
    manual: false
  - id: "C5"
    text: "THIRD_PARTY.md lists the four grammars with pin and licence; cargo deny check licenses bans sources passes (the six RUSTSEC advisories already on main are T1301's); cargo nextest run -p fe-syntax passes"
    required: true
    manual: false
---

# Highlight git commit messages, rebase todos, git config and .gitattributes with vendored upstream grammars, detecting .git/config-style paths by their tail

## Problem

Git's own files open as plain text: commit messages (`COMMIT_EDITMSG`, with Tenmoku as `GIT_EDITOR`), the
interactive rebase todo, `.gitconfig` / `.git/config`, `.gitmodules` and `.gitattributes`. The user approved
built-in highlighting for them on 2026-11-01 ("small everyday file formats"). T1282 does ignore files the same
way; this card follows its pattern (vendored upstream grammar, pinned, `vendor/<grammar>/README.md` saying how
to update).

Language detection (`crates/fe-syntax/src/language.rs` `language_name_for_path`) matches only a file name or an
extension, so `.git/config`, `~/.config/git/config` and `.git/info/attributes` (name `config` / `attributes`)
can't be told apart today.

The golden harness (`crates/fe-syntax/src/tests/golden.rs`) needs each sample `<language>.<ext>` to be detected
by its path; commit messages and rebase todos have no extension.

## Change

Four languages, each one Cargo feature (on by default) in `crates/fe-syntax/Cargo.toml`, a `Def` in
`language.rs`, and a vendored grammar crate shaped like T1282's `vendor/tree-sitter-gitignore` (generated
`src/parser.c`, `queries/highlights.scm`, `LICENSE`, a minimal `Cargo.toml` with a `cc` build.rs exposing
`LANGUAGE` and `HIGHLIGHTS_QUERY` through `tree-sitter-language`, README with the pinned commit). None has a
usable crates.io crate: the git-commit, git-rebase and git-config ones are unpublished (git-config's own
manifest pins tree-sitter ~0.20); `tree-sitter-gitattributes` 0.1.6 on crates.io (tree-sitter-grammars) needs
tree-sitter ~0.20. All four parsers are ABI 14/15, no external scanner, so they load under tree-sitter 0.27
without regenerating.

| Language | Upstream (MIT) | Pin | Object size (clang -Os) |
|---|---|---|---|
| `gitcommit` | the-mikedavis/tree-sitter-git-commit | `5a50da19b3841ac51e9d483cd9c856a85232233d` | ~69 KB |
| `gitrebase` | the-mikedavis/tree-sitter-git-rebase | `32686d6b72980b36f876ae2d07719c9c3ed154e2` | ~4 KB |
| `gitconfig` | the-mikedavis/tree-sitter-git-config | `3a61756a81a86291a0f48e3eeeaa0692b9981aa9` | ~9 KB |
| `gitattributes` | tree-sitter-grammars/tree-sitter-gitattributes | `1b7af09d45b579f9f288453b95ad555f1f431645` | ~17 KB |

About 100 KB of release binary in all. The-mikedavis git-commit is archived and its README defers to
gbprod/tree-sitter-gitcommit; we take it anyway because gbprod's parser.c is 3.2 MB (about 1.6 MB of object
code, mostly localised status headers) and its crates.io 0.5.0 is WTFPL. Say so in the vendor README.

Files (vendor dirs: `vendor/tree-sitter-git-commit`, `-git-rebase`, `-git-config`, `-gitattributes`):

- `gitcommit`: file names `COMMIT_EDITMSG`, `MERGE_MSG`, `TAG_EDITMSG`, `SQUASH_MSG`, `NOTES_EDITMSG`,
  `EDIT_DESCRIPTION`. Its upstream query uses placeholder captures (`@subject`, `@plus`…), so it gets our own
  `crates/fe-syntax/queries/gitcommit.scm` in their place: subject `markup.heading`, comments `comment`,
  trailer key `property` and value `string`, paths `string.special`, branch `label`, commit hash `constant`,
  issue items `markup.link`, change kinds `keyword`, scissors `punctuation.special`.
- `gitrebase`: file name `git-rebase-todo`; the crate's query.
- `gitconfig`: file names `.gitconfig`, `.gitmodules`; path tails `.git/config`, `.config/git/config`; the
  crate's query (`string.special.path` falls back to `string.special`).
- `gitattributes`: file name `.gitattributes`; path tails `.git/info/attributes`, `.config/git/attributes`;
  the crate's query.

Path tails: add `const PATH_SUFFIXES: &[(&str, &str)]` (tail, language) to `language.rs`, matched component by
component (ASCII case-insensitive, like file names) against the end of the path, checked before file names and
extensions, and skipped when that language is not compiled in. Add T1282's `gitignore` tails there too
(`.git/info/exclude`, `.config/git/ignore`) if T1282 left them out.

Golden harness: when a sample's path detects no language, fall back to `language_by_name(file_stem)`, so the
samples are `tests/golden/gitcommit`, `gitrebase`, `gitconfig`, `gitattributes` (no extension) with their
`.golden` files. A sample for a language left out of the build still skips.

Per-language tables every language must be in: `comment.rs` (`#` line comments for all four; `auto_pairs`:
gitcommit takes markdown's pairs so apostrophes type plainly, the other three the dockerfile/make row) and
`tests/auto_pairs.rs`'s expected table; a case in `src/tests.rs` `each_language_highlights_a_snippet`; a sample
in `src/fold/tests.rs` `CASES` (gitconfig folds `section` via a `KINDS` entry in `fold.rs`; the others fold
nothing). Parse limits keep the default ratio. Status-bar labels come in the data-formats card.

Record the four grammars in `THIRD_PARTY.md` (table row, pinned commit, licence; the gitcommit query is ours),
and note the measured release-binary growth in the decision file.

## Implementation notes

See `docs/decisions/5yv5cbh9r9-git-file-grammars.md`: the measured size, the golden harness skipping
`tests/golden/.gitattributes`, and the gaps in the crate's gitrebase query.
