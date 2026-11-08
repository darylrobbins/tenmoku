---
task_uid: "01m6w2ycskr4nr6amdvk5hr8zs"
task: "T519"
rev: 1
type: "docs"
complexity: "m"
criteria:
  - id: "C1"
    text: "Section Fetching strings per surface defines one command-label resolver usable by CommandMeta, CommandSpec, CommandInfo and TypedCommand; the Migration plan cards I3, W1 and Z1 have scopes and dependencies covering the consumers at command_palette.rs:93, contributions.rs:1737 and :1879, which_key.rs:580, keymaps_picker.rs:85 and shortcuts_editor.rs:1276; a fake-locale test of one macro-declared command across those surfaces is specified"
    required: true
    manual: false
  - id: "C2"
    text: "Section Keeping the catalogue complete requires references to resolve inside the bundle that formats them, or specifies how fallback resources populate that bundle, and specifies a negative fixture where an unselected translated branch references a term or attribute present only in en-US"
    required: true
    manual: false
  - id: "C3"
    text: "The doc specifies that kiln-shell persist::setting_writer resolves the settings template when it creates the file (provider callback or host-owned closure), not at startup (persist.rs:65), and specifies a test that initialises the writer, changes locale, then creates the settings file through a toggle"
    required: true
    manual: false
  - id: "C4"
    text: "Only docs/decisions/xte5e7hqsw-i18n-design.md changes; its status stays proposed"
    required: true
    manual: false
---

# Amend the i18n design: typed-command label resolver (F13), per-bundle reference check (F10), live-locale settings template writer (F14) (Mac T1506)

Close the medium findings F10, F13 and F14 left open when T1432 (the i18n design doc) finished review round 2; T1432 narrowed its C3 to a documented limit naming this card. `rgt review show T1432` has the exact findings and line cites. Doc edits only: no implementation, no new cards.
