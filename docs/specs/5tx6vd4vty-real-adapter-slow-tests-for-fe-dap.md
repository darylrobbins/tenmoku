---
task_uid: "01m7c5erktfz4ht95tx6vd4vty"
task: "T1844"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/dap-real.sh runs the #[ignore] tests in crates/fe-dap/tests/real_adapter.rs, installing debugpy from a wheel pinned by SHA-256 in the script into a scratch venv; debugpy_breakpoint_step_variables_evaluate passes locally on the Mac (output recorded in the decision file)"
    required: true
    manual: false
  - id: "C2"
    text: "lldb_breakpoint_step_variables_evaluate (lldb-dap, else codelldb) passes on the Mac for a tiny Rust fixture, and on Linux skips with a reason naming kernel.yama.ptrace_scope and its value where it forbids launching under ptrace (checked by reading /proc/sys/kernel/yama/ptrace_scope)"
    required: true
    manual: false
  - id: "C3"
    text: "The real tests record the message orders the adapters send (launch response before or after configurationDone, stopped before or after a step's response) in the decision file, as the first observations of §2's ordering claims"
    required: true
    manual: false
  - id: "C4"
    text: ".github/workflows/dap.yml runs scripts/dap-real.sh on ubuntu-latest and macos-latest, and a run of it passes (its link in the decision file); TESTING.md describes the group as it does lsp-real.sh"
    required: true
    manual: false
---

# Real-adapter slow tests for fe-dap: debugpy and an LLDB adapter through a breakpoint, a step, variables and an evaluate (debugger D16)

# Real-adapter slow tests for fe-dap: debugpy and an LLDB adapter through a breakpoint, a step, variables and an evaluate (debugger D16)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **s**.

## Problem

Every gate test uses fe-dap's fake. §2's ordering claims come from the
specification, not from watching an adapter (§1 "Spike": the probe was never run).

## What to build (§11 D16; §10; §13 Q5)

`#[ignore]` tests in the style of `scripts/lsp-real.sh` and `.github/workflows/lsp.yml`:

- debugpy (a pinned wheel hash in the script) runs a Python fixture through one
  breakpoint, a step, variables and an evaluate;
- lldb-dap or codelldb does the same for a tiny Rust binary;
- native tests skip, naming `ptrace_scope`, where it forbids them;
- a `dap.yml` workflow like `lsp.yml`.

**Where it runs (Q5, §13):** real native adapter tests run on a GitHub runner and on
the Mac only. helium's `ptrace_scope` is 2, so only debugpy could run there.

## Notes

- A new workflow file needs a human risk approval at review (workflow edits do);
  say so in the hand-in.
- Downloaded wheels go in a scratch directory outside the repo, run with `-I`.

## Not in this card

delve (D19), netcoredbg (D22), js-debug (D23b), java-debug (D21b).

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

Platform: **Mac or CI only** — not Linux-OK for helium (`ptrace_scope` 2 forbids the
native half; Q5). Run it on the Mac lane.
