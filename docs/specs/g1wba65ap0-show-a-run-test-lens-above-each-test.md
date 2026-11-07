---
task_uid: "01m6rmf64xm4tzamg1wba65ap0"
task: "T1410"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "test_lens_tests::a_rust_test_gets_a_run_test_lens_on_its_attribute_row and test_lens_tests::lens_follows_edits_by_version"
    required: true
    manual: false
  - id: "C2"
    text: "test_lens_tests::code_lens_setting_off_adds_no_provider and test_lens_tests::no_lens_in_a_plain_text_file"
    required: true
    manual: false
  - id: "C3"
    text: "test_lens_tests::clicking_the_lens_runs_that_test_not_the_one_at_the_caret"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-workbench pass"
    required: true
    manual: false
---

# Show a Run Test lens above each test function, running it on click

# Show a Run Test lens above each test function, running it on click

## Problem
Run Test at Cursor (previous card) needs the caret in the test. VS Code and rust-analyzer show "Run Test | Debug" above each test, a click away. `fe-gpui` has the block-row lens machinery (`lens.rs`, `LensProvider`, used by merge conflicts) and the `editor.code_lens` setting exists (default true), but no workbench code adds a provider for tests.

## Change
- New `crates/fe-workbench/src/test_lens.rs`: a `LensProvider` per editor whose `lenses(snapshot)` returns, for each Test and Module runnable of the editor's parse (`test_runnables::runnables`, cached by `BufferSnapshot::version`), a `Lens` at the runnable's first row with one item `Run Test` (or `Run Tests` for a module) whose invocation is `tests.run_row` with the row as its argument. Registered with `EditorView::add_lens_provider` for editors whose language has runnables, gated by `editor.code_lens`.
- `tests.run_row` ("Run Test at Row", hidden from the palette) in `test_commands.rs`: the same as `tests.run_at_cursor` for the given row, without moving the caret.
- Only on editors showing a file inside a trusted root; none in diff views or the commit box.
- No settings change: `editor.code_lens = false` turns it off with the language server's lenses.

## Tests (new files)
- `crates/fe-workbench/src/test_lens_tests.rs` (`test_lens_tests::...`): `a_rust_test_gets_a_run_test_lens_on_its_attribute_row`, `lens_follows_edits_by_version`, `code_lens_setting_off_adds_no_provider`, `clicking_the_lens_runs_that_test_not_the_one_at_the_caret`, `no_lens_in_a_plain_text_file`.
- The lens row height and paint are `fe-gpui`'s existing, already covered; no screenshot criterion.
