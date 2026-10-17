---
task_uid: "01m551h02s3yf5zw4vszze7kj2"
task: "T149"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-syntax::first_line_languages first_lines_pick_their_language_as_vs_code_does (tests/first_line_languages.rs, top level) passes: with no path or an extensionless one, detect_language_name returns xml for a first line holding \u003c?xml, \u003csvg or \u003c!doctype svg, yaml for one starting #cloud-config, and bash for one starting # -*- mode: shell-script -*-, as VS Code's firstLine regexes pick them; it fails at the base (None)"
    required: true
    manual: false
  - id: "C2"
    text: "A path that names a language still wins (a.py with an \u003c?xml first line stays python); near misses stay None: # cloud-config, # mode: shell-script without the -*- pair, and \u003c?xml on the second line; fe-syntax::shebang_languages and fe-syntax::shebang_runtimes pass as is; language.rs is not changed"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax passes with no existing test edited; cargo clippy -p fe-syntax --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Detect XML, YAML and shell files by their first line as VS Code does (<?xml, <svg, <!doctype svg, #cloud-config, an Emacs mode: shell-script line) when the path names no language (Mac T781)

## Problem

A file whose path names no language (untitled, or an extensionless name such as cloud-init's `user-data`) is
detected only from a `#!` line. VS Code's language contributions also detect by first line (their `firstLine`
regexes, read from /Applications/Visual Studio Code.app/Contents/Resources/app/extensions on 2026-10-17):

- xml: `(\<\?xml.*)|(\<svg)|(\<\!doctype\s+svg)` (unanchored)
- yaml: `^#cloud-config`
- shellscript: `...|^#\s*-\*-[^*]*mode:\s*shell-script[^*]*-\*-` (the Emacs mode line; the shebang half is
  already covered by `INTERPRETERS`)

At bcb3ea60, `fe_syntax::detect_language_name(None, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<a/>\n")`
is `None` (VS Code: XML), as are `#cloud-config\n...` (VS Code: YAML) and `# -*- mode: shell-script -*-\n`
(VS Code: Shell Script). The prototype test crates/fe-syntax/tests/first_line_languages.rs fails at base with
`left: None, right: Some("xml")`. The only caller is crates/fe-gpui/src/pane_item.rs, which passes the
buffer's first bytes.

## Change

In crates/fe-syntax/src/detect.rs, `detect_language_name` falls back, after `language_name_for_shebang`, to a
new private `language_name_for_first_line(text_start)` that reads the first line only and applies the three
rules above exactly as VS Code's regexes do (case-sensitive; the XML ones anywhere on the line; the others at
its start), returning `xml`, `yaml` or `bash` when that language is compiled in
(`language_names().find(..)`, as the shebang path does). A path that names a language still wins. No regex
crate: plain string matching. language.rs is untouched (T772 holds it).
