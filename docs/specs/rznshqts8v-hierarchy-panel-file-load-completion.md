---
task_uid: "01m5r6z6ck6knxpsrznshqts8v"
task: "T240"
rev: 3
type: "bugfix"
complexity: ""
criteria: []
---

# Hierarchy panel: file-load completion must not steal focus the user moved to another panel or terminal after pressing Enter; restore focus only if the originating panel still owns focus and its navigation request is current, plus a delayed-load test (T218 F7)

## Revision note (review round 2)

The regression test for this card is the focus-moved test only (`a_file_landing_after_focus_moved_does_not_take_it_back`); it fails at the base and passes with the fix. Exercising the second-stage request guard through the production predicate with a controlled second-stage completion is a test-coverage gap carried by a follow-up card (T240 F1); it is a documented limit of this card, not a criterion of it.
