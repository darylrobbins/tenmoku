---
task_uid: "01m5rsr9dyh2e3fk6s33re5arb"
task: "T339"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "lsp_links::landing_tests::a_link_clicked_right_after_an_edit_opens_its_target (in lsp_links_landing_tests.rs): cmd-click and Open Link on a target-less link right after an unrelated edit, before the debounce expires, re-ask document_links at the current version, match the link, resolve and open it (no 'This link has no target' toast); fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "lsp_links::landing_tests::a_superseded_link_open_is_dropped (in lsp_links_landing_tests.rs): resolution and the file-existence check carry the originating editor and an open-generation token; switching panes during resolution, or an older file check completing after a newer navigation, opens nothing (uses the new Origin type, so it is not a base regression test). Known limit (T353): the activation counter is also advanced by the deferred ActiveChanged event, so a link open started in the same update as a pane or tab activation can have both completion guards discard a valid open (review F1); T353 captures a synchronous activation epoch instead."
    required: true
    manual: false
  - id: "C3"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Resolve document links against the current buffer version and drop superseded link opens (T330 F9, F10)
