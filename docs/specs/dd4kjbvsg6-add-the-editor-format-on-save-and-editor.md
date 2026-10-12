---
task_uid: "01m4rbj4vs0g80n5dd4kjbvsg6"
task: "T46"
rev: 1
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-config test editing_settings::defaults_and_parse: format_on_save defaults to false and auto_close_pairs to true; [editor] format_on_save = true and auto_close_pairs = false parse; a string for either gives a diagnostic and keeps the default"
    required: true
    manual: false
  - id: "C2"
    text: "settings.schema.json describes both with their defaults, the template lists both commented out, and cargo test -p fe-config (template and schema tests included) and cargo clippy -p fe-config --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Add the editor.format_on_save and editor.auto_close_pairs settings (Mac T466)

Filed on argon from Mac T466 (card ~/coordination/argon-cards/T466.json rev 1, Mac lead message ~/coordination/from-mac-lead-2026-10-12 note j (relayed by the user).md). Mac task uid 01m4qzd8r8hfbza8tmd1v09djd. Title the host commit "... (Mac T466)".

## Problem

The user (2026-10-12) asked for format on save and for auto-closing brackets and quotes with a setting to turn them
off. Both need a setting in `EditorSettings` (`crates/fe-config/src/settings.rs` :119), its JSON schema and the
settings template. Their feature cards are on different hosts, so adding both settings here keeps them off the
same three files.

## Change

In `EditorSettings`, with doc comments that become the schema's descriptions:

- `format_on_save: bool`, default `false`: "Format a file with its language server before saving it. A server
  that doesn't answer within a second is skipped and the file saves unformatted."
- `auto_close_pairs: bool`, default `true`: "Type the closing bracket or quote when typing an opening one, type over
  it, and delete both with backspace. Off, typing inserts only what's typed."

Regenerate `crates/fe-config/settings.schema.json`, and add both, commented out with their defaults, to
`crates/fe-config/settings-template.toml` beside `tab_size` and `soft_wrap`, as the existing template test
requires. Nothing reads them yet.

## Tests

New file `crates/fe-config/tests/editing_settings.rs`.
