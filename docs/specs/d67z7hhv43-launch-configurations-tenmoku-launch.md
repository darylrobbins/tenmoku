---
task_uid: "01m7e4x8t6bybyjbd67z7hhv43"
task: "T655"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Tests in tests::launch_defs (cargo nextest run -p fe-workbench launch_defs) pass for every field and diagnostic, by table tests mirroring task_defs_tests.rs, including the_records_example_file_parses_whole with each adapter_args table on its own entry"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::launch_defs::an_attach_entry_is_listed_and_cannot_start passes, with \"attach isn't supported yet\""
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::launch_defs::command_and_input_variables_give_a_diagnostic_and_cannot_start passes"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::launch_defs::launch_json_with_comments_trailing_commas_and_an_osx_or_linux_override_imports passes, reading JSONC through fe-config's existing stripper (made public from user_snippets.rs, not a second one)"
    required: true
    manual: false
  - id: "C5"
    text: "Tests tests::launch_defs::a_launch_toml_name_wins_over_an_imported_one and tests::launch_defs::an_unknown_type_lists_no_adapter_for_type_x pass"
    required: true
    manual: false
  - id: "C6"
    text: "Tests tests::launch_defs::cargo_binaries_are_detected_with_a_json_build_step and tests::launch_defs::an_open_python_file_offers_debug_current_file pass"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Launch configurations: .tenmoku/launch.toml, Cargo and Python detection, and a read-only .vscode/launch.json import (debugger D4) (Mac T1845)

# Launch configurations: .tenmoku/launch.toml, Cargo and Python detection, and a read-only .vscode/launch.json import (debugger D4)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **l**.

## Problem

A debug session starts from a configuration: which adapter, which program, which
arguments. Users bring VS Code's `launch.json`; Tenmoku's own file and detection
must cover the common Rust and Python cases with nothing configured.

## What to build (§4; §11 D4; §13 Q2)

A data model only, tested through `ProjectFiles` (no disk), in a new
`crates/fe-workbench/src/launch_defs.rs` mirroring `task_defs.rs`:

- **`.tenmoku/launch.toml`** (Q2: a sibling file, not a `[[debug]]` table) with §4's
  `[[launch]]` schema and the `adapter_args` passthrough table; the same diagnostic
  shape and rules as `task_defs.rs` (later duplicates dropped, unknown keys reported).
- **Variables** of §4, with `${command:…}` and `${input:…}` refused by diagnostic.
- **`attach`** entries parse, listed as not startable until D20.
- **Detection**: a "Debug <bin>" per Cargo binary target, with its
  `cargo build --bin <bin> --message-format=json-render-diagnostics` pre-launch step
  (`program` comes later from cargo's artifact JSON, D7); "Debug current file" for an
  open Python file. Go is D19's; Java is D21b's.
- **`launch.json` import**, read-only, JSONC-tolerant, after `launch.toml`'s entries;
  `type` mapping through D3's aliases; `linux`/`osx` sub-objects; ignored keys with a
  diagnostic (§4).
- **JSONC:** T480 was abandoned, but `crates/fe-config/src/user_snippets.rs` already
  has `strip_jsonc` (comments and trailing commas, blanked in place). Move it to a
  public `crates/fe-config/src/jsonc.rs` and use it from both.
- fe-workbench gains its `fe-dap` dependency here, for D3's alias table.

## Not in this card

Starting anything (D7), the java type and detection (D21b), Go (D19), attach (D20).

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
