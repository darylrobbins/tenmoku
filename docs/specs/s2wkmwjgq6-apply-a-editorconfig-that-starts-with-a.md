---
task_uid: "01m4tjjnsqn3dzp7s2wkmwjgq6"
task: "T116"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-config test editorconfig_bom::editorconfig_with_a_bom_before_its_first_section_applies: a BOM then [*] indent_style = tab and [*.py] space 4 give a.rs a tab indent and b.py a 4-column space indent"
    required: true
    manual: false
  - id: "C2"
    text: "fe-config test editorconfig_bom::nested_editorconfig_with_a_bom_before_its_first_section_applies_over_its_parent: an inner BOM file with [*] indent_style = tab applies along with the outer file's end_of_line"
    required: true
    manual: false
  - id: "C3"
    text: "fe-config test editorconfig_bom::editorconfig_with_a_bom_before_its_preamble_applies passes, the existing fe-config editorconfig tests pass unchanged, and no Cargo.toml or Cargo.lock changes"
    required: true
    manual: false
---

# Apply a .editorconfig that starts with a byte order mark before its first section in fe-config, instead of skipping the file as invalid (Mac T655)

## Problem

A `.editorconfig` that starts with a UTF-8 byte order mark immediately followed by a section header is
skipped whole by fe-config (crates/fe-config/src/editorconfig.rs `read_file`), with a "skipping ...: invalid
line" warning, so none of its properties apply. Windows tools (Visual Studio among them) commonly write
`.editorconfig` with a BOM, and `[*]` on the first line is the usual layout.

Cause, in ec4rs 1.2.0 (the workspace's version): `LineReader::next_line` strips U+FEFF only while it parses
line 1, but keeps the line with the mark. The preamble loop in `ConfigParser::new_with_path` stops at the
`[*]` it parsed, and the first `read_section` calls `reader.reparse()` on the stored `"\u{feff}[*]\n"`;
`trim_start` doesn't remove U+FEFF, so the line is neither a section nor a pair and gives
`ParseError::InvalidLine`, which `read_file` treats as a broken file. A BOM before `root = true`, a comment or
a blank line parses fine, because that line is never reparsed.

Checked 2026-10-13 at Mac e6988aba (editorconfig.rs is identical on helium main, blob 19104cdc): with
`.editorconfig` = `"\u{feff}[*]\nindent_style = tab\n"`, `editorconfig_for(dir/a.rs).indent_style` is `None`
(expected `Some(IndentKind::Tab)`).

## Change

In crates/fe-config/src/editorconfig.rs `read_file`: read the file's bytes with `std::fs::read` (NotFound
still returns `None`; any other error warns and skips as now), strip a leading `EF BB BF`, and parse the rest
with `ec4rs::ConfigParser::new_with_path(std::io::Cursor::new(..), Some(file.as_path()))`, taking `is_root`
from the parser and iterating its sections exactly as the loop does today. No dependency change (no ec4rs
bump, no Cargo.toml or Cargo.lock edit). Everything else stays: a broken file is still skipped whole with
one warning, and the existing tests in crates/fe-config/tests/editorconfig.rs pass unchanged.

## Tests

New file crates/fe-config/tests/editorconfig_bom.rs, using `editorconfig_for`, `tempfile` and `fs::write`:

- `editorconfig_with_a_bom_before_its_first_section_applies` (regression, fails at base): a file
  `"\u{feff}[*]\nindent_style = tab\n[*.py]\nindent_style = space\nindent_size = 4\n"` gives `a.rs` a tab indent
  and `b.py` a space indent of 4 columns.
- `nested_editorconfig_with_a_bom_before_its_first_section_applies_over_its_parent` (regression, fails at
  base): a non-root inner file `"\u{feff}[*]\nindent_style = tab\n"` under an outer `[*]\nend_of_line = crlf\n`
  gives `proj/a.rs` a tab indent and still the outer end of line.
- `editorconfig_with_a_bom_before_its_preamble_applies` (control, passes at base):
  `"\u{feff}root = true\n[*]\nindent_style = tab\n"` gives a tab indent.
