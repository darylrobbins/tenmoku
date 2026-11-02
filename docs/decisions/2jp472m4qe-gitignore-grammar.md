# Gitignore highlighting (T1282)

- `vendor/tree-sitter-gitignore/` is shunsambongi/tree-sitter-gitignore at `f4685bf` (MIT), a plain path dependency
  rather than a `[patch.crates-io]` entry: crates.io has no upstream crate, and the third-party republish isn't taken.
- Its `parser.c` is language ABI 13, which tree-sitter 0.27 still loads, so it was not regenerated.
- Upstream has no highlight query; `queries/highlights.scm` is ours. Comments are `comment`; `!`, `*`, `**`, `?`, the
  bracket negation and range `-` are `operator`; `/` is `punctuation.delimiter`; `[` `]` are `punctuation.bracket`;
  `[:digit:]` classes are `constant.builtin`; escapes (`\#`, `\ `, `\/`) are `escape`.
- Detected by file name: `.gitignore`, `.dockerignore`, `.npmignore`, `.prettierignore`, `.eslintignore`,
  `.stylelintignore`, `.vscodeignore`, `.helmignore`, `.gcloudignore`, `.ignore`, `.rgignore`, `.fdignore`, and by the
  extension `gitignore` (the golden sample is `gitignore.gitignore`). Not `.hgignore`.
- Not done: `.git/info/exclude` and `~/.config/git/ignore`. `language_name_for_path` matches only the file name and
  extension, never a path tail, so they stay plain text until it can.
- The grammar needs a newline after every line; a last line without one parses with an error node.
- Size: the compiled `parser.c` is about 10 KB of text and under 1 KB of data (macOS arm64, `-O2`), so the release
  binary grows by about that much.
- Left default: the comment token (`#`) and parse-limit ratio (default 40) for this language, since those files
  (`comment.rs`, `parse_limit.rs`) are outside this card's scope.
