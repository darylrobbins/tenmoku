---
task_uid: "01m6z25fk8jtwfsy2j7sm1hmsh"
task: "T311"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In kiln/, 'cargo test --locked --offline -p kiln --test l10n_catalogue' passes 30 times in a row (e.g. a shell loop), with the default test threads"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p kiln --test l10n_catalogue passes"
    required: true
    manual: false
  - id: "C3"
    text: "Only kiln/crates/kiln/tests/l10n_catalogue.rs changes"
    required: true
    manual: false
---

# Flake: kiln l10n_catalogue hide_is_titled_for_the_app sees another test's fake locale under cargo test (kiln stand-alone CI step, linux) (Mac T1627)

CI's linux job fails main (runs 42534535559 on 4328833e and 42537655127 on b7b2a1de) in the step that checks kiln stands alone (`cargo test --locked --no-fail-fast --offline --workspace` run inside kiln/):

    ---- hide_is_titled_for_the_app stdout ----
    panicked at crates/kiln/tests/l10n_catalogue.rs:78:5:
      left: ["Hide L10n Catalogue", "⟦Pröbé Läbéls⟧"]
     right: ["Hide L10n Catalogue", "Probe Labels"]

Cause: `cargo test` runs a binary's tests as threads of one process (nextest gives each its own process, which is why the workspace gates pass). `placed_menu_items_show_labels` and `menu_table_items_show_labels` install a fake locale with the process-wide `kiln_l10n::set_locales` (`fake_locale()`), and only reset it with `set_locales(&[])` afterwards. `hide_is_titled_for_the_app`, running at the same time, reads labels while the fake locale is in force. The tests came with T1523.

Fix in the test file only: make every test in kiln/crates/kiln/tests/l10n_catalogue.rs that reads or sets the locale hold one shared lock (e.g. a `static LOCALE: Mutex<()>`, poison-tolerant) for its whole body, and reset the locale even when an assertion fails (a guard that calls `set_locales(&[])` on drop). No production change: this is a test-only flake fix, so prove it with repeated runs rather than a regression test.

## Implementation notes

Validation at d703521e, run in `kiln/`:

- C1: `for i in $(seq 30); do cargo test --locked --offline -p kiln --test l10n_catalogue; done` with default test threads: 30 of 30 runs `test result: ok. 6 passed; 0 failed`. Before the fix, 28 of 30 runs failed (`hide_is_titled_for_the_app`, `placed_menu_items_show_labels`).
- C2: `cargo nextest run -p kiln --test l10n_catalogue`: `6 tests run: 6 passed, 0 skipped`.
- C3: the code change is `kiln/crates/kiln/tests/l10n_catalogue.rs` only; this note is the spec's own appendix.
