---
task_uid: "01m4r179xmkw3511pepbvaq7rm"
task: "T30"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-config test editorconfig::sections_and_lookup: with a root .editorconfig ([*] indent_style = space, indent_size = 4; [*.go] indent_style = tab; [Makefile] indent_style = tab) and a nested one setting [*.js] indent_size = 2, a.go gives Tab, sub/x.js gives Space with 2 columns, and a .editorconfig above a root = true one is ignored"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test editorconfig::values_and_defaults: end_of_line crlf, insert_final_newline true and trim_trailing_whitespace false come through; indent_size = tab uses tab_width; indent_size 3 with no tab_width gives tab_width 3; unset and invalid values are None"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test editorconfig::broken_files_are_skipped: an unreadable .editorconfig is skipped with the others still applied"
    required: true
    manual: false
  - id: "C4"
    text: "cargo deny check, cargo test -p fe-config and cargo clippy -p fe-config --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Read a file's .editorconfig properties in fe-config with ec4rs: indent style and size, tab width, line ending, final newline and trailing whitespace (Mac T467)

Filed on helium from Mac T467 (~/coordination/helium-cards/T467.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-l.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-12) asked for `.editorconfig` support: indent style and size, line endings, a final newline and
trimming trailing whitespace, with EditorConfig winning over T458's indentation detection. Nothing reads
`.editorconfig` files. Matching its sections needs EditorConfig's own glob dialect (`**`, `{a,b}`, `{1..3}`, `[...]`,
paths relative to each file's directory) and its lookup rules (every `.editorconfig` from the file's directory up,
stopping at `root = true`, nearer files winning).

## Change

Depend on `ec4rs` 1.2 (Apache-2.0, no dependencies, follows the EditorConfig core tests): a workspace dependency in
`Cargo.toml` and fe-config's `Cargo.toml`; `cargo deny check` must pass, and the dependency goes in THIRD_PARTY.md
only if that file lists plain crate dependencies (it lists copied code and grammars).

New module `crates/fe-config/src/editorconfig.rs` (`mod editorconfig;` and exports in lib.rs), GPUI-free:

- `pub struct EditorConfig { pub indent_style: Option<IndentKind>, pub indent_size: Option<IndentSize>, pub
  tab_width: Option<usize>, pub end_of_line: Option<EndOfLine>, pub insert_final_newline: Option<bool>, pub
  trim_trailing_whitespace: Option<bool> }` with `IndentKind::{Tab, Space}`, `IndentSize::{Columns(usize), Tab}`,
  `EndOfLine::{Lf, Crlf, Cr}`. `unset`, missing and invalid values are `None`.
- `pub fn editorconfig_for(path: &Path) -> EditorConfig` reads the files with ec4rs and fills the fields with
  EditorConfig's defaults applied: `indent_size = tab` uses `tab_width`; `indent_style = tab` with no `indent_size`
  gets `IndentSize::Tab`; `indent_size` with no `tab_width` sets `tab_width` to it. A `.editorconfig` that can't be
  read or parsed is skipped with one `log::warn!` naming it, and the rest still apply.
- `charset` and other properties are ignored.

## Tests

New file `crates/fe-config/tests/editorconfig.rs`, on temp directories.
