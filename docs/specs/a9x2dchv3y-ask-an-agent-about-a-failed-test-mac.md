---
task_uid: "01m7f03g1q1nen51a9x2dchv3y"
task: "T391"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests in tests::test_ask_agent (cargo nextest run -p fe-workbench) pass: the_button_prefills_the_chat_with_assertion_and_output_cut_to_the_item_limit, it_attaches_the_test_function_when_in_the_root, it_is_disabled_when_chat_is_unavailable"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
---

# Ask an agent about a failed test (Mac T1590)

# Ask an agent about a failed test

Spec: `docs/design/design-system.md#runs-builds-tests-and-agent-tasks-in-one-list`
("Debug this test or Ask an agent").

## Change (`crates/fe-workbench/src/test_run_view.rs`)

- In the failed test's action row (left empty by the failure detail card), an
  **Ask an agent** button: `ask_agent` (T1588) with `send: false`, the
  prompt naming the test and asking why it fails, a text item with the
  assertion (left, right) and the captured output (cut to the chat's item
  limit from the end), and a `rows_item` for the test's function when its file
  is in the root.
- Command `runs.ask_agent_about_failure` in the palette with a failed test
  selected.

## Not in this card

Debug this test (the debugger entry-points card), a prompt for the whole run's failures.

## i18n

Every user-visible string this card adds goes through the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5): `tr!` / `text!`, whole
messages, plural selectors for counts, keys `workbench-test-runs-…` in
`crates/fe-workbench/l10n/en-US/test_runs.ftl`; a new file is added to `resources!` in
`src/l10n.rs` and to `l10n-allow.toml`'s `migrated` list. Tooltips and
accessible names count; a new command's title is the key `command-<id>` in the
same file. Tests build expected text through the catalogue.

Platform: **Linux-OK**
