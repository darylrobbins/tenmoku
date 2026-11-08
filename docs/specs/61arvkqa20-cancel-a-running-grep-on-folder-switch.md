---
task_uid: "01m6w2zby36bncs061arvkqa20"
task: "T523"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test quickfix_grep_root passes, including a_root_change_mid_search_leaves_the_list_alone"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim --test grep_empty passes, including an_empty_grep_reuses_the_last_search (E35 with no previous search; :vimgrep with no pattern still E471)"
    required: true
    manual: false
---

# Cancel a running :grep on folder switch; empty :grep reuses the last search (T1405 F7 F8) (Mac T1496)

Follow-up of T1405. crates/fe-workbench/src/quickfix_fill.rs and the :grep parsing in crates/fe-vim/src/ex/quickfix.rs.
- F7: a running :grep/:vimgrep search is not cancelled on `RootChanged` (see how markdown_preview.rs / settings_editor.rs subscribe), so switching folders mid-search pushes the old project's results into the new window. Subscribe in the same place the search is registered (Searches map) and set the cancel flag, dropping the pending fill.
- F8: `:grep` with no pattern should reuse the last search pattern (vim's behaviour; E35 `No previous regular expression` when there is none). The parser (ex/quickfix.rs, `args.is_empty() && kind != Kind::Make`) returns E471 for it. Parse an empty :grep as the last-search pattern for the host to resolve; :vimgrep with no args stays E471.
Do not touch quickfix_commands.rs (T1490 is active there); depend on T1490.


Regression tests use base APIs only. If a named regression test cannot reach the code through base APIs, block and ask the lead for a respec rather than widening scope.
