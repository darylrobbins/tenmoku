---
task_uid: "01m54vs1qhyj2r5pbbfpqdc0zw"
task: "T145"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test markdown_fences_named_as_vscode_names_them_are_highlighted (tests/fence_language_vscode_names.rs, top level) passes: a Markdown fence named xhtml, shtml, phtml, php5, ddl, xsd, rss, xslt, es6, json5, py3, gyp, sbt, bashrc, rbx, Gemfile, bsh, js:app.js or python:main.py holds a highlight span starting with its sample keyword inside the fence; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "ALIASES holds every name the spec lists, lower case; injected_language cuts a name at its first , or :, and its doc says so; T745's tests/fence_language_aliases.rs passes as is"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax passes with no existing test edited; cargo clippy -p fe-syntax --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Highlight Markdown fences VS Code names xhtml, phtml, ddl, xsd, es6, json5, py3, sbt, bashrc, rbx, bsh and its other names for fastedit's languages, and fences like js:app.js (Mac T772)

## Problem

T745 (Mac) taught Markdown fences GitHub's common names (`golang`, `c#`, `python3`, …). VS Code's Markdown grammar
(microsoft/vscode-markdown-tm-grammar `build.ts`, its `languages` table, read 2026-10-17) highlights more names for
grammars fastedit has, and at Mac main 8f606ec8 these stay plain: `crates/fe-syntax/src/language.rs`
`injected_language` finds a fence name only as a language name, a file extension or an `ALIASES` entry.

- Names VS Code takes and fastedit doesn't: html `xhtml`, `shtml`; php `php3`, `php4`, `php5`, `phpt`, `phtml`,
  `ctp`; sql `ddl`, `dml`; xml `xsd`, `tld`, `rss`, `opml`, `xsl`, `xslt`; javascript `es6`; json `json5`; python
  `py3`, `rpy`, `cpy`, `gyp`, `gypi`, `sconstruct`, `sconscript`; scala `sbt`; bash `bashrc`, `bash_profile`,
  `bash_login`, `bash_logout`, `profile`; ruby `rbx`, `rjs`, `gemfile`, `rakefile`, `irbrc`, `capfile`,
  `vagrantfile`; java `bsh`; make `gnumakefile`.
- VS Code's rule ends the name at blanks, `:`, `,`, `{` or `?` (`(?i:(ids)((\s+|:|,|\{|\?)[^`]*)?$)`).
  tree-sitter-md's `language` node already stops at blanks, `{` and `,`, and `injected_language` cuts at `,`, but
  a `:` stays in the name, so ```` ```js:app.js ```` and ```` ```python:main.py ```` (VuePress and docs-site
  style) are unknown.

A prototype at 8f606ec8 (the aliases above plus a `:` cut) passed all of `cargo test -p fe-syntax`.

## Change

`crates/fe-syntax/src/language.rs`:

- Add the names above to `ALIASES` (lower case, as lookups lowercase the name), grouped by language after the
  existing entries, with the doc comment saying they are VS Code's Markdown grammar's names too.
- `injected_language` cuts the name at the first `,` or `:` (`split([',', ':'])`), and its doc says so.

New integration test `crates/fe-syntax/tests/fence_language_vscode_names.rs` (cfg on the features it needs, as
fence_language_aliases.rs), top-level fn `markdown_fences_named_as_vscode_names_them_are_highlighted`: for each of
`xhtml` `shtml` (`<p>x</p>`, `p`), `phtml` `php5` (`<?php if (1) {} ?>`, `if`), `ddl` (`SELECT 1;`), `xsd` `rss`
`xslt` (`<a/>`, `a`), `es6` (`const a = 1;`), `json5` (`[true]`), `py3` `gyp` (`def f(): pass`), `sbt`
(`object A`), `bashrc` (`if true; then :; fi`), `rbx` `Gemfile` (`def f; end`), `bsh` (`class C {}`), `js:app.js`
and `python:main.py`, a fence of that name holds a highlight span starting with the keyword inside the fence.
