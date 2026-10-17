---
task_uid: "01m54vs28tjst15shx0f60jh0k"
task: "T146"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test json_and_python_comment_tokens_match_vscode (tests/comment_tokens_vscode.rs, top level) passes: comment_tokens(\"json\") is (Some(\"//\"), Some((\"/*\", \"*/\"))) and comment_tokens(\"python\") is (Some(\"#\"), Some((\"\\\"\\\"\\\"\", \"\\\"\\\"\\\"\"))); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Every other language's tokens are unchanged, markdown_inline stays (None, None), and tests/comment_tokens.rs values changes only its json and python rows"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax -p fe-editor passes with no existing test edited but tests/comment_tokens.rs; cargo clippy -p fe-syntax --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Comment JSON with // and /* */ and give Python a """ block comment, as VS Code's language configurations do, so toggling a comment in a .json file works (Mac T773)

## Problem

At Mac main 8f606ec8 toggling a comment in a JSON file does nothing and Python has no block comment:
`crates/fe-syntax/src/comment.rs` `comment_tokens` gives json `(None, None)` and python `(Some("#"), None)`,
from T449's hand-written table. VS Code's language configurations (microsoft/vscode
`extensions/json/language-configuration.json` and `extensions/python/language-configuration.json`, read
2026-10-17) give JSON `lineComment: "//"`, `blockComment: ["/*", "*/"]` and Python `blockComment: ["\"\"\"",
"\"\"\""]`, so Cmd-/ in a `.json` file (tsconfig.json, settings files) adds `//` there. tree-sitter-json parses
`//` and `/* */` comments as `comment` nodes, so the result highlights as a comment.

Every other language in the table matches VS Code's lineComment and blockComment (checked the same day), except
Ruby's `=begin`/`=end`, which is left out: it must start a row, and fe-editor wraps a selection's rows in the block
pair inline.

## Change

- `crates/fe-syntax/src/comment.rs`: `"json"` gets `tokens(Some("//"), Some(("/*", "*/")))` (with c_like, or
  its own arm), and `"python"` gets `tokens(Some("#"), Some(("\"\"\"", "\"\"\"")))`; `markdown_inline` stays
  `(None, None)`. The doc says the table follows VS Code's language configurations, Ruby's block aside.
- `crates/fe-syntax/tests/comment_tokens.rs` `values`: its json and python rows change to the new tokens (the only
  existing test edit).

New integration test `crates/fe-syntax/tests/comment_tokens_vscode.rs`, top-level fn
`json_and_python_comment_tokens_match_vscode`: json is `(Some("//"), Some(("/*", "*/")))`, python is `(Some("#"),
Some(("\"\"\"", "\"\"\"")))`.
