---
task_uid: "01m72f60ts3rpd808mt565bwfw"
task: "T342"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test a_file_created_after_a_locale_change_uses_the_new_template (cargo nextest run -p kiln-shell persist_template_locale) passes: make the writer, change the locale, create the file through a toggle"
    required: true
    manual: false
  - id: "C2"
    text: "The provider matches T1506's text in docs/decisions/xte5e7hqsw-i18n-design.md section 4"
    required: true
    manual: false
---

# The settings-template writer resolves the template when it creates the file, so a live locale change reaches it (T1506 G-2) (Mac T1531)

# The settings-template writer resolves the template when it creates the file (T1506 G-2)

Spec: `docs/decisions/xte5e7hqsw-i18n-design.md` § 4 "Settings descriptions" and § 11 G-2, as T1506 rewrote
them (review finding F14): `kiln_shell::persist::setting_writer`
(`kiln/crates/kiln-shell/src/persist.rs`) stops capturing the template when
the writer is made and resolves it when it creates the file (the provider
T1506 specifies). Test: create the writer, change the locale, create the file
through a toggle, and the file has the new locale's template.

Platform: **Linux-OK**.
