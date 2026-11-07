---
task_uid: "01m6t5mk73zy14xjf9y5cffqfn"
task: "T495"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "extension_install_dialog::a_namespace_registered_while_the_dialog_is_open_is_reserved"
    required: true
    manual: false
---

# Take the extensions dir and reserved command namespaces after the install dialog closes, not before (follow-up of T1349) (Mac T1489)

Review follow-up (Mac T1349, 5cd938e1), F1: extension_install.rs, `Workbench::install_extension`.

`install_extension` reads `extensions_dir(cx)` and builds `check_api(reserved_namespaces(cx))` before `cx.prompt_for_paths` and carries both across the open dialog. The dialog can stay open for minutes; a plugin or extension that loads in that time can register a command namespace the captured `reserved` list lacks, so an extension whose id takes that namespace is accepted. Take both after the user picks, inside the spawned task (via `AsyncApp::update`), not before. Keep the `busy` flag set before the dialog as now.

Regression test (new tests/extension_install_dialog.rs, base public APIs only; point the state dir with TENMOKU_STATE_DIR under a serial lock like tests/grammar_prompt.rs): open the dialog through the `workbench.install_extension` command, register a command in namespace `acme` while the dialog is open, then pick a zip whose manifest id is `acme.tool`: the install is refused with the reserved-namespace toast. Fails at base (it installs).
