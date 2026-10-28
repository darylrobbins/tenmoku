---
task_uid: "01m5zzv69z8evrtpbc3a4mtgmt"
task: "T254"
rev: 1
type: "chore"
complexity: ""
criteria:
  - id: "C1"
    text: "In hierarchy_panel_tests.rs and type_hierarchy_tests.rs every wait on an expanding row names the expected row text, not a row count the Loading placeholder satisfies"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest on fe-workbench hierarchy_panel passes 3 consecutive runs with the load average logged"
    required: true
    manual: false
---

# Flake: hierarchy panel tests wait on a row count that the Loading placeholder already satisfies

Flake in the fe-workbench hierarchy panel tests, found in neon's post-merge checks of T232 (frozen neon) and in T231's gate.

Failures (test_all, load average 14-16), a different test each time, all the same shape: the test waits with `s.until("...", |s| texts(s).len() == N)`, but the lazy "Loading…" placeholder row already makes the row count N before the server reply is applied, so the assertion that follows reads "Loading…":
- type_hierarchy_tests::show_type_hierarchy_lists_supertypes (type_hierarchy_tests.rs:104-105): left [..., "    Loading…", ...], right "    No supertypes"
- hierarchy_panel_tests::expanding_loads_incoming_calls_lazily (hierarchy_panel_tests.rs:192-193): left "    Loading…", right "    c"
- type_hierarchy_tests::a_cyclic_hierarchy_is_marked (:168): "Loading…" instead of "a (recursive)"

Fix (test-only): every `until` in these two files that counts rows while an expansion is loading must wait for the expected row text instead (for example `texts(s).last() == Some("    c")` or "no supertypes" present), not the row count. Check the sibling waits at type_hierarchy_tests.rs:126, hierarchy_panel_tests.rs:321, :454 and :500. Keep each test's assertions otherwise identical.

Criteria are three consecutive runs of both files under load with the load logged.

## Implementation notes

- All row-count waits on expanding rows now wait for the full expected rows (`:454`/`:500` wait for the "Loading…" row itself).
- Carried F2 fixed: the superseded-read test asserts the workbench owner holds focus before and after.
- Carried F1 not fixed: with focus still on the panel and a stale request, `Pane::file_landed` restores `before` (the panel) either way, so the request predicate has no focus-observable effect; a test would need a production hook.
- Three consecutive `cargo nextest run -p fe-workbench hierarchy_panel` runs passed (35/35) at load average 25.6, 26.0, 26.4 (before the show-helper change; rerun after it: 3.1, 3.1, 3.1).
- Both `show` helpers now take the expected leading rows and wait for them (T254 review F1).
