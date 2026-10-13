---
task_uid: "01m4se4n9hmtfzf0r8gt227wkv"
task: "T71"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test outline_data::outlines_match_golden_files passes: json.json, yaml.yaml, css.css and Dockerfile in tests/outline_data_samples/ each outline to their committed .outline golden (kind, name, range, depth)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test outline_data::json_and_yaml_keys_nest passes: {\"a\": {\"b\": 1}, \"c\": [{\"d\": 2}]} outlines as Module \"a\" (depth 0) \u003e Field \"b\", and Field \"c\" (depth 0) \u003e Field \"d\"; the same shape in YAML block style gives the same items, a quoted YAML key keeps its quotes, a flow mapping {x: 1} lists x, and both documents of a two-document YAML file are outlined"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test outline_data::css_and_dockerfile_items passes: a CSS rule set is a Class named by its selectors with whitespace runs collapsed (a two-line \"a,\\n  b\" reads \"a, b\"), nested under an @media Module named by its first query; @supports and @keyframes are Modules (keyframes named by keyframes_name); Dockerfile \"FROM rust:1 AS build\" is one Module named \"build\" and \"FROM debian\" one Module named \"debian\", each listed once"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax test outline_data::outline_but_no_symbols passes: outline_supported is true and symbols_supported false for json, yaml, css and dockerfile, symbols() and symbols_job() give Err(NoOutline::Unsupported) for each, and html and xml still have neither; outline_scripts::supported_flags and outline::no_outline_without_a_full_tree pass with their \"no outline\" language changed from json to html"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax and -p fe-workbench pass, and cargo clippy -p fe-syntax --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Outline JSON and YAML keys, CSS rule sets and at-rules, and Dockerfile stages in fe-syntax, with no definition symbols for them (Mac T591)

Filed on helium from Mac T591 (~/coordination/helium-cards/T591.json; Mac lead message ~/coordination/from-mac-lead-2026-10-13-helium-zh.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The plan's M3 "Browse" milestone gives the workbench an outline, and `fe-syntax` builds it from per-language outline
queries (`crates/fe-syntax/src/outline.rs`, `QUERIES`, 21 entries). Every code language has one, and so do Markdown
(headings) and TOML (tables), but JSON, YAML, CSS and Dockerfile have none: `outline_supported("json")` is false
(pinned by `crates/fe-syntax/tests/outline_scripts.rs` `supported_flags` and `crates/fe-syntax/tests/outline.rs`
`no_outline_without_a_full_tree`). So the outline panel, the breadcrumbs and cmd-shift-o (`outline.show`) show
nothing for `package.json`, a CI workflow, a stylesheet or a Dockerfile, which are among the files users browse
most. T64 and T113 left these languages out because their grammars ship no `tags.scm`; T497 then did the same for
Bash, Make and SQL with queries of our own written from `node-types.json`.

## Change

Add outline queries, ours, written from each grammar's `src/node-types.json` (no upstream query text), each
starting with the same header line as `queries/outline/bash.scm` (`;; outline query, ours; capture convention of
tree-sitter tags`), and register them in `QUERIES` with **no definitions query** (`&[]`, as TOML does), so
`symbols()` stays `Unsupported` for all four and fe-nav's search index gets no JSON keys or selectors.

1. **`queries/outline/json.scm`** (tree-sitter-json 0.24.8): every `pair` is an item named by its key's text
   without the quotes. A pair whose value is an `object` is `@definition.module`; every other pair (arrays and
   scalars) is `@definition.field`. Put the module pattern first: `outline.rs` keeps the first pattern that
   matches a node (outline.rs ~line 327). Pairs nest by range, so `{"a": {"b": 1}}` gives `a` at depth 0 and `b`
   at depth 1 under it; objects inside arrays nest their pairs under the array's key.
2. **`queries/outline/yaml.scm`** (the vendored tree-sitter-yaml in `vendor/tree-sitter-yaml`): every
   `block_mapping_pair` and `flow_pair` with a key is an item named by the key as written (a quoted key keeps its
   quotes). A pair whose value is a `block_mapping` or `flow_mapping` is `@definition.module`, any other is
   `@definition.field`, module first. Keys of mappings inside sequence items (`- name: x`) nest under the
   sequence's key. Every document of a multi-document file is outlined.
3. **`queries/outline/css.scm`** (tree-sitter-css 0.25.0): each `rule_set` is `@definition.class` named by its
   `selectors` (outline.rs already collapses whitespace runs in names, so a multi-line selector list reads on one
   line). `media_statement` and `supports_statement` are `@definition.module` named by their first query child,
   and `keyframes_statement` is `@definition.module` named by its `keyframes_name`. Rule sets inside `@media` and
   nested rule sets nest under them.
4. **`queries/outline/dockerfile.scm`** (tree-sitter-containerfile 0.9.2): each `from_instruction` is
   `@definition.module`, named by its `image_alias` when it has `AS <alias>` and by its `image_spec` otherwise
   (use the negated field `!as` so a stage with an alias isn't listed twice). An item's range is the FROM
   instruction itself: no section is built for the instructions under it.

Update the two existing tests that pin "JSON has no outline": in `outline_scripts.rs` `supported_flags`, the
"neither" list becomes `["html", "xml"]`, and add an assertion that json, yaml, css and dockerfile have an outline
but no symbols; in `outline.rs` `no_outline_without_a_full_tree`, use `html` where it uses `json`. If fe-workbench's
`tests/pane_toolbar.rs` (`for language in [None, Some("json")]`, "a language with no outline") fails because JSON
now has an outline, change `"json"` to `"html"` there and nothing else.

HTML and XML are out of scope: an element tree needs names built from several nodes (tag, id, class), which the
capture convention can't express. No new `OutlineKind`, no fe-workbench or fe-nav code change, no keymap change.

## Tests

New `crates/fe-syntax/tests/outline_data.rs`, `#![cfg(all(feature = "json", feature = "yaml", feature = "css",
feature = "dockerfile"))]`, in the style of `tests/outline_scripts.rs`: one sample per language in the new
directory `tests/outline_data_samples/` (`json.json`, `yaml.yaml`, `css.css`, `Dockerfile`), each compared with a
`<sample>.outline` golden in that file's format (kind, name, range, two-space indent per depth), regenerated with
`UPDATE_GOLDEN=1`, plus the focused tests the criteria name. Samples stay small (under 100 lines each); read the
directory at run time from `CARGO_MANIFEST_DIR` as `outline_scripts.rs` does.

## Implementation notes

- JSON keys are named by what's between their quotes: `name_range` in `outline.rs` strips a JSON `string`'s
  quote tokens (for JSON only; a TypeScript enum's `"Left"` keeps its quotes), so escapes read as written
  (`"with \"escapes\""` reads `with \"escapes\"`) and an empty key, `""`, is an item with an empty name
  (review F1).
- `@media` and `@supports` are named by their first query, after any comments (`. [(comment) (js_comment)]* .`),
  so `@media /* c */ screen, print` reads `screen` (review F2).
- `outline_data::outline_but_no_symbols` checks that HTML and XML are listed languages only when their features
  are built (review F3).
- `tests/outline.rs` `outlines_match_golden_files` checks that every outlined language has a sample, so it now
  also reads `outline_data_samples/`.
- Out of the listed scope, `crates/fe-workbench/src/tests/outline_panel.rs` `empty_and_too_large_states`
  pinned "No outline for json" and failed; it uses `page.html` and "No outline for html" now, the same swap
  the spec gives for `pane_toolbar.rs`. `pane_toolbar.rs` didn't fail (its Rust text has no JSON pairs), but
  JSON is no longer "a language with no outline", so it uses `html` too.
