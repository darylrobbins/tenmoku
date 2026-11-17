---
task_uid: "01m7c5fz9qzad60mekgycwqzns"
task: "T1854"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test tests::debug_hover::no_evaluate_is_sent_without_supports_evaluate_for_hovers passes (cargo nextest run -p fe-workbench debug_hover)"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::debug_hover::a_value_from_before_a_step_is_never_shown_after_it passes (epoch-tagged, §2)"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_hover::the_value_shows_above_the_language_servers_hover_with_up_to_20_children passes, read through the live-popover text hook, with the identifier's extent from the syntax tree"
    required: true
    manual: false
  - id: "C4"
    text: "Tests tests::debug_hover::debug_hover_and_debug_evaluate_show_a_popover pass"
    required: true
    manual: false
  - id: "C5"
    text: "Every command this card adds has its rows in typed_command_ids_tests.rs and command_catalog_tests.rs, and cargo nextest run -p fe-workbench typed_command_ids command_catalog passes"
    required: true
    manual: false
  - id: "C6"
    text: "On the Mac, the audit scene crates/fe-workbench/examples/screenshots/audit/debug_hover.rs renders in Yohen and Hakuji and its probes pass (recorded in the decision file); the scene must not skip on SCREENSHOTS_ALLOW_OTHER_SCALE, the popover with a value and its children"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Hover evaluation while stopped, above the language server's hover (debugger D11)

# Hover evaluation while stopped, above the language server's hover (debugger D11)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **s**.

## Problem

Resting on a variable while stopped should show its value.

## What to build (§5 "Hover to evaluate"; §11 D11)

- Hover with context `hover`, only where the adapter says `supportsEvaluateForHovers`,
  merged above the LSP hover in `LspHover` (`crates/fe-workbench/src/lsp.rs:2266`,
  as `ignore_assist::merged_hover` merges its part), with the identifier's extent from
  the syntax tree.
- `HoverContent` markdown (fe-gpui's hover is text only, `crates/fe-gpui/src/hover.rs:88`):
  type, value, up to 20 first-level children fetched once; no expansion in the popover.
- Commands `debug.hover` and `debug.evaluate`.

`lsp.rs`'s hover is also changed by Mac T1602 (lent to helium; caller counts), which
lands first; T1739 and T1741 touch other parts of `lsp.rs`.

## Not in this card

Inline values (D18), pinned hovers (T1603).

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

Platform: **Mac** — acceptance includes a macOS look check of an audit scene (§5, §10), which review fails when made on Linux. The headless criteria can be built on Linux first.
