---
task_uid: "01m54vs0ne2e1sbjxb9xfnfmd2"
task: "T143"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test g_ampersand_substitutes_every_line_as_nvim_does (tests/substitute_all_lines_g_amp.rs, top level) passes, as nvim --clean does from (0,0): \"a a a\\na a a\\nA a A\" with :s/a/b/g\u003cCR\u003eg\u0026 gives \"b b b\\nb b b\\nA b A\"; \"a a a\\nx a a\\nA a A\" with :s/a/b/\u003cCR\u003e/x\u003cCR\u003eg\u0026 gives \"b a a\\nb a a\\nA a A\"; \"a a a\\na a a\\nA a A\" with :s/a/b/i\u003cCR\u003eg\u0026 gives \"b b a\\nb a a\\nb a A\"; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "g\u0026 is one undoable transaction (u after it restores the text before it), . does not repeat it, an operator before it (dg\u0026) cancels, and with no previous search it reports E35 as :s does"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: normal-mode g& runs the last substitute on every line with the last search pattern and flags, as nvim's :%s//~/& does, instead of doing nothing (Mac T770)

## Problem

At Mac main 8f606ec8 normal mode's `g&` does nothing in fe-vim: `reducer.rs` `Pending::complete` has no `g&` arm, so
`Prefix::G` with `&` goes to `unmatched` → `feature::g`, which no feature takes, and the key cancels silently. nvim's
`g&` is `:%s//~/&`: the last substitute run again on every line, with the last *search* pattern, the last
replacement and the last substitute's flags (`:help g&`).

nvim --clean --headless 0.12.6 (checked 2026-10-17), cursor at (0,0):

- `a a a / a a a / A a A`, `:s/a/b/g<CR>g&` gives `b b b / b b b / A b A` (fe-vim: `b b b / a a a / A a A`).
- `a a a / x a a / A a A`, `:s/a/b/<CR>/x<CR>g&` gives `b a a / b a a / A a A` (the search's `x`, not `a`).
- `a a a / a a a / A a A`, `:s/a/b/i<CR>g&` gives `b b a / b a a / b a A` (`i` kept).

The crate docs (lib.rs) list `g&` as not planned; that line is left to the `:sg` card in this batch, which owns
lib.rs.

## Change

- `crates/fe-vim/src/reducer.rs` `Pending::complete`: `Prefix::G if ch == '&'` with no operator and not visual
  commits a new `vim.substitute_all_lines` invocation with no args (a count is dropped, as nvim's `g&` ignores it);
  after an operator it cancels.
- `crates/fe-vim/src/commands.rs`: register `vim.substitute_all_lines` ("Vim Substitute All Lines", normal
  context, mutates the buffer), beside `vim.repeat_substitute`: `follow(target)`, then
  `crate::ex::run(target, actor, "%s//~/&")`, then `marks::settle(target)`. Its description says it is `g&`, one
  transaction, which `.` doesn't repeat. Errors are `:s`'s own (E35 with no previous search, E486 when nothing
  matches).

New integration test `crates/fe-vim/tests/substitute_all_lines_g_amp.rs`, top-level fn
`g_ampersand_substitutes_every_line_as_nvim_does`, through `HeadlessVim`, with the three cases above.
