---
task_uid: "01m6rmmm1bchgqkv4b7vtw00p2"
task: "T482"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "quickfix_fill_tests::grep_fills_one_item_per_matching_line: :grep foo over a temp project lists each matching line with path, row, column and text and jumps to the first"
    required: true
    manual: false
  - id: "C2"
    text: "quickfix_fill_tests::grep_respects_gitignore_and_skips_binaries"
    required: true
    manual: false
  - id: "C3"
    text: "quickfix_fill_tests::vimgrep_flag_g_lists_every_match_on_a_line and quickfix_fill_tests::vimgrep_globs_limit_the_files"
    required: true
    manual: false
  - id: "C4"
    text: "quickfix_fill_tests::no_match_reports_e480_and_keeps_the_old_list"
    required: true
    manual: false
  - id: "C5"
    text: "quickfix_fill_tests::lgrep_fills_only_the_panes_location_list"
    required: true
    manual: false
  - id: "C6"
    text: "quickfix_fill_tests::make_fills_the_list_from_the_tasks_matched_problems: with the fake terminal session, :make runs the default build task and, on exit, the list holds its matched problems and the first error is open; the Problems panel source is unchanged from T1331"
    required: true
    manual: false
  - id: "C7"
    text: "quickfix_fill_tests::make_without_a_build_task_says_so"
    required: true
    manual: false
  - id: "C8"
    text: "ex_quickfix_fill::vimgrep_splits_pattern_flags_and_globs and ex_quickfix_fill::vimgrep_accepts_other_delimiters"
    required: true
    manual: false
  - id: "C9"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-vim -p fe-workbench pass"
    required: true
    manual: false
---

# Fill the quickfix list from :grep, :vimgrep and :make, and the location list from :lgrep and :lvimgrep (Mac T1405)

Lent from Mac T1405 (lend y50, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

# Fill the quickfix list from :grep, :vimgrep and :make, and the location list from :lgrep and :lvimgrep

## Problem
With the quickfix model (previous card) nothing fills it. nvim users run `:grep foo`, `:vimgrep /foo/ **/*.rs` and `:make`, then walk the results with `:cnext`.

## Change
- Project search is the engine for `:grep` and `:vimgrep` (`crates/fe-project/src/search.rs`: `search_filtered` with `SearchQuery`, `PathFilter`, which already honours .gitignore, binary skipping and a result cap). New `crates/fe-workbench/src/quickfix_fill.rs`:
  - `:grep {pattern}` and `:lgrep`: the rest of the line is the pattern, a regex, case-smart (the project search's smart case), over the project root. An empty pattern uses the last search pattern (`/` register) when there is one, else E35. LazyVim sets `grepprg` to `rg --vimgrep`, so project search is the right engine and the quickfix title is `:grep {pattern}`.
  - `:vimgrep /pat/[gj] {glob...}` and `:lvimgrep`: the delimiter may be any non-word char; flags `g` (every match, not one per line) and `j` (do not jump to the first item). Globs (`**/*.rs`, `*.md`) become the `PathFilter` include list; none means the whole project. The pattern is the project search's regex (Rust regex syntax), not Vim's: documented in the module docs as the known divergence.
  - Runs off the main thread with a cancel flag and a result cap (the same cap as the search panel); the list replaces the current quickfix list when the run finishes and is pushed on the stack; unless `j`, `:grep`/`:vimgrep` jump to the first item. No matches: `E480: No match: {pattern}` and the list is untouched.
  - `:make` and `:lmake`: runs the project's default build task (`tasks.run_build`'s pick, `task_commands.rs`) through `Workbench::run_task` as `:make` with arguments is nvim's `makeprg`; when the task's session exits, the problems its matcher found (`task_problems.rs`, `task_run.rs` `OutputReader`) become the quickfix list titled `:make`, then the first error item is jumped to. The Problems panel still receives them exactly as T1331 does. No default build task is nvim's "nothing to run" message from `nothing_to_run`. Arguments after `:make` are ignored with a status-line note (tasks, not shell lines). An untrusted folder gets the existing trust toast.
- vim_host.rs: the arms that answer the not-yet variants of the previous card now call this module. fe-vim's parser is unchanged unless the pattern/flag split needs a pure parse function: if so add it to `crates/fe-vim/src/ex/quickfix.rs` (`parse_vimgrep(text) -> (pattern, flags, globs)`).

## Tests (new files)
- `crates/fe-workbench/src/quickfix_fill_tests.rs` (`quickfix_fill_tests::...`): `grep_fills_one_item_per_matching_line`, `grep_respects_gitignore_and_skips_binaries`, `vimgrep_flag_g_lists_every_match_on_a_line`, `vimgrep_globs_limit_the_files`, `no_match_reports_e480_and_keeps_the_old_list`, `lgrep_fills_only_the_panes_location_list`, `make_fills_the_list_from_the_tasks_matched_problems` (fake terminal session as task_run_tests do), `make_without_a_build_task_says_so`.
- `crates/fe-vim/tests/ex_quickfix_fill.rs` (bare ids) only if the parse function exists: `vimgrep_splits_pattern_flags_and_globs`, `vimgrep_accepts_other_delimiters`.

## Not in scope
Vim-regex translation, `:vimgrepadd`/`:grepadd`/`:cgetexpr`, user `makeprg`/`grepprg` options, running arbitrary shell lines for `:make`.

## Implementation notes

- Scope widened (respec, rev 3) to `quickfix.rs`, `quickfix_commands.rs`, `quickfix_commands_tests.rs`, `task_run.rs` and `task_commands.rs`: the not-yet arms lived in `quickfix_commands.rs` (its test asserted the old message), and `:make` needs to hear a run's problems.
- `task_run.rs`: `Workbench::run_task_then(task, heard, …)` is `run_task` plus a callback handed the same problems the Problems panel lists for the tab (`RunProblem`, columns in chars), once, for that session only (not a rerun's). The Problems panel path is unchanged. Tests' fake sessions leave the output in the `FakeTaskOutput` global.
- `task_commands.rs`: `default_build` is `tasks.run_build`'s pick; `read_tasks` and `nothing_to_run` are crate-visible.
- `parse_vimgrep` is an associated fn, `fe_vim::QuickfixAction::parse_vimgrep(args) -> (pattern, flags, globs)`, as `ex.rs`'s re-exports are out of scope. It follows nvim's `skip_vimgrep_pat()`: a word-char start is a one-word pattern without flags; an unclosed delimiter is E682; `f` is unsupported.
- Empty `:grep` stays E471, as the parser (and nvim) has it; the last-search rule applies to `:vimgrep //`.
- `:vimgrep` globs are anchored at the root as vim's are to its cwd (`*.md` is top-level only, `**/*.rs` recursive); `%` is the typed-in editor's file; absolute paths under the root work.
- `:grep` items are untyped (`QuickfixKind::Untyped`): `(1 of 3): text`, as nvim shows `rg --vimgrep` entries.
- With `j` the status line gets the stack summary. A capped search says `(the first 10000 matches)`.
