---
task_uid: "01m4ewtnzh0t7zx4jdxdvj3vth"
task: "T215"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test tests::search_next::next_star_and_highlight: after /foo\u003cCR\u003e, n and 2n move to the next and second-next foo and N back; after ?foo\u003cCR\u003e, n goes backward; n with no last search is E35 and moves nothing; * on foo in \"foo foobar foo\" moves to the third word (whole-word) and sets the / register to \\\u003cfoo\\\u003e; # goes backward; * with no keyword left on the row is E348; VimState::highlight() is the last pattern after /, n, * and #; dn and d* are one transaction each and u restores the text; n in visual mode moves the visual cursor and keeps the anchor"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test search_next_matches_neovim (tests/search_next_oracle.rs): tests/corpus/search_next.toml has at least 20 cases covering n and N after / and after ? with counts and wrapping, * and # on a word, on punctuation, between words and with no keyword left on the row, dn, d*, n in visual mode, and spot_row_22 (/foo\u003cCR\u003en) and spot_row_23 (*) on docs/decisions/vim-engine.md section 6's buffer from (0,4); each matches its nvim 0.12.4 expectation in buffer, cursor, mode and registers; every earlier expected file is byte-identical and search_next.expected.json is the recorder's unedited output"
    required: true
    manual: false
  - id: "C3"
    text: "Every earlier corpus and unit test passes with its files unchanged; nothing under crates/fe-vim/src/visual/ changes; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim"
    required: true
    manual: false
---

# fe-vim: n, N, * and # as motions over the last search, and the search highlight state for hosts

# fe-vim: n, N, * and #, and the search highlight state

Milestone M7 "Vim", second batch (resized), card 04c: the last third of the old card 4. Read 04a
and 04b. Design: `docs/decisions/vim-engine.md` §4 (Search) and §6 (spot-check rows 22
`/foo<CR>n` and 23 `*`, left out of T156).

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help n`, `:help star`,
`:help 'hlsearch'`; never source, hjkl, Zed or Helix.

## Change (`src/motion.rs`, `src/search.rs`, `src/state.rs`, `src/reducer.rs`, `src/commands.rs`)

- `n` and `N` (with counts) commit `search_next` / `search_previous` motions over 04b's last search
  and its direction. With no last search, nvim's E35, nothing moves.
- `*` and `#` search forward and backward for the keyword under or after the cursor as `\<word\>`
  (`:help star`; keyword chars from `src/chars.txt`'s classes), and set the last search and the `/`
  register. With no keyword on the rest of the row, E348, nothing moves.
- All four are exclusive charwise motions after an operator (`dn`, `d*`), and in a visual mode move
  the visual cursor and keep the anchor (as 04b's search does, through `visual::move_cursor`).
- `VimState::highlight() -> Option<&Pattern>`: the last search's pattern while 'hlsearch' is on
  (nvim's default); set by every search, `n`, `N`, `*` and `#`. 05a's `:noh` clears it until the
  next search.
- Errors (E35, E348, E486) go to 04b's `VimState::message()`.
- Searches don't set the `''` jump mark yet; 08a adds it.

## Corpus

`tests/corpus/search_next.toml`, at least 20 cases: `n` and `N` after `/` and after `?` (with
counts, wrapping), `*` and `#` on a word, on punctuation, between words and with no keyword left on
the row (last key), `dn`, `d*`, `n` in visual mode, and §6's rows 22 and 23 on §6's buffer from
(0, 4), named `spot_row_22` and `spot_row_23`.

## Tests (new files)

- `src/tests/search_next.rs`, declared in `src/tests/mod.rs`.
- `tests/search_next_oracle.rs` (`#[allow(dead_code)] mod support;`) with `search_next_matches_neovim`.

## Size

About 200 non-test lines.

## Out of scope

`g*`, `g#`, `gn`, search offsets, the GPUI highlight (06b). Any change outside `crates/fe-vim`.

## Implementation notes

- `n`/`N`/`*`/`#` resolve to `Search`/`SearchWord` at run time and the recorded change keeps the original motion, so `.` searches again. `n`/`N` keep the last search's direction.
- `*` and `#` take a run of one character class, as nvim does. A one-branch pattern that begins with `\<` and ends with `\>` checks them by character class on each match, as Vim does, not with `regex`'s word boundaries.
- nvim's own mappings make `*`/`#` search the selection in visual mode, so those aren't in the corpus.
