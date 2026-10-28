---
task_uid: "01m5qphzj35avnff3nr9rs99t9"
task: "T232"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test function_motions_move_between_functions (top level, crates/fe-vim/tests/definition_motions.rs): with a canned host, ]m [m ]M [M land on the next or previous function start or last char, counted, nested functions counting, from inside a function and between functions"
    required: true
    manual: false
  - id: "C2"
    text: "function_motions_fail_at_the_ends: ]m after the last function and [m before the first fail, set no jump mark and stop a macro; a host without syntax fails the same way"
    required: true
    manual: false
  - id: "C3"
    text: "function_motions_with_operators_and_visual: d]m y]M c[m and visual ]m act over the ranges nvim's exclusive/inclusive rules give, and . repeats d]m"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test function_bounds_tests::real_rust_view_lists_functions: the host's function_bounds for a Rust text lists fns and methods in order, and fe-syntax's function_bounds is bounded by MAX_OUTLINE_ITEMS"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim -p fe-syntax -p fe-gpui and clippy pass"
    required: true
    manual: false
---

# fe-vim: ]m [m ]M [M go to the next or previous function start or end from the host's syntax, with counts and operators (Mac T1160)

## Problem

The user asked on 2026-10-24 for `]m [m` (and the `]]` family) among the unimpaired-style pairs. `]m`/`[m` go to the next/previous function start and `]M`/`[M` to the next/previous function end, as nvim-treesitter-textobjects' conventional `@function.outer` motions (LazyVim ships `]f [f ]F [F` for the same; `]m` is the Vim method motion). Syntax comes from the host, so it works in every language with an outline query. Plain `]]` sections are T1158.

## Current state

- fe-vim motions see only the text (memory fe-vim-feature-seam-gaps: `motion()` hooks get no host or model); the escape hatch is `line_motions::with_view(target, viewed, ..)` (`crates/fe-vim/src/line_motions.rs:165`), which looks up host data only for the `Viewed` motion's command (it does so for `gj`/`gk` hops). Callers at `crates/fe-vim/src/commands.rs:116,133,366`.
- T1151 adds `fe_syntax::definition_objects(syntax, offset)` and `EditorHost::definition_objects`; this needs the list of functions, not the chain at one offset.
- A motion's other seams: `args.rs` (:132, :375, :430), `motion.rs` (:310), `marks.rs` `lands` (:2251), `visual.rs` (:754); `unmatched.rs` is the model.

## Change

- fe-syntax `textobject.rs` (T1151's file): `pub fn function_bounds(syntax: &Syntax) -> Vec<Range<usize>>`, every Function/Method item's node range in start order (the outline's cap, `MAX_OUTLINE_ITEMS`, applies), and the `EditorHost::function_bounds(&self) -> Vec<Range<usize>>` (fe-editor commands.rs, default empty) with the fe-gpui impl.
- New `crates/fe-vim/src/feature/definition_motions.rs`: after `[`/`]`, `m` and `M` give `Motion::Function { forward, end }`: the start (`m`) or last char (`M`) of the count-th function whose start/end is after (`]`) or before (`[`) the caret; nested functions count; charwise exclusive for `m` and inclusive for `M` as nvim's `]M`; a jump. `with_view` fills the list only when the command's motion is this one. Without syntax the motion fails and sets no jump mark.
- Not here: class motions (`]c` is the existing change motion), `]f` aliases, argument motions.

## Tests

New `crates/fe-vim/tests/definition_motions.rs` with a custom host listing canned function ranges: `]m`, `[m`, `]M`, `[M`, counts, nested functions, from inside a function, from the first/last function (fails), `d]m`, `y]M`, visual, `.`, jump mark on success and not on failure, no-syntax host. A fe-gpui test (a new `crates/fe-gpui/src/function_bounds_tests.rs`, declared by one `#[cfg(test)] #[path]` line in commands.rs) proves a Rust view's real list.

## Implementation notes

- `fe_syntax::function_bounds` filters the whole-file `outline()` to Function/Method items, so it shares the outline's `MAX_OUTLINE_ITEMS` cap and its pre-order (by start, an outer function before those nested in it). A trait method's signature is a Method item, so it's listed; a closure isn't.
- A count past the last function goes as far as there are, as nvim-treesitter-textobjects' repeated steps and Vim's `]m` do; only when there's none at all does the motion fail (no jump mark, macro stops).
- `]M` and `[M` land on the node's last char, skipping a line break that ends a node. Being inclusive, `d[M` also takes the caret's char, as an `o_v`-forced inclusive motion does.
- `line_motions::object` now also returns the view for a `]m`-family target, so `ys]M` (surround) sees the host's functions too.
