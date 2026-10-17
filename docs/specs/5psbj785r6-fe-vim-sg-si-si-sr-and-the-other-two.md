---
task_uid: "01m54vrk40vp8mv65psbj785r6"
task: "T135"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test two_letter_substitute_forms_repeat_with_their_flags (tests/ex_substitute_short_forms.rs, top level) passes, as nvim --clean does: on \"a a a\\na a a\\nA a A\" the lines s/a/b/, 2, sg, 3, s/a/c/, si give \"b a a\\nb b b\\nc c A\"; s/a/b/g, 2, sI give \"b b b\\nb a a\\nA a A\"; s/a/b/, 2, \"sg 2\" give \"b a a\\nb b b\\nA b A\"; on \"a a a\\nx a x\\nA a A\" after s/a/b/g and /x\u003cCR\u003e, each of \u0026r, sr and \"s r\" gives row 2 \"b a x\" and each of ~g and \u0026\u0026r gives \"b a b\"; no line leaves an E message; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Vim's name rule holds: :sil, :sim, :sig, :scs, :scr and :sre are not taken as :s (they keep their current result), and :sc… keeps the \"c flag (confirm)\" refusal; :\u0026 and :\u0026\u0026 without r still use the last substitute's pattern; lib.rs no longer lists the two-letter forms or :~ as not planned"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: :sg, :si, :sI, :sr and the other two-letter :s forms, :~ and the r flag repeat the last substitute as nvim's do, instead of E492 and E488 (Mac T769)

## Problem

At Mac main 8f606ec8 fe-vim refuses `:s`'s two-letter forms and `:~`, which nvim takes as the last
substitute repeated with flags:

- `ex.rs` `command` reads the alphabetic name `sg` (`si`, `sI`, `sge`, …) and, since `"substitute".starts_with("sg")`
  is false, falls through to `lines::command`: `E492: Not an editor command: sg`. The crate docs (lib.rs, "Not
  planned for M7's first ex cards") list "the two-letter forms (`:sg`, `:si`, …)" and `:~`.
- `:~` is E492 too, and `substitute.rs` `flags_and_count` stops at `r`, so `:&r` and `:s r` are `E488: Trailing
  characters: r`.

nvim --clean --headless 0.12.6 (checked 2026-10-17):

- on `a a a / a a a / A a A`: `:s/a/b/`, `:2`, `:sg`, `:3`, `:s/a/c/`, `:si` gives `b a a / b b b / c c A`;
  `:s/a/b/g`, `:2`, `:sI` gives `b b b / b a a / A a A`; `:s/a/b/`, `:2`, `:sg 2` gives `b a a / b b b / A b A`.
- on `a a a / x a x / A a A` after `:s/a/b/g` and `/x<CR>` (cursor on row 2): `:&r` and `:sr` give row 2
  `b a x`; `:~g` and `:&&r` give `b a b`.

Vim's rule (ex_docmd.c `find_ex_command`): a name starting `s` is `:s` with the rest as flags when its second letter
is `g`, `I`, `c` (but not `scs`, `scr`, `sc?ip`), `i` (but not `sim`, `sil`, `sig`) or `r` (but not `sre`).

## Change

- `crates/fe-vim/src/ex.rs` `command`: before the `"substitute".starts_with(name)` test, a name matching Vim's rule
  above is `:s` with flags: `Substitute::repeat(&rest[1..], false)` after `check(true)`. `:sc…` keeps the existing
  "the c flag (confirm)" refusal. `:~` (name empty, `after` starting `~`) is `:&` with `r` set and the rest as
  flags (`Substitute::repeat`, `&` allowed first, as `:&&`), checked like `:&`.
- `crates/fe-vim/src/substitute.rs`: a `search_pattern: bool` in `Flags` for `r`, parsed in `flags_and_count`;
  `run`, for a repeat (`with: None`) with it set, takes the last search's pattern
  (`target.registers.last_search()`, else `ExError::NoPreviousSearch`) instead of the last substitute's, keeping
  the last replacement. `&` (keep) doesn't carry `r` over, as in nvim.
- `crates/fe-vim/src/lib.rs`: take `:sg`/`:si` and `:~` out of the "Not planned" list and name them where `:s`'s
  flags are listed (with `r`).

New integration test `crates/fe-vim/tests/ex_substitute_short_forms.rs`, top-level fn
`two_letter_substitute_forms_repeat_with_their_flags`, through `HeadlessVim` (feed each line as `:{line}<CR>`; no
`E` message after any of them), with the cases above.
