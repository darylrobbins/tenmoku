---
task_uid: "01m56ex6b7v7bj9c3qh6ghpx1r"
task: "T192"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The integration test set_reads_numbers_in_nvims_bases (top level, in crates/fe-vim/tests/ex_set.rs) passes: through HeadlessVim, ts=010 / ts:010 / sw=0010 are 8, ts=017 is 15, ts=0x8 / 0X8 / 0x08 are 8, ts=0xa / 0xA are 10, ts=0o10 / 0O10 are 8, ts=0b101 is 5 and ts=0B11 is 3, ts=08 / 09 are decimal (8, 9) and sw=018 / sw=0019 are decimal (indent_style() Spaces(18), Spaces(19)), while ts=018 still gives the existing over-16 tabstop refusal (MAX_TABSTOP stays 16), and :set ts=010 ts? reports exactly '  tabstop=8'; sw=00 / 0x0 / 0b0 / 0o0 / -0 / -00 set indent_style() None; ts=00, ts=0x0, ts=-0, ts=-010, ts=-0x8, ts=-08 and sw=-0x1 give exactly 'E487: Argument must be positive: {arg}'; ts=0x, 0X, 0xg, 0x-8, 0b, 0b2, 0b12, 0o, 0o8, 0o18, 00x8, 010x, 8x, 0_8, +8 and 1e1 give exactly 'E521: Number required after =: {arg}'; each refusal sets state().failed() and leaves tab_size() and indent_style() unchanged; starting from a shiftwidth other than 2, :set sw=2 ts=0x sets shiftwidth 2, leaves tab_size() unchanged and gives E521 with failed() true"
    required: true
    manual: false
  - id: "C2"
    text: "The integration test ex_set_matches_neovim (top level, in crates/fe-vim/tests/ex_set_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_set.toml re-recorded with nvim 0.12.4 into ex_set.expected.json, which holds (by name) shift_after_sw_octal, shift_after_sw_hex, shift_after_sw_hex_upper, shift_after_sw_binary, shift_after_sw_0o, shift_after_sw_09_decimal, shift_after_sw_0010, shift_after_shiftwidth_colon_octal, shift_after_ts_octal_sw0 and shift_after_ts_hex_sw0 alongside T825's cases"
    required: true
    manual: false
---

# fe-vim: :set tabstop and shiftwidth read nvim's 0x, 0b, 0o and leading-0 octal numbers, not only decimal (Mac T825 review F2) (Mac T978)

Filed on argon from Mac T978 (card ~/coordination/argon-cards/zzv/T978.json rev 1; note zzv). Mac T825 = argon T147.

## Problem

Mac T825 (argon T147, landed on argon as fa9ec63, 2026-10-18; reaches Mac main with argon's batch) parses
a `:set` number in decimal only. Argon's codex review of T147 (review f5c8f3, F2 medium,
`crates/fe-vim/src/ex/set.rs:231` and `:241`, `set_number`): "Neovim accepts octal and hexadecimal option
values: sw=010 sets shiftwidth to 8, but this implementation silently sets 10; ts=0x8 incorrectly fails
with E521. This affects both tabstop and shiftwidth, including their aliases and colon assignments."
`set_number` refuses any byte that isn't an ASCII digit, then calls `value.parse::<usize>()`.

`:help :set-args` (nvim 0.12): "For numeric options the value can be given in decimal, hex (preceded
with 0x) or octal (preceded with '0' or '0o')." Probed with `nvim --headless -u NONE` 0.12.6 (each from
ts=4 sw=4; nvim parses with `vim_str2nr(..., STR2NR_ALL)`, so `'nrformats'` plays no part: with it empty,
`ts=010` is still 8):

- Accepted: `ts=010` 8, `ts=017` 15, `sw=0010` 8, `ts=0x8` / `ts=0X8` / `ts=0x08` 8, `ts=0xa` / `ts=0xA`
  10, `ts=0o10` / `ts=0O10` 8, `ts=0b101` 5, `ts=0B11` 3 (binary, undocumented but accepted), `ts:010` 8,
  `ts=0x8 sw=010` both 8; `:set ts=010 ts?` reports `  tabstop=8`.
- A leading 0 followed by an 8 or 9 is decimal: `ts=08` 8, `ts=09` 9, `ts=018` 18, `ts=0019` 19.
- Zero: `ts=00`, `ts=0x0` are `E487: Argument must be positive: ts=00` (as `ts=0`); `sw=00`, `sw=0x0`,
  `sw=0b0`, `sw=0o0`, `sw=-0`, `sw=-00` set shiftwidth 0. `ts=-0` is E487.
- Negative: a leading `-` before any form, `ts=-010`, `ts=-0x8`, `ts=-08`, `sw=-0x1`, is
  `E487: Argument must be positive: {arg}`.
- `E521: Number required after =: {arg}` for a prefix with no digit of its base or anything after the
  number: `ts=0x`, `ts=0X`, `ts=0xg`, `ts=0x-8`, `ts=0b`, `ts=0b2`, `ts=0b12`, `ts=0o`, `ts=0o8`,
  `ts=0o18`, `ts=00x8`, `ts=010x`, `ts=8x`, `ts=0_8`, `ts=+8`, `ts=1e1`. Nothing is set.

## Change

In `crates/fe-vim/src/ex/set.rs`, replace `set_number`'s digit check and decimal `parse` with one parse of
nvim's number forms, then the existing checks unchanged:

- An optional `-`, then: `0x`/`0X` and one or more hex digits; `0b`/`0B` and one or more binary digits;
  `0o`/`0O` and one or more octal digits; a `0` followed by digits all 0 to 7 (octal); otherwise decimal
  digits (so `08`, `09`, `018` are decimal). Anything else, or anything left after the digits, is E521 as
  today's message (`E521: Number required after =: {arg}`, `{arg}` as typed, `=` or `:`).
- A negative value is E487 as today; `-0` (any form) is 0, so `sw=-0` sets shiftwidth 0 and `ts=-0` is
  E487 (tabstop's zero check). A value too large for `usize` keeps today's refusal (the overflow path),
  as do fastedit's bounds (tabstop 1 to 16, shiftwidth up to 255) and the noexpandtab refusals: only the
  value read changes.
- Update the module doc comment to name the forms.

Tests, no new test file (argon's regression check finds 0 tests at base for a new top-level test file;
both files below exist at base after T825):

- In `crates/fe-vim/tests/ex_set.rs`, a new top-level `set_reads_numbers_in_nvims_bases` through
  `HeadlessVim`, exact messages: every accepted form above with its value (`tab_size()`, `indent_style()`,
  and `:set ts?` / `:set sw?` messages, e.g. `  tabstop=8` after `:set ts=010`), the decimal leading-0
  cases, the zero cases (`sw=-0` gives `indent_style() == None`), and each E487 and E521 case with its
  exact message and the editor unchanged (`vim.state().failed()`); plus `:set sw=2 ts=0x` leaving sw 2
  with E521.
- Oracle: add cases to `crates/fe-vim/tests/corpus/ex_set.toml`, each a `:set` then `>>` (or `>>j>>`):
  `shift_after_sw_octal` (`sw=010`), `shift_after_sw_hex` (`sw=0x6`), `shift_after_sw_hex_upper`
  (`sw=0X6`), `shift_after_sw_binary` (`sw=0b11`), `shift_after_sw_0o` (`sw=0o5`),
  `shift_after_sw_09_decimal` (`sw=09`), `shift_after_sw_0010` (`sw=0010`),
  `shift_after_shiftwidth_colon_octal` (`shiftwidth:010`), `shift_after_ts_octal_sw0` (`ts=010 sw=0`)
  and `shift_after_ts_hex_sw0` (`ts=0x8 sw=0`; shiftwidth following tabstop 8). Re-record `ex_set.expected.json` with
  the ignored `record_corpus_with_neovim` in `tests/oracle.rs` (it needs nvim 0.12.4, as argon has).
  Refusals stay out of the corpus (an error ends a case and the oracle doesn't compare option values).

Touch nothing outside set.rs and these test files.
