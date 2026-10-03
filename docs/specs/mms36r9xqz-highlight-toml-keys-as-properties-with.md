---
task_uid: "01m417z46801jhg4mms36r9xqz"
task: "T18"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax test tests::query_overrides::toml_keys_are_properties passes: pair and dotted keys are property, table headers type, and no span covers only whitespace"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test tests::query_overrides::json_and_yaml_keys_differ_from_values passes"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test tests::each_language_highlights_a_snippet passes with TOML name expected as property"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax test tests::golden::highlights_match_golden_files passes with a regenerated toml.toml.golden"
    required: true
    manual: false
---

# Highlight TOML keys as properties with an nvim-treesitter override query

## Problem

T-14 (draft, P3) says upstream queries are weak: "upstream gives TOML keys the `type` highlight and gives JSON and YAML keys no colour distinct from their values". Checked against main and the golden output from T-07's prior attempt, only the TOML part still holds:

- **TOML:** tree-sitter-toml-ng 0.7.0's `highlights.scm` captures `(bare_key) @type`, and its `(pair (bare_key)) @property` captures the whole *pair*, not the key. So pair keys show as `type`, and the whitespace inside each pair gets a stray `property` span (`toml.toml.golden`: `4:1-5 type "name"`, `4:5-6 property " "`). The snippet test pins this today: `("name", "type")` in `each_language_highlights_a_snippet` (`crates/fe-syntax/src/tests.rs:93`).
- **JSON** keys are already `string.special.key` and **YAML** keys `property`, and the theme in `crates/fe-gpui/src/lib.rs` colours both apart from `string`. Nothing to fix there; this card only confirms it with a test.

## Change

- Add `crates/fe-syntax/queries/toml.scm`, a small override query translated from nvim-treesitter's TOML `highlights.scm` (Apache-2.0, an allowed source per `CLEAN_ROOM.md`). It captures pair keys, including dotted ones, as `property` and table-header keys as `type`. Translate nvim-only predicates (`#lua-match?` to `#match?`; drop `#set! priority`). Start the file with a header naming the source repo, commit, path and license.
- Append it after `tree_sitter_toml_ng::HIGHLIGHTS_QUERY` in the toml `Def` in `crates/fe-syntax/src/language.rs` (`include_str!`). TOML's query is last-pattern-wins, so the appended patterns take precedence on the same node. Document the overrides mechanism on `Def::highlights`: crate query first, then our `queries/<lang>.scm`.
- Update the TOML snippet in `crates/fe-syntax/src/tests.rs` to expect `property` for `name`.
- Record the override in `THIRD_PARTY.md`: a row in "Code copied or adapted", and the toml row's Query column becomes "`HIGHLIGHTS_QUERY` + `queries/toml.scm`". Keep the Apache-2.0 text shipped.
- Never use Zed queries or tree-sitter-language-pack.
- Regenerate `crates/fe-syntax/tests/golden/toml.toml.golden` and review the diff.

New test file: `crates/fe-syntax/src/tests/query_overrides.rs` (`mod query_overrides;` in `tests.rs`).

## Open questions (draft card: needs the user's approval)

- T-14 is still a draft. Approving this part doesn't commit to the grammar batches.
- T-14 also says to "consider font styles in highlights, such as italic comments and bold headings; GPUI's TextRun supports them". That's a product decision for a person, so it isn't in any card. If approved, it touches `crates/fe-syntax/src/highlight.rs` (theme entries carrying a style) and `crates/fe-gpui/src/{lib,element}.rs`. It would also give T-12's unmapped `text.emphasis`/`text.strong` captures somewhere to go.
