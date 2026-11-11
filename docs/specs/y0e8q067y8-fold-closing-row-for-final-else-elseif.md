---
task_uid: "01m75dnb32qz5fwhy0e8q067y8"
task: "T324"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --test fold_branch_closers passes: in Lua, hovering a final else body's fold and an elseif body's fold gives the enclosing if's end row as the closing row; an if body whose end is its own sibling and a Python if/else (no closer) keep their current closing rows"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --features php --test fold_branch_closers passes: PHP alternative syntax if (a): ... elseif (b): ... else: ... endif; gives the endif row as the closing row of the elseif and else bodies' folds"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax and cargo nextest run -p fe-gpui --test gutter_b3 pass; fold ranges (fold_candidate_closing's range) are unchanged for every existing test"
    required: true
    manual: false
---

# Fold closing row for final else/elseif bodies in Lua and PHP alternative syntax: resolve the closer through enclosing branch wrappers (T1549 review F1) (Mac T1724)

Review follow-up of T1549 (editor gutter B3), its review F1 (medium, open after 3 rounds; T1549's C1 was narrowed to leave this case out).

`fe_syntax::fold_candidate_closing` (crates/fe-syntax/src/fold.rs) gives a fold's closing row, where the gutter draws the up-chevron while that fold is hovered. Since T1549, a `Body` whose closer belongs to its parent (Lua `end`, Swift `}`, PHP `endif`) takes the parent's closer through `closed_after`, which only looks at the body's immediate siblings. A final `else`/`elseif` body sits inside a branch node (Lua `else_statement`/`elseif_statement`, PHP alternative-syntax `else_clause`/`else_if_clause` with a `colon_block`), while `end`/`endif` is a child of the enclosing `if`. So for

    if a then
      x()
    else
      y()
    end

hovering the fold at row 2 (`else` body) puts the up-chevron on row 3 instead of row 4 (`end`). PHP's `if (a): ... else: ... endif;` has the same problem.

Fix: when a body's own siblings hold no closer, resolve through enclosing branch wrappers (walk up through else/elseif branch nodes to the statement that owns the closer) and take that closer when it starts a row past the body. Keep fold ranges unchanged, and don't borrow an unrelated enclosing delimiter for delimiter-free bodies (Python, YAML; an `if` with no wrapper between body and closer keeps today's behaviour).

Tests go in a new integration-test file crates/fe-syntax/tests/fold_branch_closers.rs (public API only: build a Syntax for a `.lua`/`.php` path the way crates/fe-syntax/src/fold/lookup_tests.rs's `parsed` helper does, then call fold_candidate_closing). PHP cases are `#[cfg(feature = "php")]` and run with `--features php`.
