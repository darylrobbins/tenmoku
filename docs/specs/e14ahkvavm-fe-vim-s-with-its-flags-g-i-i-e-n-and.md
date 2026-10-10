---
task_uid: "01m4ewtr6p7n791fe14ahkvavm"
task: "T218"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::substitute::flags_specials_and_bounds: :s/a/b/ replaces the first a and :s/a/b/g every one on the row; i and I override case; n counts without changing; e suppresses E486; \u0026 keeps the previous flags; the specials \u0026 \\\u0026 \\0 \\1 ~ \\r \\t \\\\ \\u \\l \\U \\L \\E produce nvim's text; \\n in the replacement and the c flag are errors that change nothing; :%s over a 200,000-row buffer is one transaction undone by one u and visits each row once (cfg(test) counter); a g substitute on a row of 100,000 matches finishes within the test; . after :s repeats the change before it, not the :s; the pattern becomes the last search and the / register"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test substitute_matches_neovim (tests/substitute_oracle.rs): tests/corpus/substitute.toml has at least 30 cases covering :s with and without g i I \u0026 n e, an empty pattern using the last search, :s :\u0026 :\u0026\u0026 and normal-mode \u0026, each replacement special, \\r splitting a row, a non-/ delimiter, :%s over many rows, :'\u003c,'\u003es from visual mode, u after :%s and a substitute that finds nothing (last key); each matches its nvim 0.12.4 expectation in buffer, cursor, mode and registers (the / register included); every earlier expected file is byte-identical and substitute.expected.json is the recorder's unedited output"
    required: true
    manual: false
  - id: "C3"
    text: "Every earlier corpus and unit test passes with its files unchanged; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs list :s's flags and specials"
    required: true
    manual: false
---

# fe-vim: :s with its flags (g i I e n &) and replacement specials, :& :&& and normal-mode &

# fe-vim: :s with its flags and replacement specials, :& :&& and normal-mode &

Milestone M7 "Vim", second batch (resized), card 05c: the `:s` part of the old card 5. Read 04a
(patterns), 05a (ranges, `src/ex.rs`) and 05b. Design: `docs/decisions/vim-engine.md` §1, §9 item
12 (`:'<,'>s/…/…/g`), §11 (hjkl's substitute behaviour was never verified: our corpus is the only
check).

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help :s`, `:help s_flags`,
`:help sub-replace-special`, `:help &`; never source, hjkl-ex, Zed or Helix.

## Change (new `src/substitute.rs`, `src/ex.rs`, `src/search.rs`, `src/reducer.rs`)

- `:s[ubstitute]/pat/rep/[flags] [count]` over 05a's ranges, with any delimiter that isn't a
  letter, digit, `\`, `"` or `|`. Flags `g` `i` `I` `e` `n` `&`; `c` is rejected with a message (no
  confirm UI).
- `:s` with an empty pattern reuses the last search; `:s` alone and `:&` repeat the last
  substitute, `:&&` keeps its flags, `&` in normal mode repeats it on the cursor row.
- Replacement specials: `&`, `\&`, `\0`–`\9`, `~` (previous replacement), `\r` (line break), `\t`,
  `\\`, `\u \l \U \L \E \e`. `\n` (a NUL in Vim) is rejected with a message.
- The pattern becomes the last search and the `/` register; the cursor ends where nvim leaves it.
- Each `:s` is one transaction with the actor's origin, so one `u` undoes it; `.` doesn't repeat
  it (nvim's `.` doesn't repeat ex commands; `&` is its own).
- Bounded work: `:%s` over a 200,000-row buffer visits each row once and makes one edit batch; a `g` substitute on a row with 100,000 matches is linear in fe-vim's own work: the row is read once and the substitute makes one edit for the row. The matcher's own worst case, for patterns whose leftmost-first search scans past each match (like `a.*z\|a`, quadratic in nvim too), is out of scope here; T310 covers it. 04a's matcher may gain a captures-aware find for the specials.

### Corpus

`tests/corpus/substitute.toml`, at least 30 cases: `:s/a/b/` with and without `g`, `i`, `I`, `&`,
`n`, `e`, an empty pattern using the last search, `:s`, `:&`, `:&&` and normal-mode `&`, every
replacement special above, `\r` splitting a row, a delimiter other than `/`, `:%s` over many rows,
`:'<,'>s` from visual mode, `u` after `:%s`, and a substitute that finds nothing (E486, last key).

## Tests (new files)

- `src/tests/substitute.rs`, declared in `src/tests/mod.rs`.
- `tests/substitute_oracle.rs` (`#[allow(dead_code)] mod support;`) with `substitute_matches_neovim`.

## Size

About 400 non-test lines (`:s` parsing and flags ~120, replacement specials ~150, running and `&`
~130).

## Out of scope

`c` (confirm), `\=` expressions, `:g`/`:v`, `g&`, `:~`, history. Any change outside `crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- **`:s` without `c`.** Confirm needs UI; it is an error message.

## Implementation notes

- `:s` makes one edit per changed row, spanning its first match to its last with the text between
  copied, all in one transaction. One edit per match cost ~45µs each inside `fe-buffer` (debug), so
  a `g` over 100,000 matches took 4–70s depending on machine load; as one edit it takes well under a
  second. `:%s` over 200,000 rows is still one batch of one edit per row.
