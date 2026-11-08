---
task_uid: "01m6swfk63ajmghdybd8myp1jh"
task: "T1490"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "quickfix_commands_tests::a_superseded_landing_says_nothing"
    required: true
    manual: false
  - id: "C2"
    text: "quickfix_commands_tests::a_failed_open_lands_no_message"
    required: true
    manual: false
---

# Quickfix landing: skip the message when a newer landing superseded it or the open failed (follow-up of T1404)

Review follow-ups (Mac T1404, ae1a3dc9), F1 (medium) and F2 (low): quickfix_commands.rs `Workbench::land_on`.

F1: the landing continuation (`cx.spawn(...).detach()`) has no staleness check. Vim off, :cnext onto a slow-loading 2nd item, then :cnext onto an already-open 3rd: the 2nd's completion still says "(2 of 3)" after the 3rd's message, though the list selects the 3rd.
F2: `open_document_at` resolves on a failed open too, and the continuation always publishes the landing message, so an unreadable path or a directory gets an info toast "(n of m) ..." (`landed_editor` is None and `say` falls back to a toast).

Fix, one change in `land_on`: take a per-window landing sequence number (kept in the `Lists` global, bumped by every `Outcome::Land`) and, once the open resolves, say the message only if the number is still current and `landed_editor` found the tab; otherwise say nothing. A failed open leaves the list selection as it is.

Regression tests (base APIs only): a slow-opening 2nd item superseded by an open 3rd announces only the 3rd's message; a directory item produces no landing toast. Both fail at base. NOTE: at base no public API fills a list (`push_quickfix_list` is pub(crate)), so these can only live in the existing src-declared quickfix_commands_tests.rs, not a tests/ file; this card's scope names that file.
