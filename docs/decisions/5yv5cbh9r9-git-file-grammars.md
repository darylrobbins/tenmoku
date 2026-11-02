# Git file grammars (T1297)

- Four vendored path dependencies, shaped like T1282's `vendor/tree-sitter-gitignore`, each behind a default Cargo
  feature: `gitcommit`, `gitrebase`, `gitconfig`, `gitattributes`. Parsers are ABI 14 (gitrebase 15), no external
  scanner, not regenerated.
- gitcommit takes the-mikedavis' archived grammar over gbprod's, whose `parser.c` is 3.2 MB and whose crates.io 0.5.0
  is WTFPL. Its query is ours (`crates/fe-syntax/queries/gitcommit.scm`). Beyond the spec's list, `:`, `=` and `->` are
  `punctuation.delimiter` in a trailer or change line only; upstream's bare `":"` would also colour the colon of
  `# Changes to be committed:` inside a comment. `@user` mentions stay plain.
- Path tails: `PATH_SUFFIXES` in `language.rs`, matched per component and checked first, so a bare repo's
  `foo.git/config` and a plain `proj/config` stay plain text. Also T1282's `.git/info/exclude` and `.config/git/ignore`.
  Not covered: `.git/modules/<name>/config` and `.git/worktrees/<name>/config`.
- Golden harness: a sample with no detected language falls back to `language_by_name(file_stem)`. It also now skips
  dotfiles: `tests/golden/.gitattributes` (`* -text`, there since T2) is detected as a language once gitattributes
  exists, and isn't a sample.
- Size: 99,280 bytes of release binary, measured on a small example linking `fe-syntax` (release profile, macOS arm64)
  built with all default features and again without these four; the spec's estimate was about 100 KB.
- gitrebase takes the crate's query as the spec says, plus `crates/fe-syntax/queries/gitrebase.scm` after it (outside the
  card's listed scope; the review asked for it, since C3 wants each command highlighted and the crate's query alone
  doesn't do that). The vendored query stays unchanged. The override adds `update-ref`/`u`, `noop`, what `exec` runs, and
  `merge`/`m` without an option or a subject. It also makes the whole subject of `fixup -C <hash> <subject>` a comment,
  since the grammar reads its first word as a second label.
- gitrebase's sibling patterns (`(command) (label) (message)?`) don't make its query non-local; a window of a
  20,000-row todo highlights correctly in about 0.2 ms in a debug build.
- Left default: the parse-limit ratio (40). `gitconfig` folds `section`; the others fold nothing.
- `cargo deny check`: licenses, bans and sources pass, which is what the gate's `deny` stage runs. `advisories` fails on
  six RUSTSEC entries (wasmtime, wasmtime-wasi, pulldown-cmark, ctutils) that are on main already: this card's lockfile
  change only adds the four grammar crates. T1301 owns clearing them, so they aren't bumped here.
