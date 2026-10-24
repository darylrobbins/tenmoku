---
task_uid: "01m5nmjnnxdprpj65mp1kxf7pp"
task: "T302"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Resetting a key whose path ends in a dotted key inside an inline table (for example minimap.enabled in editor = { # keep editor minimap.enabled = true }) removes the whole dotted assignment and one comma, keeps every comment in and around the table, and prunes a table it empties only when no comment is inside it; applies to reset_settings and write_settings(.., None). Tests include the closing brace on the value's line, with and without sibling keys"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK, no new dependency"
    required: true
    manual: false
---

# Reset a setting that sits in a commented inline table with dotted keys, keeping every comment (follow-up to Mac T1095)

## Problem
T294 (Mac T1095) narrowed C2 at review round 3: resetting a key that sits in an inline table the exact key-and-comma cut cannot take (for example the dotted key `minimap.enabled` inside `editor = { # keep ... }`) and whose removal through toml_edit would drop a comment is now refused (`Invalid`, file untouched). This card removes the limit.

## Change
- `kiln/crates/kiln-core/src/write.rs`: when the key's path ends inside an inline table with dotted keys, locate the enclosing inline table and cut the complete dotted assignment (and one comma) keeping every comment, then prune tables it empties. Keep the check that the cut parses to the same table toml_edit's removal gives.
- Remove the refusal for the cases now handled; keep it for any case still not cuttable.

## Tests
Regressions in `kiln/crates/kiln-core/tests/write_setting_values.rs`: dotted paths with the closing brace on the value's line, with and without sibling keys, with comments before, inside and after; `reset_of_a_dotted_key_in_a_commented_inline_table_is_refused` becomes a success test.
