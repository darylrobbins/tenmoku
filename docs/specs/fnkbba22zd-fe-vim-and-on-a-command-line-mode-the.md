---
task_uid: "01m4ewtn2dg5xca0fnkbba22zd"
task: "T214"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim test tests::cmdline::line_mode_and_routing: / commits vim.command_line {\"kind\":\"search_forward\"} and ? search_backward; after d2/ the commit carries operator delete and count 2; vim.mode is then cmdline with vim.cmdline_kind set and HeadlessVim no longer calls step(); vim.command_line_submit and vim.command_line_cancel are registered with context editor \u0026\u0026 vim.mode == cmdline; cancel returns to the mode the line came from (normal, or visual with the selection kept) with no operator applied; in HeadlessVim \u003cBS\u003e on an empty line cancels and \u003cEsc\u003e cancels"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::search::motion_register_and_visual: after /foo\u003cCR\u003e the cursor is on the next foo and the / register holds foo, \"/p puts foo and a yank into / is refused; an empty /\u003cCR\u003e reuses the last pattern; /zzz\u003cCR\u003e and /\\zs\u003cCR\u003e leave cursor, text and the other registers unchanged (/zzz\u003cCR\u003e sets the / register to zzz, as in nvim; /\\zs\u003cCR\u003e leaves it unchanged) and set VimState::message() (E486, and the unsupported atom), cleared by the next key; in visual mode /x\u003cCR\u003e moves the visual cursor to the match and keeps the anchor; d/x\u003cCR\u003e is one transaction and u restores the text"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test search_matches_neovim (tests/search_oracle.rs): tests/corpus/search.toml has at least 30 cases covering / and ? with counts, wrapping past either end, an empty pattern, the atoms \\\u003c \\\u003e \\{n,m} \\(a\\|b\\) \\v \\V \\c and a class, a failed search as the last key, d/ c/ and y?, a search in visual mode, a match at the cursor, multibyte text and \"/p; each matches its nvim 0.12.4 expectation in buffer, cursor, mode and registers (the / register included)"
    required: true
    manual: false
  - id: "C4"
    text: "The recorder records the / register; every earlier tests/corpus/*.expected.json is byte-identical and search.expected.json is the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C5"
    text: "Every earlier corpus and unit test passes with its files unchanged; nothing under crates/fe-vim/src/visual/ changes; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; src/lib.rs's crate docs describe the command-line mode and the host's part in it"
    required: true
    manual: false
---

# fe-vim: / and ? on a command-line mode the host types into, search as a motion and operator target (d/foo), and the / register

# fe-vim: / and ? on a command-line mode, search as a motion (d/foo), and the / register

Milestone M7 "Vim", second batch (resized), card 04b: the middle of the old card 4. Read 04a
(patterns, `Pattern::find`) and cards 1–2 (visual mode). Design: `docs/decisions/vim-engine.md`
§4 (Search: last pattern, direction), §9 item 2 (`vim.mode` `cmdline`) and item 4 ("Motion data
includes … the search pattern").

Assumes card 00 (the `visual.rs` split) has landed: `src/visual.rs` is the mode's root and holds
`move_cursor`; nothing here touches `src/visual/*.rs`.

**Clean room** as every M7 card: `nvim --clean` (0.12.4) and `:help /`, `:help search-commands`,
`:help exclusive`; never source, hjkl, Zed or Helix.

## Change

### The command line as a mode (new `src/cmdline.rs`, `src/state.rs`, `src/reducer.rs`)

- `/` and `?` commit `vim.command_line {"kind": "search_forward" | "search_backward"}`, carrying
  what was pending: count, register and operator (`d/`, `2/`, `y?`). `VimState` goes to
  `Mode::CmdLine` (`vim.mode` is `cmdline`, plus `vim.cmdline_kind`), remembering the mode it came
  from (normal, operator-pending or a visual mode).
- The host owns the line's text. It ends the line with `vim.command_line_submit {"text": "…"}` or
  `vim.command_line_cancel`, both registered with context `editor && vim.mode == cmdline`. Cancel
  returns to the mode the line came from (a visual selection kept), dropping a pending operator.
- `HeadlessVim` stands in for the host's input in `cmdline` mode, as it does for insert mode: a char
  appends, `<BS>` deletes the last char (and cancels on an empty line), `<Esc>` cancels, `<CR>`
  submits. `step()` isn't called in `cmdline` mode.
- The ex card (05a) adds the `:` kind on the same commands.

### Search as a motion (`src/motion.rs`, `src/operator.rs`, `src/commands.rs`, `src/visual.rs`)

- Submitting a search runs `vim.move` with motion `{"search": {"pattern", "forward"}}` (or
  `vim.operate` with that target when an operator was pending), with the carried count. An empty
  line reuses the last pattern. The pattern becomes the last search, with its direction (04c's
  `n`/`N` read it).
- A search is exclusive charwise; T155's exclusive rules (`:help exclusive`, `exclusive-linewise`)
  apply to `d/foo`, `c/foo`, `y?x`.
- In a visual mode the search moves the visual cursor and keeps the anchor (through
  `visual::move_cursor` in the root `visual.rs`).
- A search that finds nothing is E486 and changes nothing (cursor, text, the other registers, the
  pending operator), except that the `/` register becomes the pattern, as in nvim. An unsupported or
  invalid pattern (04a's error) changes nothing, the `/` register included.
- `VimState::message() -> Option<&str>` holds the last error for the host to show, cleared by the
  next key.

### The `/` register (`src/register.rs`)

`Registers` gains the read-only `/` register: `"/p` puts the last pattern; a yank into it is
refused as nvim does.

### Oracle (`tests/support/mod.rs`, `tests/support/oracle.lua`)

- The recorded registers gain `/` (oracle.lua already clears it). No earlier case searches, so
  every earlier expected file stays byte-identical.
- Cases end at a failed search: nvim flushes queued keys after an error.

### Corpus

`tests/corpus/search.toml`, at least 30 cases: `/` and `?` with and without counts, wrapping past
either end, an empty pattern reusing the last, the magic atoms `\<\>`, `\{n,m}`, `\(a\|b\)`, `\v`,
`\V`, `\c` and a class, a failed search (the last key), `d/`, `c/` with typed text, `y?`, a search
inside visual mode, a match at the cursor (not found again), multibyte text, and `"/p`.

## Tests (new files)

- `src/tests/cmdline.rs` and `src/tests/search.rs`, declared in `src/tests/mod.rs`.
- `tests/search_oracle.rs` (`#[allow(dead_code)] mod support;`) with `search_matches_neovim`.

## Size

About 400 non-test lines (command line ~200, search motion and operator target ~150, register and
state ~50).

## Out of scope

- `n N * #`, the highlight state (04c); the GPUI line (06a); search offsets, `gn`, history,
  'incsearch'; marks set by searches (08a).
- Any change outside `crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- **The host owns the command line's text**; fe-vim owns its mode and what submitting means, so the
  GPUI line, `HeadlessVim` and macros (08b) share one path.
