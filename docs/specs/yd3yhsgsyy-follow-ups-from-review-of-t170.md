---
task_uid: "01m4c9q4et936n7hyd3yhsgsyy"
task: "T184"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F10: write_setting adds a missing key or table to any file that loads, without requiring unrelated content to round-trip through toml_edit's serialization: the new text is inserted at a location taken from the parsed original (an absent table appended directly), every other byte unchanged. Files with interleaved dotted keys such as `editor.tab_size = 2\\nvim.enabled = false\\neditor.font_size = 15.0\\n` (adding status_bar.visible) and the same with status_bar.visible separating the editor keys (adding vim.enabled) succeed byte-preservingly; the refusal expectations in crates/fe-config/tests/settings_write.rs (around line 195) are replaced by successful byte-preserving assertions, and the write_setting doc comment no longer describes the refusal."
    required: true
    manual: false
  - id: "C2"
    text: "When write_setting adds a key to a table that holds only the template's commented-out default for it (e.g. `[vim]` / `# Whether the editor starts...` / `# enabled = false`), the commented default line becomes the live line in place (`enabled = true`), keeping its doc comment directly above it and every other byte unchanged, instead of the new line landing under the header above the doc comment. Tested for both vim.enabled and status_bar.visible starting from USER_SETTINGS_TEMPLATE (empty or missing file) in the new file crates/fe-config/tests/settings_write_insert.rs; existing expectations that codify the old placement (write.rs a_change_meanwhile_is_kept, settings_write.rs only_an_empty_file_gets_the_template) are updated."
    required: true
    manual: false
  - id: "C3"
    text: "New tests for F10 go in crates/fe-config/tests/settings_write_insert.rs, covering both toggles (vim.enabled, status_bar.visible) on interleaved dotted settings, with LF and CRLF files."
    required: true
    manual: false
---

# Follow-ups from review of T170

Findings of the review of T170 that don't block the merge (below merge.block_severity):

- F10 medium crates/fe-config/src/write.rs:93: Adding a missing setting fails on valid settings files when unrelated dotted keys do not round-trip through toml_edit unchanged. For example, `editor.tab_size = 2\nvim.enabled = false\neditor.font_size = 15.0\n` loads successfully, but adding status_bar.visible returns Err because serialization groups the editor keys and align rejects the result. The same problem prevents adding vim.enabled when status_bar.visible separates those editor keys. The spec requires missing keys and tables to be added; the refusal test instead codifies this persistence failure.
  fix: Insert missing settings using parsed locations in the original text, appending absent tables directly, without requiring unrelated content to round-trip through serialization. Replace the refusal expectations with successful, byte-preserving tests, including both toggles on interleaved dotted settings.
  instances: crates/fe-config/src/write.rs:93, crates/fe-config/tests/settings_write.rs:195
