---
task_uid: "01m5v8yvqjfbxr2eapggk0174z"
task: "T1176"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "eol_commands_tests::to_crlf_marks_the_buffer_modified: on a clean LF buffer editor.set_end_of_line_crlf makes buffer.line_ending() CrLf, the buffer modified, and a save writes every break as \\r\\n (disk_io style round trip); the text rows and selections are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "eol_commands_tests::to_lf_and_noop: editor.set_end_of_line_lf on a CRLF buffer saves LF breaks; choosing the ending the buffer already has leaves it unmodified"
    required: true
    manual: false
  - id: "C3"
    text: "eol_commands_tests::undo_restores: undo after a change returns the previous ending and the clean/dirty state (VS Code makes this one undo step), redo reapplies it"
    required: true
    manual: false
  - id: "C4"
    text: "command_catalog_tests lists both commands with titles 'Change End of Line Sequence to LF' and '... to CRLF'; Catalog tests updated (command_catalog_tests, typed_command_ids_tests, fe-editor tests::commands); cargo nextest run -p fe-editor -p fe-keymap -p fe-workbench -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# Change End of Line Sequence: LF and CRLF commands, as VS Code's workbench.action.editor.changeEOL

fastedit already detects the ending (majority rule) and keeps it on save, and the status bar shows it, but nothing can change it. VS Code opens a picker; to keep workbench.rs/lib.rs/palettes out of this card, expose two palette commands in fe-editor instead (a picker plus status-bar click is a separate follow-up). Uses Buffer::set_line_ending (crates/fe-buffer/src/buffer.rs:532); read crates/fe-editor/src/disk.rs:620-650 for how saved state compares the ending so the dirty flag is right.
