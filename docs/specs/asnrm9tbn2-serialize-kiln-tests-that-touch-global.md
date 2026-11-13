---
task_uid: "01m796pfbzsntw7wasnrm9tbn2"
task: "T592"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-platform lib tests and the default_keys binary pass 50 consecutive runs at --test-threads=8"
    required: true
    manual: false
  - id: "C2"
    text: "menus::tests::the_window_menu_keeps_its_role_when_translated and the_app_name_is_quoted_into_the_bundled_menus pass under cargo test --workspace in kiln/"
    required: true
    manual: false
---

# Serialize kiln tests that touch global locale and the app-name arg (Mac T1757)

Two linux tests in kiln's stand-alone workspace (`cargo test --workspace` in kiln/, threads in one
test binary) race on process-global l10n state:
 - kiln/crates/kiln-platform/src/menus.rs:394 `menus::tests::the_window_menu_keeps_its_role_when_translated`
   calls `set_locales(qps-ploc)` then `set_locales(&[])`; a concurrent kiln-platform unit test that
   also calls set_locales (locale.rs, command_actions.rs tests) flips the locale mid-assert.
 - kiln/crates/kiln/tests/default_keys.rs:306 `the_app_name_is_quoted_into_the_bundled_menus` got
   "Quit Notes": the `$app` label arg set by another test's `kiln::init(config())` (app name "Notes")
   leaks into this test's menu build. Introduced by 31e67284a and cfb81f762 (i18n K1/K2).

Fix by test isolation, no behaviour change: a shared mutex guard for tests that touch global locale
or the app-name arg. (a) kiln-platform: a cfg(test) helper (`test_lock.rs`, declared from lib.rs under
cfg(test)) returning a `MutexGuard` (ignore poison); take it in every kiln-platform unit test that calls
set_locales (menus.rs, locale.rs, command_actions.rs) and have the guard reset locales to `&[]` on drop.
(b) default_keys.rs: take a file-local lock in each test that calls `catalog(cx)`/`kiln::init`, and
set the app name explicitly inside the quoting test after init. If the real cause turns out to be
production code (menu_bar_for reading the global $app instead of Config's name), say so in a
decision note and respec; that makes it a bugfix needing a base-failing regression test.

Verify on Linux: run both binaries 50x with `--test-threads=8`; `cargo test --workspace` in kiln/.
Check locale.rs and command_actions.rs for which tests actually call set_locales before editing;
touch only those.
