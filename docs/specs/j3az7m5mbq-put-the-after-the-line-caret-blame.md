---
task_uid: "01m58zqec00nq3g7j3az7m5mbq"
task: "T216"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Integration test fe-config inline_blame_default_off_and_parse passes: EditorSettings::default().inline_blame is false, and '[editor]\\ninline_blame = true' loads with no diagnostics as true; template_is_all_defaults and the schema golden test (settings.schema.json regenerated) pass"
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-workbench workbench::git_blame::git_blame_tests::setting_tests::the_setting_starts_line_blame passes: a window made with inline_blame true shows the fixture's caret-row blame after REST with no command run; one made with the default shows none until git.toggle_line_blame"
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-workbench workbench::git_blame::git_blame_tests::setting_tests::changing_the_setting_follows passes: reload_settings changing inline_blame from false to true turns an open window's line blame on, true to false turns it off; a reload that leaves inline_blame as it was keeps a window's git.toggle_line_blame choice"
    required: true
    manual: false
---

# Put the after-the-line caret blame behind an editor.inline_blame setting, off by default, that starts every window's line blame on or off (Mac T821)

{
  "mode": "inline",
  "body": "## Problem\n\nThe user, 2026-10-17 (Mac main 92c57c57), on blame: \"I don't like the git blame view. Can we do something\ncloser to jetbrains with a column getting added before the line.\" The previous card makes the JetBrains\nblame column, `git.toggle_blame_annotations`, the blame view. The lead's decision for the after-the-line\ncaret blame (T555, crates/fe-workbench/src/git_blame.rs, `git.toggle_line_blame`) is to keep its code but\nput it behind a setting that is off by default.\n\nToday line blame is off in every new window and turned on per window by `git.toggle_line_blame` (`toggle`,\ngit_blame.rs:259). No setting exists to start a window with it on, and none can turn it off everywhere.\nThe settings schema has no blame key: `EditorSettings` in crates/fe-config/src/settings.rs:179, and the\ngolden crates/fe-config/settings.schema.json. `Workbench::new` already receives `EditorSettings`, and\n`Workbench::reload_settings` (crates/fe-workbench/src/workbench.rs:2461) gets every settings change.\n\n## Waits for (not Mac tasks, so not --depends-on)\n\nThe previous card's waits apply here too: helium's T135 (git_blame.rs) and helium's run of T704\n(workbench.rs) must have landed on Mac main first.\n\n## Change\n\n1. **Setting**: `editor.inline_blame: bool`, default `false`, in `EditorSettings`\n   (crates/fe-config/src/settings.rs). Its doc line: \"Show the caret row's git blame after the line in\n   every new window; Git: Toggle Line Blame turns it on or off for one window.\" Then:\n   - regenerate crates/fe-config/settings.schema.json (`UPDATE_GOLDEN=1`);\n   - add `# inline_blame = false` to crates/fe-config/settings-template.toml's `[editor]`;\n   - add `\"inline_blame\"` to the editor keys in crates/fe-config/tests/template.rs.\n\n2. **Workbench** (crates/fe-workbench/src/workbench.rs → git_blame.rs):\n   - `Workbench::new` with `inline_blame` true turns the window's line blame on, as `git.toggle_line_blame`\n     does. False leaves it off, as today.\n   - `reload_settings` turns every window's line blame on or off to match when `inline_blame` changes. If\n     it doesn't change, a window's own toggle stands.\n   - `git.toggle_line_blame` keeps working per window either way.\n   - Factor `toggle` into `set_line_blame(workbench, on: bool, cx)` and call it from both places.\n\nTests:\n- crates/fe-config/tests/inline_blame_setting.rs, a new integration test as\n  crates/fe-config/tests/indent_guides_setting.rs is.\n- crates/fe-workbench/src/git_blame_setting_tests.rs, declared in the existing\n  crates/fe-workbench/src/git_blame_tests.rs by `#[path = \"git_blame_setting_tests.rs\"] mod setting_tests;`.\n  It uses the parent's `fixture`, `put_caret`, `wait`, `annotations` and `counted` helpers, building its\n  window with `Workbench::new(.., EditorSettings { inline_blame: true, ..EditorSettings::default() }, ..)`.",
  "ref": "",
  "ref_blob": ""
}
