---
task_uid: "01m6svcbv3vxav8mkyhz24qgcv"
task: "T493"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "todo_picker_tests::lists_todo_comments_across_the_project_with_their_kind and todo_picker_tests::a_leading_keyword_filters_by_kind"
    required: true
    manual: false
  - id: "C2"
    text: "todo_picker_tests::respects_gitignore, todo_picker_tests::enter_opens_at_the_line and todo_picker_tests::caps_at_5000_and_says_so"
    required: true
    manual: false
  - id: "C3"
    text: "todo_commands_tests::next_and_previous_walk_the_buffers_hits_without_wrapping and todo_commands_tests::at_the_last_hit_next_toasts"
    required: true
    manual: false
  - id: "C4"
    text: "vim_lazyvim_preset_tests::lazyvim_layout_is_the_table (rows for st, xt, ]t, [t) and vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent pass"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-workbench -p fastedit pass"
    required: true
    manual: false
---

# List the project's TODO comments in a picker, and jump with ]t and [t (LazyVim <leader>st, <leader>xt) (Mac T1414)

# List the project's TODO comments in a picker, and jump with ]t and [t (LazyVim <leader>st, <leader>xt)

## Problem
todo-comments.nvim adds `:TodoTelescope` / `:TodoQuickFix` (`<leader>st`, `<leader>xt` in LazyVim) to list every TODO in the project and `]t`/`[t` to jump between them in a buffer. Tenmoku has project search (`fe-project/src/search.rs`) but no one-key view of TODO/FIXME comments.

## Change
- Command `todo.list` ("Search TODO Comments"): a kiln-shell `Picker` in `crates/fe-workbench/src/todo_picker.rs`, rows `KIND  text  path:row`, the keyword badge in the kind's colour (the previous card's `Kind` -> tint), filtered by subsequence, a typed leading keyword (`FIX `, `TODO `) restricting the kind. Rows come from `fe_project::search_filtered` run off the main thread with the regex built from the keywords (`\b(TODO|FIX(ME|IT)?|BUG|HACK|XXX|WARN(ING)?|PERF|OPTIMIZE|NOTE|INFO|TEST(ING)?)(\([^)]*\))?:`), then each hit line is confirmed with `todo_comments::scan` against the file's comment ranges only when the file has an open editor with a parse (other files keep the regex hit; documented, matches todo-comments' own ripgrep behaviour). Capped at 5,000 rows; the header says so when capped. Enter opens at the hit, moving the selection previews it as the symbol picker does.
- `todo.next` / `todo.previous` ("Next/Previous TODO Comment"): move the caret to the next/previous hit of the active editor from `todo_comments::scan` over the whole buffer (not just the visible rows), wrapping nowhere, selecting nothing, toast "No more TODO comments" at the ends. Reuses the previous card's scanner.
- Commands in a new `crates/fe-workbench/src/todo_commands.rs` (typed commands; `workbench_commands.rs` untouched). Preset: `<leader>st` -> `todo.list`, `<leader>xt` -> `todo.list`, `]t` -> `todo.next`, `[t` -> `todo.previous`; `xt` and `st` removed from lazyvim.vimrc's "Unmapped" list; TABLE rows.

## Tests (new files)
- `crates/fe-workbench/src/todo_picker_tests.rs` (`todo_picker_tests::...`): `lists_todo_comments_across_the_project_with_their_kind`, `a_leading_keyword_filters_by_kind`, `respects_gitignore`, `enter_opens_at_the_line`, `caps_at_5000_and_says_so`.
- `crates/fe-workbench/src/todo_commands_tests.rs` (`todo_commands_tests::...`): `next_and_previous_walk_the_buffers_hits_without_wrapping`, `at_the_last_hit_next_toasts`.
- `vim_lazyvim_preset_tests::lazyvim_layout_is_the_table` (four new rows) and `vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent` pass.
Linux-OK.

## Implementation notes
- `todo_picker.rs` and `todo_commands.rs` are declared from `workbench.rs` with `#[path]` (lib.rs is out of scope); the commands sit on a `TodoCommands` unit struct and the root element handles their actions (`Workbench::attach_todo_commands`), as `picker_commands.rs` does. New palette: `Palette::Todos`.
- A row per match, not per line. The badge shows the keyword as written (`FIXME`), in the colour the editor gives its kind's tint; the text is what follows the tag. The search's cap is 5,000 matches (`SearchOptions::max_matches`); when it stops there the status line reads "Stopped at the first 5,000 matches".
- The leading-keyword filter takes a keyword in capitals followed by a space (`FIX `, `TODO `); its kind is the scanner's (`FIX ` also lists `FIXME:` and `BUG:`). Lower case is an ordinary query.
- Confirmation: a file with an open editor whose parse is current (`Syntax::version` equal to the buffer's, not markdown, not over `MAX_SPELLABLE_BYTES`, highlighting complete) keeps a hit only if `todo_comments::scan` over the buffer's comments has a keyword at the same row and byte column. A hit whose buffer row doesn't have the keyword at that column (unsaved edits, another encoding) can't be checked and stays.
- A line over the search's 512-byte preview is read again once (bounded at 1 MiB; not for UTF-16 files, whose search offsets are of the transcoded text) for its tags, texts and char columns, so a second keyword far along or a long `(user)` still lists and Enter lands on the keyword. The picker records no history itself: `open_document_at` does (review round 1, F1–F3).
- With the cap reached, the empty-list messages keep the cap note ("No matching TODO comments: stopped at the first 5,000 matches"). (F4.)
- Comments are read in 64-row chunks (`todo_commands::CHUNK_ROWS`), each retried up to four times: `Syntax::checked_highlights` has a 2 ms budget doubling to 16 ms per range, so one query over a whole large buffer may never finish. The picker reads each chunk holding a hit of an open file once; `todo.next`/`todo.previous` read chunks out from the caret's until a keyword turns up. A buffer over `MAX_SPELLABLE_BYTES` gets its own toast from them instead of "No more TODO comments".
- `todo.next`/`todo.previous` toast "TODO comments aren't known yet: the file is still being parsed" while the parse is behind or highlighting is unfinished, instead of claiming there are none. They are motions, not jumps: no jump-list entry, as todo-comments sets the cursor directly.
- `<leader>st` was never in the "Unmapped" list; only `<leader>xt` came out.
