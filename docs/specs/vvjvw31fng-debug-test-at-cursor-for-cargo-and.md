---
task_uid: "01m7c5g3qh7mkckevvjvw31fng"
task: "T1856"
rev: 4
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Test tests::debug_test::a_rust_test_in_a_workspace_member_launches_its_test_binary passes (cargo nextest run -p fe-workbench debug_test): built with --no-run --message-format=json-render-diagnostics (approved by lead core: compile errors must reach the build tab and Problems; the artifact messages are the same JSON), launched with [name, \"--exact\", \"--nocapture\"], cwd at the artifact's manifest_path directory and CARGO_MANIFEST_DIR set"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::debug_test::a_pytest_node_id_launches_debugpy_with_module_pytest passes"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_test::run_and_debug_resolve_the_same_test passes: tests.run_at_cursor and debug.test_at_cursor consume one resolved test (program, args, cwd, env and framework) from test_runnables.rs, and Run's command lines are unchanged (test_commands_tests pass)"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::debug_test::a_go_test_is_refused_until_delve passes, with \"Go tests need delve\""
    required: true
    manual: false
  - id: "C5"
    text: "Tests tests::debug_test::the_lens_reads_run_test_and_debug and tests::debug_test::debug_this_test_in_a_failure_opens_a_debug_session pass (the lens text `▷ Run test · Debug`, the failure detail's Debug this test, an accessible button). Known limit (lead core): the lens's Debug item, like every code lens item, is painted text with a mouse hitbox and no accessibility node; T1964 makes code lens items screen-reader buttons. Keyboard and screen-reader users reach the same action through \"Debug: Debug Test at Cursor\" in the palette and \u003cleader\u003etd"
    required: true
    manual: false
  - id: "C6"
    text: "Test vim_remaps::vim_lazyvim_debug_tests::leader_td_dispatches_debug_test_at_cursor passes, and lazyvim_layout_is_the_table passes with the \u003cleader\u003etd row"
    required: true
    manual: false
  - id: "C7"
    text: "Every command this card adds has its rows in typed_command_ids_tests.rs and command_catalog_tests.rs, and cargo nextest run -p fe-workbench typed_command_ids command_catalog passes"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Debug Test at Cursor for Cargo and pytest, a resolved test shared by Run and Debug, and Debug on the test lens and in a test failure (debugger D13)

# Debug Test at Cursor for Cargo and pytest, a resolved test shared by Run and Debug, and Debug on the test lens and in a test failure (debugger D13)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **l**.

## Problem

"Debug the test I'm in" is the most common way into a debugger. Run Test at Cursor
landed (Mac T1409, as `test_runnables.rs` and `test_commands.rs`) without a resolved
test type: it builds command lines, which Debug must not parse back (§7).

## What to build (§7 "Debug Test at Cursor"; §11 D13; §13 amendments)

- **The seam (§13):** a resolved test target in `test_runnables.rs` — label,
  program, args, cwd, env and a `TestFramework` (`Cargo { package, target, name }`,
  `Pytest { node_id }`, `GoTest { package_dir, name }`) — that Run and Debug both
  consume (§7's `ResolvedTest` sketch). `tests.run_at_cursor` moves onto it with no
  change to what it runs.
- **`debug.test_at_cursor`**: Cargo through `--no-run --message-format=json`, the test
  executable from the artifact message, cwd and `CARGO_MANIFEST_DIR` from its
  `manifest_path` (not the resolved cwd); pytest through debugpy's `module = "pytest"`;
  a `GoTest` refused with "Go tests need delve" until D19.
- **Entry points (§13):** the test lens reads `▷ Run test · Debug` (`test_lens.rs`,
  T1410 landed; T1605 adds history to it first) and the failure detail's Debug this
  test (`test_run_view.rs`, from T1577 and T1590, which land first) opens it.
- `<leader>td` in the LazyVim preset.

## Notes on shared files

`test_runnables.rs`/`test_commands.rs` (T1575), `test_lens.rs` (T1605) and
`test_run_view.rs` (T1576, T1577, T1590) are in ready cards' scopes; this card waits
for them.

## Not in this card

Go (D19), C# tests (D22), Tests in File for Debug.

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Linux-OK** (headless tests only; no screenshot goldens move).

## Implementation notes

Design calls, the lead-approved message format (C1) and the known limits are in
`docs/decisions/vvjvw31fng-debug-test-at-cursor.md`. The scope gained
`crates/fe-workbench/src/tests/test_lens_history.rs`, which pinned the test lens's
item positions that the Debug item moves.
