---
task_uid: "01m4sxmbepr8hx01x650pkxs18"
task: "T108"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "find_replace: Replacement::parse expands $0, $\u0026, $1..$99 (two digits at most, so $1a is group 1 then a), ${name}, $$, \\n, \\t and \\\\ with regex on, and is literal with regex off"
    required: true
    manual: false
  - id: "C2"
    text: "find_replace: replace_all returns every match matches_in finds (whole-word and case toggles applied, empty matches once) with its expansion, and Pattern::replacement_at is None for a range that is not exactly a match"
    required: true
    manual: false
  - id: "C3"
    text: "find_replace: with a time limit of 1 ms on a pathological pattern, replace_all returns Err(Interrupted), never a partial list"
    required: true
    manual: false
  - id: "C4"
    text: "find_replace: for literal \"foo\" with match case on, applying replace_all gives the same text as :%s/foo/bar/g; for regex (\\w+)-(\\w+) with $2-$1 the same text as :%s/\\(\\w\\+\\)-\\(\\w\\+\\)/\\2-\\1/g (headless :s)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim and cargo clippy -p fe-vim --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Expand find-bar replacements in fe-vim: capture groups, literal mode and replace-all edits for a find pattern (Mac T504)

## Problem

The in-file find bar only finds: `crates/fe-workbench/src/find_bar.rs` builds a `fe_vim::search::Pattern` from a
`FindQuery` (`Pattern::from_query`, crates/fe-vim/src/search.rs:233) and steps through matches; nothing can
replace them. The 2026-10-12 day-one feature audit lists find-and-replace as table stakes (user: "card the day-one
ones"). The find bar's pattern exposes ranges only, so there is no way to expand capture groups into a replacement.

## Change

A pure replacement engine for find-bar patterns, in a new module `crates/fe-vim/src/search/replace.rs`, declared
`pub mod replace;` from `crates/fe-vim/src/search.rs` (a child module, so it reads `Pattern`'s private regex and
slots; add only what it needs to search.rs). No GPUI, no host.

- `Replacement::parse(template: &str, regex: bool) -> Replacement`. With `regex` false the template is literal.
  With `regex` true (VS Code's rules): `$0` and `$&` are the whole match, `$1`..`$99` a group (at most two digits,
  so `$1a` is group 1 then `a`; a group the regex lacks expands to nothing), `${name}` a named group, `$$` a `$`,
  `\n` a line break, `\t` a tab, `\\` a backslash; anything else is literal.
- `Pattern::replacement_at(&self, text: &Text, range: Range<usize>, with: &Replacement) -> Option<String>`: the
  expansion if `range` is exactly a match the pattern finds (as `matches_in` finds it on that row, whole-word
  filter included), else `None`. The find bar's Replace uses it on the selected match.
- `replace_all(text: &Text, pattern: &Pattern, with: &Replacement) -> Result<Vec<(Range<usize>, String)>,
  Interrupted>`: every match in the text, in order, non-overlapping, row by row exactly as `matches_in` finds them
  (empty matches included, once each), with its expansion. Under one `Deadline` of `time_limit()`: running out is
  `Err(Interrupted)`, never a partial list.
- Semantics vs `:s` (state in the module docs): the find bar's pattern is the `regex` crate's syntax with case from
  its Match Case toggle (off = case-insensitive whatever the query holds, no smartcase), and `$n` groups; `:s`
  keeps Vim's syntax (`\(\)`, `\1`, `&`, `\c`, 'ignorecase' off by default). Both match within one row, and for
  equivalent patterns they produce the same text.

## Tests

New file `crates/fe-vim/tests/find_replace.rs`, using fe-vim's headless host (crates/fe-vim/src/headless.rs) for
the `:s` comparisons.
