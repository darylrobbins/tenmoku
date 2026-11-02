---
task_uid: "01m6c9t42c8esb9t71fst1vjhv"
task: "T266"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test matchit_cycles_keyword_groups (crates/fe-vim/tests/matchit.rs) passes with nvim-probed expectations: the Lua if/elseif/else/end cycle with % and with g%, nested function/if/end, bash's row-position rule (echo if; if x; then :; fi lands on fi), case/esac, while/do/done, and make's ifeq/else/endif"
    required: true
    manual: false
  - id: "C2"
    text: "Test matchit_falls_back passes: with no language, or one without a table (rust, c), % is as before (brackets, /* */, #if rows); a bracket before the row's first keyword keeps bracket %; {count}% is unchanged; g% without a table fails and changes nothing"
    required: true
    manual: false
  - id: "C3"
    text: "Test matchit_operators_and_jumps passes: d% from if to end is charwise inclusive (x if a then y end z -\u003e x  z), v% and y% likewise, . repeats d%, and % and g% set '' when they land and leave it when they fail"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-vim passes, including which_key_table_keys_are_real with g% listed and every *_matches_neovim corpus test unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-gpui/src/commands.rs's host answers EditorHost::language_name with the buffer's language name"
    required: true
    manual: false
---

# fe-vim: % and g% cycle through Lua, bash and make keyword groups (if/elseif/else/end, function/end, case/esac, do/done, ifeq/else/endif) as nvim's matchit does, from the host's language (Mac T1291)

## Problem

The user approved matchit as a built-in vim feature on 2026-11-01: `%` should jump between keyword pairs
(if/elif/else/end, do/done, …), not only brackets.

Today `%` without a count (`motion::match_pair`, `crates/fe-vim/src/motion.rs:1610`) does nvim's built-in `%`
only: brackets (the first at or after the caret on its row), C comments `/* */`, and C preprocessor
`#if/#else/#endif` rows. Nothing knows the buffer's language: `motion::motion` sees only the text, and
`EditorHost` (`crates/fe-editor/src/commands.rs`) exposes `comment_tokens` and syntax lookups but no language.

Scope decision: a tree-sitter version (pairs from each language's grammar nodes) needs per-language node
knowledge and a new fe-syntax query per language; that's larger than the feature. This card does what nvim's
matchit does from `b:match_words`: per-language keyword tables, text only. It starts with the shipped languages
whose nvim ftplugin defines keyword groups and needs no skip expression: Lua, bash, make. HTML/XML/Markdown tags
are the next card; Ruby (its postfix `x if y` needs matchit's skip expression), SQL and PHP are later follow-ups.

Probed (nvim 0.12.6, `nvim --clean --headless --cmd 'packadd matchit' --cmd 'filetype plugin on' -l probe.lua`,
setting `vim.bo.filetype` per case; with no syntax on, `b:match_skip` skips nothing, so keywords in strings count,
which is what a text-only table does):
- Lua `if a then / x() / elseif b then / … / else / z = 'end' / end`, caret on `if`: `%` goes to `elseif`
  (3:0), `else` (5:0), then the `end` inside the string (6:7), then back to `if`. `g%` from `if` goes the other
  way: `end`, `else`, `elseif`.
- Nesting counts: `local f = function() if a then return end end` from col 0: `%` lands on the last `end` (col 42).
- Caret not on a keyword: the first keyword or bracket at or after the caret on its row, whichever starts first:
  `x = (a) if b then` from col 0 goes to `)` (col 6); from `then` in `if a then y end` (not a Lua keyword) it
  takes `end` and lands on `if`.
- Unbalanced: `elseif a then y end` from `elseif` lands on `end`.
- bash: a keyword counts only at the row's start (after blanks) or after `;` and blanks:
  `echo if; if x; then :; fi` from col 0 lands on `fi` (col 23). `case`→`esac`, `while … do`→`done`.
- `d%` from `if` to `end` is charwise inclusive (`x if a then y end z` → `x  z`).

## Change

1. Host: `EditorHost::language_name(&self) -> Option<&str>` (default `None`) in `crates/fe-editor/src/commands.rs`;
   fe-gpui's host gives `self.language`'s name (`crates/fe-gpui/src/commands.rs`, beside `comment_tokens`);
   `HeadlessVim::set_language(Option<&'static str>)` with its `ViewHost` forwarding it
   (`crates/fe-vim/src/headless.rs`).
2. `crates/fe-vim/src/line_motions.rs`: `with_view` looks the language up for `%` without a count and `g%`
   (motion, operator target, visual, and `.`), as it looks up `function_bounds` for `]m`, and only then.
3. New feature module `crates/fe-vim/src/feature/matchit.rs` (listed in `FEATURES`): the keyword tables for
   `lua`, `bash` and `make`, transcribed from nvim 0.12.6's `b:match_words` for `lua`, `sh`/`bash` and `make`
   (print them with the probe) into fixed Rust tables: each group an opener list, middle list and closer list,
   whole keywords only, with bash's and make's row-position rules. Not a Vim-regex engine (fe-vim's T417 went 4
   review rounds emulating patterns; don't). Its matcher: from the keyword under the caret (anywhere on it) or the
   first keyword at or after the caret on the row, counting nested groups, `%` goes to the next word of the
   group, from the closer back to the opener; `g%` goes to the previous one, from the opener to the closer. It
   lands on the word's first char. A bracket before the first keyword on the row keeps today's bracket `%`.
   With no language, or one without a table, `%` is exactly as today.
4. `crates/fe-vim/src/motion.rs`: `match_pair` consults the matchit table for the view's language before its
   bracket scan, so the motion, the `marks.rs` lands check (`crates/fe-vim/src/marks.rs:2300`) and operators all
   agree. A keyword match is charwise inclusive (not `Match.rows`). A failing `%` still fails as today.
5. `g%`: `Motion::MatchPairBackward` (`crates/fe-vim/src/args.rs`, serde `match_pair_backward`), taken by the
   feature's `g` hook; a jump, inclusive, with a lands arm in `marks::jump` (a failing jump must not set `''`); a
   title in `crates/fe-vim/src/which_key.rs` `BUILTIN_KEYS` (`g`: "match backward"), which
   `tests/which_key.rs::which_key_table_keys_are_real` requires. `g%` with no table for the language does
   nothing (fails), as nvim does without matchit's keywords.

Out of scope: `[%`, `]%`, `a%`; skipping keywords in strings and comments.

Tests: new `crates/fe-vim/tests/matchit.rs` (cases probed in nvim as above, hand-written: nvim's oracle corpus
can't set a filetype or load matchit). Probe every asserted value.

Note: the other drafter's undo-marks card may hold `crates/fe-vim/src/marks.rs`; order this after it if so.

## Implementation notes

- Probed with the local nvim 0.12.4 (the tables are the same as 0.12.6's `b:match_words` printed by the probe).
  nvim loads matchit by default even under `--clean`, so the fallback cases (no language, rust, c) were probed
  with `:normal!` for the built-in `%`; the keyword cases with `:normal` and `packadd matchit`.
- Tables (`crates/fe-vim/src/feature/matchit.rs`): Lua `do|function|if : return|else|elseif : end` and
  `repeat : until`; sh/bash `if : elif|else : fi`, `for|while : done`, `case : esac` where a command starts;
  make `ifeq|ifneq|ifdef|ifndef : else [ifeq…] : endif` at the row's start after spaces (the match, and so the
  landing, starts in column 0), `define : endef`, and nmake's `!if…/!else…/!endif`. Lua's long-bracket group
  (`[[ ]]`, `[==[ ]==]`, a backreference) is left out: not a keyword.
- Operators: matchit's `d%` takes the whole word it lands on when it moves forward (`CleanUp` in o mode), so a
  keyword ahead of the caret is taken to its last char; one behind is taken from its first char, as probed.
- `{count}g%` is sent as `{count}%` (matchit runs `normal! {count}%`); `Motion::MatchPairBackward` with a
  count fails.
- With a table, `g%` on a bracket or comment is `%` (probed), and on a C `#if` row goes the other way
  (`#if`→`#endif`, else back to the `#else`/`#if` before), as matchit's default group does.
- Divergences, documented in the module: matchit sets `''` when it finds a keyword even if `searchpair()` then
  fails; per C3 a failing `%`/`g%` here leaves it. A delete over keyword `%` still writes register `1`
  (built-in `%`'s rule; matchit's mapped `d%` doesn't); `g%` doesn't.
- `crates/fe-vim/src/which_key.rs` carried findings: T224 F1 fixed (keys a map has whole, `Exact` or
  `ExactAndPrefix`, list no built-ins); T224 F2 fixed (insert and replace mode list insert maps, the command line
  its maps, built-in tables only in normal and visual, nothing while `<C-r>` waits for a register name).
  Regressions are inline in `which_key.rs`.
- After merging main's T1281 (a shown which-key popup stops a held prefix timing out), F2's model fix made
  fe-gpui show a popup for insert mode's held `j` of `inoremap jk <Esc>`, which then never typed
  (`vim::remap_insert_tests` failed). `crates/fe-gpui/src/vim/which_key.rs`'s `which_key_now` now shows vim's
  popup in normal and the visual modes only, as its chord popup already did, so the editor behaves as before F2
  while `VimState::which_key` answers in every mode. A small edit outside the card's listed scope, forced by
  the merge.
