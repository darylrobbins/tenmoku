---
task_uid: "01m6sp82wpp66rd4bc79fzdjwh"
task: "T298"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "[[contributes.grammars]] accepts line_comment, block_comment (exactly two non-empty strings) and shebangs, fe_syntax::GrammarSpec carries them, and the extension loader refuses malformed values"
    required: true
    manual: false
  - id: "C2"
    text: "fe_syntax::comment_tokens returns a registered language's tokens when no built-in has the name, and toggling a line comment in a buffer of a registered wasm language inserts its token through the editor's toggle-comment path (test)"
    required: true
    manual: false
  - id: "C3"
    text: "A file with no extension whose first line is #!/usr/bin/env \u003cname\u003e (or #!/usr/bin/\u003cname\u003e3) opens in the registered language declaring \u003cname\u003e; built-ins win, and a shebang a built-in claims is refused at registration (tests)"
    required: true
    manual: false
  - id: "C4"
    text: "A grammar without the keys behaves as before; cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Let extension grammars declare line and block comment tokens and shebang interpreters, so toggle-comment and shebang detection work for wasm languages (Mac T1434)

# Extension grammars declare comment tokens and shebang interpreters

Today a language an extension registers (`[[contributes.grammars]]`,
`crates/fastedit/src/config.rs` `ExtensionGrammar`, `deny_unknown_fields`) can
say its file extensions and file names only. Comment delimiters
(`fe_syntax::comment_tokens`, `crates/fe-syntax/src/comment.rs`) and shebang
detection (`language_name_for_shebang`, `crates/fe-syntax/src/detect.rs`) are
tables keyed by built-in language names, so a wasm language gets no
toggle-comment (`cmd-/`, vim `gc`) and a `#!/usr/bin/env elixir` script with no
extension opens as plain text. The first-party language cards (Dart, Elixir,
... ) need both.

## Add

- In `[[contributes.grammars]]` (and `fe_syntax::GrammarSpec`):
  - `line_comment = "//"` (optional string),
  - `block_comment = ["/*", "*/"]` (optional, exactly two non-empty strings),
  - `shebangs = ["elixir"]` (optional interpreter names, matched as the
    built-in table matches them: base name, `env` and `env -S` handled,
    version suffixes such as `python3.12` allowed).
- `comment_tokens(name)` returns a registered language's tokens when no
  built-in has the name. `CommentTokens` holds `&'static str`; registered
  languages are already `&'static Language`, so leak or intern alongside it.
- Shebang detection: built-ins first, then registered languages' `shebangs`.
  A registered interpreter that a built-in already claims is refused at
  registration as an extension or file name is (same error path).
- The JSON schema the extension loader checks follows the struct.
- The decision file documents the keys. Don't add them to the eight
  first-party extensions (their built-ins already have tokens): this card has
  no `extensions/grammars/**` scope and runs beside the discovery card.

## Tests

Through the real paths: a registered grammar with `line_comment`/`block_comment`
toggles comments through the same code `cmd-/` uses
(`fe-gpui` `comment_tokens()`, a headless editor test is enough), and a file
with no extension and a `#!/usr/bin/env <name>` first line picks the
registered language; one with no keys behaves as today. Reuse the
runtime-grammar test fixtures (`crates/fe-syntax/tests/runtime_grammar.rs`
loads a real wasm module) rather than new modules.
