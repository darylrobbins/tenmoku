---
task_uid: "01m548ncvpwf80fmrmrv0fbkrg"
task: "T745"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "markdown_fences_named_by_common_aliases_are_highlighted passes: fences named golang, Golang, c#, C#, python3, docker, containerfile, makefile, Makefile, shell-script, node, javascriptreact, typescriptreact and jsonc highlight their code as go, csharp, python, dockerfile, make, bash, javascript, tsx and json"
    required: true
    manual: false
  - id: "C2"
    text: "The existing fence cases in injection::tests::markdown_inline_and_fenced_code_are_highlighted (JS, py, c++, shell, rust,ignore) and injection::tests::unknown_injected_languages_stay_plain pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "The workspace gate (nextest and clippy -D warnings) passes and only the two scoped files change"
    required: true
    manual: false
---

# Highlight Markdown fences named golang, c#, python3, docker, makefile and the other common GitHub and VS Code language aliases

## Problem

A Markdown fence names its language by the info string's first word, and
`injected_language` in `crates/fe-syntax/src/language.rs` resolves it, ignoring
case, by a language's name, then one of its file extensions, then `ALIASES`.
`ALIASES` holds only `("shell", "bash")`, so fence names GitHub (linguist) and
VS Code accept for languages fastedit has resolve to `Injected::Unknown` and the
fence stays plain.

Reproduced at c092189b with a probe integration test that compiles the target
language, parses ```` ```<name>\n<source>\n``` ```` as markdown and looks for a
`keyword` span (`Syntax::highlights`):

- plain (FAIL): `golang`, `Golang`, `c#`, `C#`, `python3`, `docker`,
  `containerfile`, `makefile`, `Makefile`, `shell-script`, `node`,
  `javascriptreact`, `typescriptreact`
- highlighted (ok, for contrast): `go`, `cs`, `csharp`, `py`, `dockerfile`,
  `Dockerfile`, `make`, `mk`, `sh`, `shell`, `zsh`, `js`, `jsx`, `ts`, `tsx`,
  `rs`, `cpp`, `c++`, `kt`, `kotlin`, `rb`

`yml`, `md`, `jsonc` were not probed by keyword; `yml`/`md` resolve by extension.
`jsonc` (linguist's "JSON with Comments") is neither a name nor an extension, so it
is plain too. `objc`, `ps1`/`pwsh` and `console` have no grammar in this build and
stay plain on purpose.

## Change

Extend `ALIASES` in `crates/fe-syntax/src/language.rs` (keys lowercase, as
`injected_language` lowercases first):

- `golang` → `go`
- `c#` → `csharp`
- `python3` → `python`
- `docker`, `containerfile` → `dockerfile`
- `makefile` → `make`
- `shell-script` → `bash`
- `node`, `javascriptreact` → `javascript`
- `typescriptreact` → `tsx`
- `jsonc` → `json`

Update the doc comment on `ALIASES` ("other names injections use", now the
GitHub linguist / VS Code fence names). Nothing else changes: an alias to a
language whose Cargo feature is off still resolves to `Unknown`, because
`injected_language` looks the canonical name up in `DEFS`.

Add the regression test as a new integration test
`crates/fe-syntax/tests/fence_language_aliases.rs` (public API only:
`language_by_name`, `Syntax::new`, `parse_job().run()`, `finish`, `highlights`;
gate it with `#![cfg(all(feature = "markdown", feature = "go", …))]` for the
languages it uses). Test
`markdown_fences_named_by_common_aliases_are_highlighted` compiles each target
language first (`language_by_name`), then for each alias above (and `Golang`,
`C#`, `Makefile` for case) parses a one-fence markdown document and asserts a
keyword in the fence has the `keyword` highlight (`func`, `class`, `def`,
`FROM`, `ifeq`, `if`, `const`; for `jsonc`, `true` as `constant.builtin`).
