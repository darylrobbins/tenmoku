---
task_uid: "01m5qphtrndtqqdkk20spt0kyw"
task: "T322"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-keymap tests/list_vim_keys.rs: with vim on, in ReferencesPanel k and j resolve to references_panel.select_prev and select_next; in ReferencesPanel, problems_panel and search_results g g, shift-g and o resolve to select_first, select_last and open (open_selected for search) on both platforms; with vim off none of the new keys resolve; no key is bound in an editor context"
    required: true
    manual: false
  - id: "C2"
    text: "list_vim_keys_tests::references: with three references listed and the panel focused, j j k g g shift-g move the selection and o opens the selected one in the editor, as enter does"
    required: true
    manual: false
  - id: "C3"
    text: "list_vim_keys_tests::problems: the same sequence over a Problems list with two files, where gg/G land on the first and last problem row (file rows are skipped, as j and k skip them; the Problems panel has no collapse) and o opens the diagnostic, as enter does"
    required: true
    manual: false
  - id: "C4"
    text: "list_vim_keys_tests::search: the same over search results where the selection can sit on a file header or a line: with a file collapsed G lands on the last visible row; o does what enter (search.open_selected) does: on a line it opens it, on a file header its first match, and on a folder header (View as Tree) it toggles the folder"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-keymap -p fe-workbench and cargo clippy -p fe-keymap -p fe-workbench --all-targets -- -D warnings pass; no existing test is edited except the binding inventories the new keys extend: the ReferencesPanel \u0026\u0026 vim context in fe-gpui command_tests, keymap_tests and vim/tests, the two o rows in fe-keymap day_one_bindings' card, and the References keymap test in fe-workbench tests/references_panel.rs"
    required: true
    manual: false
---

# Give the References, Problems and Search results lists the same vim keys: j and k in References, gg and G and o in all three, as in the tree (Mac T1130)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" The list panels disagree: Problems and Search results take j/k (T523), References takes arrow keys only, and none has gg, G or o, which the project tree has (fastedit.toml:854-860). A vim user should not have to remember which panel supports which keys.

## Current state

- References: fastedit.toml:903-916, context `ReferencesPanel` (references_panel.rs:60), commands references_panel.select_prev/select_next/open, no vim layer.
- Problems: :955-980, context problems_panel (+ vim flag from problems_panel.rs:452), j/k yes, no select_first/last.
- Search results: :694-730, context search_results, j/k yes; search.collapse_or_parent / expand_or_first_child on left/right (search_results_collapse_bindings.rs); no first/last.
- The Changes panel handles its own keys in git_panel.rs on_key_down (:923-990); that and its gg/G are the next card.

## Change

- Add select_first and select_last commands to the Problems and Search panels, and references_panel.select_first/last, each as a plain action like its siblings; add the `vim` flag to ReferencesPanel's key context (copy problems_panel.rs:452).
- fastedit.toml, one commented block per panel beside the existing ones: References j k; all three g g, shift-g, o (open). Same keys and meanings as the tree (nvim-tree / neo-tree).
- Keep this one coherent behaviour: "every list panel has j k gg G o". Counts are out of scope.

## Tests

New files only: list_vim_keys_tests.rs (declared in lib.rs with the other test modules) and fe-keymap tests/list_vim_keys.rs; use simulate_keystrokes as problems_panel_tests.rs does.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- Commands: `references_panel.select_first`/`select_last` (typed commands, handled by the panel while focused, as its siblings), `problems.select_first`/`select_last` (`ProblemsOp::SelectFirst`/`SelectLast`) and `search.select_first`/`select_last` (`SearchOp`). Each selects the first or last row its `j`/`k` can reach: a reference, a problem or related row, any shown search row (a collapsed file's lines aren't shown).
- `o` is bound to the panel's Enter command, so it does what Enter does. In Search that means a file header opens its first match and a folder header (View as Tree) toggles; C4 was respecced (revision 2) to say so, since `open_selected` on a file header doesn't toggle.
- The Problems panel has no collapse, so C3's "collapsed file" clause was narrowed to "file rows are skipped".
- Respec (revision 2) added five existing tests to scope. They list every binding or context and the new keys extend them: the three fe-gpui context lists gain `ReferencesPanel && vim`; fe-keymap `day_one_bindings::vim_safe` lists every binding of `search.open_selected` and `problems.open`, which gain `o`; and fe-workbench `tests::references_panel::the_default_keymap_binds_the_references_commands` pins the References commands' bindings, which gain `k`, `j` and `o`.
