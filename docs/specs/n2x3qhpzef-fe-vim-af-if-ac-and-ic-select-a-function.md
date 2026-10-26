---
task_uid: "01m5qphza9c09kq6n2x3qhpzef"
task: "T229"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test definition_objects_delete_and_yank (top level, crates/fe-vim/tests/definition_objects.rs): with a host returning a canned chain, daf deletes the outer function, dif the body (linewise when whole lines inside braces), yac yanks the class, cic changes its body; the text and register match"
    required: true
    manual: false
  - id: "C2"
    text: "definition_objects_count_and_nesting: 2daf takes the enclosing function of a nested one, a count past the outermost fails and changes nothing, a caret in a class field takes the class for ac"
    required: true
    manual: false
  - id: "C3"
    text: "definition_objects_visual_grows: vaf selects the function and a second af grows to the next enclosing one; vif then af works; visual object.rs arm tested for all four objects"
    required: true
    manual: false
  - id: "C4"
    text: "definition_objects_repeat_and_carets: . repeats daf at another place, and with several carets each takes its own function"
    required: true
    manual: false
  - id: "C5"
    text: "definition_objects_without_syntax_cancel: with a host that has no syntax, daf/dif/dac/dic cancel like an unknown object (no edit, no message state left pending)"
    required: true
    manual: false
---

# fe-vim: af, if, ac and ic select a function or class and its body from the host's syntax (nvim-treesitter-textobjects), with counts and visual growth (Mac T1152)

## Problem

The user asked on 2026-10-24 for Neovim's treesitter text objects as vim defaults: `af` and `if` (a function, its body), `ac` and `ic` (a class or type, its body), as nvim-treesitter-textobjects maps them (also Zed's and IdeaVim's). They stay on in every preset. `ia`/`aa` (argument) already exist (`crates/fe-vim/src/feature/argument.rs`, Mac T892).

## Current state

- `text_object::select(text, offset, object, count, tab)` (`crates/fe-vim/src/text_object.rs:18`) sees only the text, so the way syntax reaches it is the thread-local view `line_motions::with_view` (`crates/fe-vim/src/line_motions.rs:165`) that `vim.move`, `vim.operate`, `.` and `vim.select` run in (`crates/fe-vim/src/commands.rs:116,133,366,566`). Feature-seam traps (memory fe-vim-feature-seam-gaps): a new object needs an arm in `visual/object.rs` (as `argument` has, :70) and a hook in `feature.rs`; `tests/feature_stubs.rs` lists only the T414 variants, so new ones don't break it.
- T1151 adds `EditorHost::definition_objects(offset)`; the default is empty, so without syntax `daf` cancels as an unknown object does.

## Change

- New `crates/fe-vim/src/feature/definition.rs` (declared in `feature.rs`, listed in `FEATURES`): the `object` hook maps `f` and `c` to new `TextObject::{InnerFunction, AFunction, InnerClass, AClass}` (args.rs, serde names `inner_function`...), dispatched from text_object.rs, with a visual arm in `visual/object.rs`.
- `with_view` (line_motions.rs) snapshots, when the command's text object is one of these, `host.definition_objects(offset)` for each caret's head and anchor into its `View`; `select` reads the chain from there (nothing is looked up for other commands).
- `af`: the nth enclosing function (count n) as the host's `outer`, charwise; `if`: its `inner`, linewise when the inner is whole lines inside braces (nvim-treesitter-textobjects) else charwise; `ac`/`ic` likewise for classes. A caret before any function in a class (a field) or on a bodyless item takes the nearest enclosing, else fails like any object. Visual mode: the object again grows to the next enclosing one.
- Out of scope: argument, loop, conditional, block and comment objects, `]f`/`[f` motions (separate cards), jump-to-next-function seeking (a later card).

## Tests

New `crates/fe-vim/tests/definition_objects.rs` with a custom `EditorHost` returning canned chains (as `tests/host.rs` does): `daf`, `dif`, `yac`, `cic`, `2daf` (outer fn), nested chain, count past the outermost fails, visual `vaf` then `af` grows, `.` repeats `daf` at another caret, multi-caret, no-syntax host cancels, `dif` on an empty body. No nvim oracle (the plugin has no headless oracle here): behaviour reference is nvim-treesitter-textobjects' `@function.outer/inner`.

## Implementation notes

- `line_motions::Viewed` is now an enum: a motion with its count, or a text object. `vim.operate`, `.` (through the last change's target) and `vim.select` pass their object, and `with_view` looks up `host.definition_objects` only for `af if ac ic`: once per distinct offset among each caret's head (as given and on its grapheme), its tail, and in a visual mode the vim cursor and anchor. `line_motions::definitions(offset)` reads them back; outside a `with_view`, or for an offset it didn't look at, it is empty and the object fails.
- `if`/`ic` skip an item without a body (a trait method's signature) and take the body of the item around it; `af`/`ac` still take that item. Equal ranges next to each other in the chain count once.
- `if`/`ic` are linewise when the body has only blanks before it on its first row and after it on its last (so whichever opener and closer bound it are on other rows); this covers a braced body and a Python block alike. Otherwise charwise.
- `af` over one row stays charwise under `d`, as any delete within a row does (`:help d`'s linewise exception needs the range to span rows), so `daf` on `    fn c() { x(); }` leaves the indent; over several whole rows it becomes linewise.
- Visual mode stays charwise (nvim-treesitter-textobjects' default `selection_modes`), including for a body that is whole rows; an empty body fails there.
- `ys{obj}` (`vim.surround_add`, and `.` after it) and `!{obj}` (`vim.filter`, and `.` after it) look the chain up too: `line_motions::object(target)` gives `with_view` the object, around `surround::targets` and in `filter::run` (review F2; `definition_objects_surround_and_filter`).
- Visual over one position this object didn't make takes the `count`th item's object as an operator does, so `vif` on a function's name or brace selects its body; only growing requires the object to hold the selection and be larger (review F1).
- Carried findings: T156 F4 is fixed (`fe_vim::dispatch` makes a command that does nothing after `<C-o>` return `Effect::Changed`, since insert mode resumes; `tests::commands::a_command_after_ctrl_o_that_does_nothing_still_repaints`). T197 F2 is fixed (`a_words` reads the row whole when its backward blank scan reaches a window's start that isn't the row's; `caret_window::a_word_takes_a_long_run_of_blanks_before_it`). T156 F1 (Ctrl-o in the `:normal` runner) is not: its fix lives in `insert/keys.rs` and `ex/normal.rs`, outside this card, and this card's change to `commands.rs` doesn't touch the runner.
