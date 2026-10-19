---
task_uid: "01m55mhz3nfh356ms8g8jxxn4e"
task: "T158"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test search_case_matches_neovim (top level, in crates/fe-vim/tests/search_case_oracle.rs) passes on crates/fe-vim/tests/corpus/search_case.toml, at least 18 cases recorded with nvim 0.12.x using the new set field, among them (asserted by name) ic_slash, ic_question, ic_n_and_N, scs_lowercase_ignores, scs_uppercase_exact, scs_backslash_class, lower_c_overrides, upper_c_overrides, star_ic, star_ignores_scs, hash_ic, n_after_star, default_exact, ic_operator_search, ic_visual_search."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test search_case_highlight_and_register (top level, same file) passes: with ignorecase and smartcase on, VimState::highlight() matches what the search matched, and the / register holds the pattern as typed; every existing oracle corpus still passes (the set field absent)."
    required: true
    manual: false
  - id: "C3"
    text: "support::check in crates/fe-vim/tests/support/mod.rs fails a case whose set field names an option it doesn't know, asserted by the integration test unknown_set_option_fails_the_check (top level, in crates/fe-vim/tests/search_case_oracle.rs)."
    required: true
    manual: false
---

# fe-vim: 'ignorecase' and 'smartcase' for / ? n N * and #, as nvim applies them, held on the shared registers (Mac T887)

## Problem

At Mac main 6f032b12 fe-vim's searches always match case exactly unless the pattern holds `\c`: there is no
'ignorecase' or 'smartcase' (`crates/fe-vim/src/lib.rs` "Search patterns": "'ignorecase' off"). Both are
standard in VSCodeVim (`vim.ignorecase`, `vim.smartcase`) and IdeaVim. The user approved them in the "Vim
parity" track on 2026-10-17, with smartcase applied to `*` and `#` as nvim applies it. nvim 0.12.6:
`:set ic scs` then `/Hello` on `x` / `Hello world` / `hello` from (0,0) lands on (1,0) (exact case, an
uppercase letter turns smartcase off). `:help 'smartcase'`: it is used for typed patterns (`/ ? n N :g :s`),
not for `*`, `#` or `gd`.

## Change

- The options live on `Registers` (`crates/fe-vim/src/register.rs`), which every editor shares as nvim's
  options are global: `Registers::set_search_case(ignore_case: bool, smart_case: bool)` and a getter.
  `HeadlessVim` reaches them through `registers_mut()` (add a convenience setter in
  `crates/fe-vim/src/headless.rs`).
- Each search motion resolves its case when it is matched, not when stored: `/`, `?`, `n`, `N` ignore case
  when 'ignorecase' is on and, with 'smartcase', the pattern has no uppercase letter (one after a backslash,
  as in `\S`, doesn't count, as nvim's doesn't: probe); `*` and `#` (and `n`/`N` after them, probe) use
  'ignorecase' only. `\c` and `\C` override both. The `/` register keeps the pattern as typed. The
  highlight (`VimState::highlight`, 'hlsearch') matches as the search does. Work in
  `crates/fe-vim/src/search.rs` (`search::parsed` and the pattern cache key take the case) and
  `crates/fe-vim/src/cmdline.rs` (where searches, `*` and `#` resolve).
- `:s`, `:g`, `:sort /pat/` and ex addresses are the next card's.
- Corpus support: a new optional corpus field `set` (nvim `:set` arguments, e.g.
  `set = "ignorecase smartcase"`), applied by `crates/fe-vim/tests/support/oracle.lua` with `vim.cmd("set
  " .. set)` before the case's keys, and by `support::check` (`crates/fe-vim/tests/support/mod.rs`) through
  the headless setters for the names it knows (`ignorecase`/`ic`, `smartcase`/`scs`, their `no` forms); an
  unknown name fails the check. Existing recordings don't change.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/search_case.toml`, recorded into
`search_case.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/search_case_oracle.rs` modelled on `tests/line_motions_oracle.rs`.

search.rs is changed by the lent T799 and T807 (T807 last), register.rs by T836, cmdline.rs by T801 and
headless.rs by T802: this card depends on T807, T836 and T802 (T802 follows T801).

## Implementation notes

- Probed with nvim 0.12.4 (the corpus's `NVIM`): `n`, `N` and `/<CR>` after `*` or `#` keep 'smartcase'
  off (nvim's `spats[].no_scs`), and a typed `/Foo` after `*` turns it back on. `search_word` on
  `Registers` carries that, and `Search` gains two never-serialized fields, `word` and `ignore_case`,
  which `cmdline::resolve` fills for every search motion as it runs, so `.` repeating `d/foo` and `dn`
  takes the options as they are then.
- `search::has_uppercase` is nvim's `pat_has_uppercase()`: an escaped letter doesn't count, nor `\_X` or
  `\%X`; after `\v` (the last `\v` or `\V` decides for the whole pattern, as in nvim) a backslash skips
  nothing, so `\v\Sello` matches case exactly (case `scs_very_magic_class_counts`), while `%X` and `_X`
  are skipped. Letters in a collection count (`[H]ello`).
- `\_S` isn't a pattern fe-vim translates yet, so the corpus has no case for it (the unit test in
  `search.rs` covers how `has_uppercase` reads it).
- Follow-up: the 'incsearch' preview (`VimState::search_preview`, which landed on main alongside this
  card) still matches case exactly: it reads no `Registers`, and giving it the options changes a public
  signature `fe-gpui` calls. Nothing in a host sets 'ignorecase' yet, so the two can't disagree today.
