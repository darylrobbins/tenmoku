---
task_uid: "01m70zkjk4depn8x6m660ewyem"
task: "T546"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Every test in kiln/crates/kiln/tests/l10n_catalogue.rs that sets or reads the process-global locale holds one static, poison-tolerant guard for its whole run and sets the locale it expects under it; no production code changes"
    required: true
    manual: false
  - id: "C2"
    text: "cd kiln \u0026\u0026 cargo test -p kiln --test l10n_catalogue passes 20 times in a row under plain cargo test (threaded), and cargo nextest run -p kiln --test l10n_catalogue passes"
    required: true
    manual: false
  - id: "C3"
    text: "scripts/check-no-gpui.sh passes"
    required: true
    manual: false
---

# CI (linux, kiln standalone): kiln l10n_catalogue tests race on the process-global locale under cargo test, so hide_is_titled_for_the_app sees pseudo-locale labels (Mac T1662)

# kiln l10n_catalogue tests race on the process-global locale

Lent from the Mac (lend y87, Mac T1662). GitHub CI linux job "Kiln builds and tests on its own" has been red since
Mac 081bb012: under plain `cargo test` (threads, not nextest's process per test) the tests in
`kiln/crates/kiln/tests/l10n_catalogue.rs` run in parallel in one process. `fake_locale()` calls
`kiln_l10n::set_locales(&fake)` and `placed_menu_items_show_labels` / `menu_table_items_show_labels` call
`set_locales(&[])`, while `hide_is_titled_for_the_app` and `kiln_commands_and_command_keys_match` read labels.
So `hide_is_titled_for_the_app` can see pseudo-locale labels.

## Fix

Test-only. Serialize every test in that file that sets or reads the process-global locale with one static guard
(a `static LOCALE: Mutex<()>`, poison-tolerant: `lock().unwrap_or_else(|e| e.into_inner())`), held for the whole
test, and have each test that depends on a locale set the one it expects (`set_locales(&[])` for the default) under
the guard rather than relying on the state left by another test. Don't change kiln or kiln_l10n production code.
kiln/ comments must not name fe-* crates or fastedit (no_gpui boundary).
