---
task_uid: "01m55maxc3pjy96pfrn16mvj8k"
task: "T188"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "vimrc_reads_map_commands (top level, crates/fe-vim/tests/remap_vimrc.rs): nmap nnoremap xmap xnoremap omap onoremap map noremap vmap vnoremap and their abbreviations give the modes and recursive flags in the spec, with a leading : allowed, \u003csilent\u003e/\u003cnowait\u003e ignored and \" comments skipped"
    required: true
    manual: false
  - id: "C2"
    text: "vimrc_unmap_and_leader: unmap, {n,x,v,o}unmap, mapclear and {mode}mapclear remove earlier entries; let mapleader applies to \u003cleader\u003e in the lines after it only; | ends a command unless written \\| or \u003cBar\u003e"
    required: true
    manual: false
  - id: "C3"
    text: "vimrc_reports_problems: imap/cmap/map! (insert and command-line not yet), smap, \u003cbuffer\u003e/\u003cexpr\u003e/\u003cscript\u003e, a listing form, set and any other ex command, and an entry RemapSet::new rejects each give one VimrcIssue at its 1-based line, and every other line still loads"
    required: true
    manual: false
---

# fe-vim: parse a vimrc subset (map, noremap and unmap families, let mapleader, comments) into remap records with line-numbered problems (Mac T871)

## Problem

The user wants both TOML remaps and a vimrc subset (`docs/decisions/vim-remaps.md` §1(c),
decided 2026-10-17), so nvim users can paste their map lines. This is the record's card 6,
parser half: text in, `Remap` records and line-numbered problems out. Loading and watching
the file is a later host card. The corpus card also uses this parser on both sides of the
oracle, so a mis-parse shows up as a difference from nvim.

## Change

New `crates/fe-vim/src/vimrc.rs`, declared in `lib.rs` (`mod vimrc;`,
`pub use vimrc::{Vimrc, VimrcIssue, parse_vimrc}`). It is pure: no I/O, no `VimState`.

`parse_vimrc(text: &str) -> Vimrc`, where `Vimrc { remaps: Vec<Remap>, issues:
Vec<VimrcIssue> }` and `VimrcIssue { line: usize /* 1-based */, message: String }`. Each
line is read as nvim reads it:

- Blank lines, and lines whose first non-blank is `"`, are skipped. A leading `:` is
  allowed.
- **Map commands**, with nvim's abbreviations (`nn[oremap]`, `xn[oremap]`, `no[remap]`, …):
  - `nmap`/`nnoremap` give mode n; `xmap`/`xnoremap` give x; `omap`/`onoremap` give o.
  - `map`/`noremap` give n, x and o. They load without a note: Select mode isn't in fastedit.
  - `vmap`/`vnoremap` give x, with an issue noting that Select mode isn't there (§2).
  - `*map` is `recursive: true` and `*noremap` is `false`.
- **Not yet supported** (one issue each, the line skipped): `imap`, `cmap`, `lmap`, `tmap`,
  `smap`, `map!` and their `noremap` forms. The message for `i` and `c` is "insert and
  command-line remaps aren't supported yet".
- **Unmap.** `unmap`, `nunmap`, `xunmap`, `vunmap` and `ounmap` remove the entries with that
  `from` for those modes, from those parsed above. `{mode}mapclear` and `mapclear` remove all
  of them for those modes.
- **Arguments.** Leading `<silent>` and `<nowait>` are accepted and ignored. `<buffer>`,
  `<expr>`, `<script>`, `<unique>` and `<special>` give an issue and skip the line.
- **Splitting the line.** `from` is the first blank-separated word. `to` is the rest, with
  trailing blanks kept as nvim keeps them. An unescaped `|` ends the command (`:help
  map-bar`): `\|` and `<Bar>` don't. A `"` after the `to` is part of it, as in nvim.
- **Leader.** `let mapleader = "…"` (or `'…'`) sets the leader for the lines after it. Each
  `<leader>` is replaced when its line is read, as nvim does. The default is `\`.
- **Listing.** A map command with no `to` (`nmap`, `nmap x`) lists maps in nvim. Here it
  is an issue: "listing maps isn't supported".
- **Other lines.** Any other ex command (`set`, `let` of anything else, `syntax`, `autocmd`,
  `source`, a function) gives one issue, "not supported: {command}", and is never run. A line
  continuation (`\` at the start of the next line) is an issue too.
- **Checking.** Each record is then checked with `RemapSet::new`. Its `RemapIssue`s come
  back as `VimrcIssue`s at the line that made the entry.

Tests go in the new top-level `crates/fe-vim/tests/remap_vimrc.rs`.

## Review note (lead respec after round 3)

Two gaps in the `let mapleader` string decoder held C2 open; fix them and keep everything else:

- **Numeric modifiers.** Recognize nvim's `2-`, `3-` and `4-` modifiers (as in `"\<2-LeftMouse>"`)
  before classifying the key. A recognized key fe-vim can't represent, and an unknown name, gives
  a `VimrcIssue` at its line (leader unchanged), never literal text or a mapping on the wrong keys.
  Regressions for all three modifiers and for an unknown name.
- **`<char-N>`.** Parse N as nvim does: `0x`/`0X` hex, a leading `0` octal, with nvim's decimal
  fallback when a leading-zero sequence holds an 8 or 9, and nvim's integer conversion. Regressions
  that map and unmap through an equivalent leader for each case.

Where nvim's own behaviour is unclear, prefer an issue over a guess.

## Implementation notes

- Each record is checked with `RemapSet::new` as its line is read; one it rejects is left
  out of `remaps` (so the config card doesn't report it twice) and doesn't override or
  unmap anything.
- `<leader>` is replaced in the record's text by the leader in vim notation (`,`,
  `<Space>`, `<lt>`), so the records carry no `<leader>`. A `<` that the leader would close
  into a `<name>` is written `<lt>`, matching `RemapSet`'s reading of the pieces apart.
- `unmap` compares keys, not spellings (`<Space>a` unmaps `<space>a`), drops trailing blanks
  as nvim does, and unmapping something not mapped is silent: pasted vimrcs unmap nvim's
  default maps (`nunmap Y`), which fastedit doesn't have.
- `vunmap` and `vmapclear` act on Visual mode without a note; only `vmap`/`vnoremap` note
  Select mode. `lmap` and `tmap` families get their own messages; `{i,c,s}unmap` and
  `{i,c,s}mapclear` are reported like their map forms.
- After `|`, the next command on the line is read too. An unsupported ex command ends the
  line, since it may own the `|` (`autocmd`, `normal`).
- `let mapleader` takes the string's value as nvim's `eval` gives it: `\x` and octal escapes
  are bytes (octal truncated to a byte), `\u`/`\U` are characters, and the bytes are then
  read as UTF-8, so `"\xC3\xA9"`, `"\303\251"`, `"é"` and `'é'` are one leader. A NUL
  ends the value; what follows it isn't checked. `\<…>` is read as nvim's
  `find_special_key` reads it in a string (modifiers, `<C-[>`, `<S-a>`, `<char-N>`, `t_xx`
  termcap names and nvim's key-name table with its aliases), then made one `VimKey`:
  `\r`, `\<C-M>` and `\<CR>` are `<CR>`; `\b` and `\<C-H>` are `<C-h>`, distinct from
  `\<BS>`. A name nvim doesn't know stays literal text (`\<foo>`, `\<C-foo>`, `\<Nop>`,
  `\<Leader>`); a key nvim knows that fe-vim can't type (`\<F2>`, `\<S-Tab>`, `\<M-a>`,
  `\<C-@>`) is an issue and the leader is unchanged. Checked against nvim 0.12.4 on about
  8,500 strings: every name in its key table, each with 15 modifier spellings, and every
  printable ASCII character after a modifier.
- The mapclear family takes a trailing `"` comment, as nvim's `:mapclear` does; the map and
  unmap families keep `"` as part of their keys.
- After the round-3 respec: `2-`, `3-` and `4-` are read as nvim's click modifiers, so
  `\<2-LeftMouse>` and `\<C-3-a>` are issues (keys fe-vim can't type). An unknown name after a
  click modifier (`\<2-foo>`) is an issue too, though nvim leaves it as text, as the respec
  asks. `5-` and `1-` aren't modifiers, so `\<5-a>` is text.
- `<char-N>` reads N as nvim's strict `vim_str2nr` with every radix: `0x`, `0o`, `0b`, a
  leading `0` octal unless a digit is 8 or 9 (`<char-018>` is 18), saturating, then the low
  32 bits as a C `int` (`<char-4294967361>` is `A`; a negative value is a special key, an
  issue). A number nvim rejects (`<char-0x>`, `<char-65a>`, `<char->`) is an issue, as nvim
  fails the `let`. A `-` sign is read by the scanner, so `<char--1>` is text as in nvim.
  Rechecked against nvim 0.12.4 on about 8,900 strings, with nvim's errors expected as issues.
- `\u` and `\U` values are C `int`s as in nvim: a negative one writes its low byte alone
  (`"\U80000041"` is `A`). Nothing is replaced with U+FFFD: a value whose bytes aren't UTF-8
  (`"\xFF"`, `"\xC3"`) or whose code isn't a character (`"\uD800"`, `"\U110000"`) is an issue,
  "mapleader isn't valid UTF-8", and the leader is unchanged.
