---
task_uid: "01m7f02wge0adwxenex9xfp4q3"
task: "T390"
rev: 3
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "module rows are keyed by binary identity and module path separately: a library module p::checks and an integration target checks of package p get separate rows, counts, failure glyphs and labels"
    required: true
    manual: false
  - id: "C2"
    text: "expanding one of the two rows leaves the other collapsed, and module selection picks only its own target's tests"
    required: true
    manual: false
  - id: "C3"
    text: "the regression test in src/tests/test_run_view_module_keys.rs fails at base and passes with the fix"
    required: true
    manual: false
---

# Test run detail: a library module and an integration target with the same path no longer merge into one module row (T1576 F9) (Mac T1876)

Review follow-up of T1576 (test run detail), finding F9 (medium, carried after round 4).

crates/fe-workbench/src/test_run_view.rs ~l.240: module keys join the binary identity and the module path with "::", so a library test ("p", "checks::shared") and an integration test ("p::checks", "shared") both get module key "p::checks". Their modules merge: counts, failure glyphs and expansion state combine, and two indistinguishable "shared" rows show. Executable test ids already keep selection and history apart; only the module grouping is left.

Fix: a structured module key (binary identity and module path as separate fields) used throughout grouping, expansion and module selection, with the display label formatted separately.

Regression test in a new file crates/fe-workbench/src/tests/test_run_view_module_keys.rs (declared from src/tests/mod.rs): a library module and an integration target both named `checks`, each with a `shared` test; assert separate module rows, separate counts and labels, and that expanding one leaves the other collapsed. It fails at base (the rows merge).

Platform: Linux-OK (headless view model; no screenshots or audit scene change expected).
