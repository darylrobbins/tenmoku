---
task_uid: "01m77zvt269makhcqbkhgjvqe7"
task: "T1737"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`tests::add_ons::merge_completion::two_lists_merge_and_trigger_char_asks_only_its_server` and `tests::add_ons::merge_completion::incomplete_server_reasks_both_at_new_version` pass"
    required: true
    manual: false
  - id: "C2"
    text: "`tests::add_ons::merge_collisions::completion_collisions_give_same_list_in_both_orders` and `tests::add_ons::merge_collisions::code_action_collisions_give_same_list_in_both_orders` pass, covering every collision case listed in section 7"
    required: true
    manual: false
  - id: "C3"
    text: "`tests::add_ons::merge_lists::one_server_failing_still_gives_the_others_results` passes for hover, locations, actions, lenses, hints, links and colours"
    required: true
    manual: false
  - id: "C4"
    text: "`tests::add_ons::workspace_symbols::mixed_languages_each_server_asked_once_and_duplicates_listed_once` passes"
    required: true
    manual: false
---

# Merge completion, hover, navigation, workspace symbols, code actions, lenses, hints, links and colours across servers in fe-lsp (LSP add-ons A7, T1645)

# Merge completion, hover, navigation, workspace symbols, code actions, lenses, hints, links and colours across servers in fe-lsp (LSP add-ons A7, T1645)

Spec: §5 (merge rules, completion versions and dedupe, code-action dedupe, workspace symbols, colours precedence), §6.2 (`LspNavigation` row), §7 (Merging, Owners), §8 row A7.

Row: merged features in fe-lsp: completion (one version per list, re-ask every server on any `isIncomplete`, equivalence-only dedupe, trigger union), hover, the goto family and references with `LspNavigation` readiness over `language_servers`, workspace symbols (dedupe over the existing all-sessions enumeration), code actions (equivalence-only dedupe), lenses, inlay hints, links and colours (precedence by range), with one server's failure tolerated.

Test files: crates/fe-lsp/src/tests/add_ons/merge_completion.rs, merge_lists.rs (hover, locations, actions, lenses, hints, links, colours; one failing), merge_collisions.rs (each completion and code-action collision case in BOTH orders of arrival, per §7), workspace_symbols.rs (mixed languages).

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.
