---
task_uid: "01m5ekgq34xk6msa3f5egehb1h"
task: "T1020"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The settings template's theme.name comment names every bundled theme title, in BUNDLED's order, including Dark 2026 and Light 2026"
    required: true
    manual: false
  - id: "C2"
    text: "A test in crates/fe-config/src/tests/template_theme_names.rs fails if a bundled theme title is missing from that comment (fails at base, since the base lacks the 2026 names), and cargo test -p fe-config passes"
    required: true
    manual: false
---

# List the Dark 2026 and Light 2026 themes in the settings template's theme.name comment, with a test that every bundled theme title is listed there

T1017 and T1018 bundled "Dark 2026" and "Light 2026" (crates/fe-config/src/themes.rs BUNDLED), but the settings template's theme.name comment still lists only the older four:

    # A bundled theme by name: Dark Modern, Fastedit Dark, Fastedit Light or
    # Fastedit Ember. ...

(crates/fe-config/settings-template.toml, around line 108). A user reading the template can't tell the 2026 themes exist.

Change: list every bundled theme title in that comment, in BUNDLED's order (Dark Modern, Dark 2026, Light 2026, Fastedit Dark, Fastedit Light or Fastedit Ember), re-wrapped to the file's width. Leave the commented example (`# name = "Dark Modern"`) as is.

Keep it from drifting again: add a test in a new file, crates/fe-config/src/tests/template_theme_names.rs (declared in src/tests/mod.rs), that takes the template's comment paragraph beginning "# A bundled theme by name:" from USER_SETTINGS_TEMPLATE and asserts every `themes::bundled_themes()` title appears in it. Line wrapping may split a title across two comment lines, so join the paragraph's lines (strip "# ") before matching.
