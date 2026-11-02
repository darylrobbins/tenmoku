---
task_uid: "01m6c9qm6xcxr3cx3tvvbe38fz"
task: "T1299"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "x.diff and x.patch are diff; the diff.diff golden shows added lines and +++ as markup.inserted and removed lines and --- as markup.deleted, with hunk locations and commit hashes highlighted; every theme in crates/fe-config/themes styles markup.inserted and markup.deleted"
    required: true
    manual: false
  - id: "C2"
    text: "vendor/tree-sitter-json5 holds Joakker/tree-sitter-json5 at 248b856 with LICENSE and README; x.json5 is json5; the json5.json5 golden shows quoted and unquoted keys as string.special.key, numbers as number and comments as comment; a json5 Markdown fence highlights with json5"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fe-syntax/tests/data_formats.rs: a tsconfig.json text with // and /* */ comments and a trailing comma highlights both comments as comment and every key, including the one after the trailing comma, as string.special.key; x.code-workspace and x.code-snippets are json"
    required: true
    manual: false
  - id: "C4"
    text: "pane_item_tests: language_label gives Git Commit Message, Git Rebase Todo, Git Config, Git Attributes, Ignore, INI, Dotenv, Just, Diff and JSON5"
    required: true
    manual: false
  - id: "C5"
    text: "THIRD_PARTY.md lists diff and json5; cargo deny check licenses bans sources passes (the RUSTSEC advisories already on main are T1301's); cargo nextest run -p fe-syntax -p fe-config -p fe-gpui passes"
    required: true
    manual: false
---

# Highlight diffs and patches in green and red, add JSON5, check JSONC comments and trailing commas, and give every new language a status-bar label

## Problem

Patches (`.diff`, `.patch`) and JSON5 files open as plain text, and the user asked (2026-11-01) whether
JSONC files (`tsconfig.json`, `jsconfig.json`, `.vscode/*.json`, `*.jsonc`) highlight. They mostly do already:
the built-in `tree-sitter-json` 0.24.8 grammar has `comment` (both `//` and `/* */`) in its extras, and every
one of those names is already JSON by extension (`json`, `jsonc` in `language.rs`). A trailing comma is not in
that grammar; it becomes an `ERROR` node, and nothing pins how the rest of the file highlights around it. VS
Code's other JSONC files, `*.code-workspace` and `*.code-snippets`, are plain text. A `json5` Markdown fence
is aliased to JSON (`ALIASES` in `language.rs`).

The new languages from this card and the two before it (and T1282's `gitignore`) show their raw ids in the
status bar and language picker, since `language_label` in `crates/fe-gpui/src/pane_item.rs` falls through.

## Change

- `diff`: crates.io `tree-sitter-diff` 0.1.0 (published by the-mikedavis, the grammar's author; MIT; ABI 14;
  `tree-sitter-language` 0.1; about 18 KB of object code). New feature `diff`, on by default. Extensions
  `diff`, `patch`. Its query paints additions `string` and deletions `keyword`; give them proper colours
  instead: add `markup.inserted` and `markup.deleted` to `NAMES` in `crates/fe-syntax/src/highlight.rs` (with
  `diff.plus`/`diff.minus` as `ALIASES`, nvim's names), add `crates/fe-syntax/queries/diff.scm` after the
  crate's query mapping `(addition)`/`(new_file)` to `markup.inserted` and `(deletion)`/`(old_file)` to
  `markup.deleted`, and give both names a colour in each bundled theme's `[syntax]` table
  (`crates/fe-config/themes/*.toml`), taken from that theme's own green and red. Fold `block` and `hunk`.
  Markdown ```` ```diff ```` fences then highlight through `injected_language` with no alias.
- `json5`: vendor Joakker/tree-sitter-json5 (the upstream, MIT, pin
  `248b8564567087d7866be76569b182f6dd7e14e9`) as `vendor/tree-sitter-json5`, shaped like T1282's gitignore
  crate. crates.io's `tree-sitter-json5` 0.1.0 (Joakker) needs tree-sitter ~0.20. ABI 15, no scanner, about
  18 KB. Extension `json5`; drop the `("json5", "json")` alias. Its query paints keys and numbers as
  `constant`; `crates/fe-syntax/queries/json5.scm` goes after it so a member's `name` is `string.special.key`
  and `number` is `number`, matching JSON. Fold `object` and `array`; comment tokens and auto pairs as JSON.
- JSONC: add extensions `code-workspace` and `code-snippets` to the JSON `Def`. New test: a `tsconfig.json`
  text with a `//` comment, a `/* */` comment and a trailing comma highlights both comments as `comment` and
  every key, including the one after the trailing comma, as `string.special.key`.
- Labels: `language_label` gives `Git Commit Message`, `Git Rebase Todo`, `Git Config`, `Git Attributes`,
  `Ignore`, `INI`, `Dotenv`, `Just`, `Diff` and `JSON5` (whichever exist at this card's base).

Per-language tables for `diff` and `json5`: `comment.rs` (diff has no comment tokens; json5 as json),
`tests/auto_pairs.rs`, `src/tests.rs` `each_language_highlights_a_snippet`, `src/fold/tests.rs` `CASES`.
Goldens `tests/golden/diff.diff` (a `git diff` with a rename, a mode change, two hunks and `\ No newline`) and
`json5.json5`.

Record `diff` and `json5` in `THIRD_PARTY.md` (`diff.scm` and `json5.scm` are ours) and note the
release-binary growth (about 36 KB) in the decision file.
