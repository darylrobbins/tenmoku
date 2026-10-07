---
task_uid: "01m49xn12w0dn08e68ew8mrfwj"
task: "T155"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-vim test tests::operators::transactions_origins_and_failed_targets: HeadlessVim d3w dispatched as Actor::Ai produces exactly one TransactionId in Dispatched::transactions, and Buffer::undo with an Ai filter restores the text; yy produces no transaction and leaves the text and dirty flag unchanged; dfz on a line without z, d% on a line without a bracket and di( outside any parentheses each leave the text, the cursor and every register unchanged and produce no transaction; with two carets dw deletes both words in one transaction and leaves a caret at each, the register holding the last caret's word; vim.operate with operator change returns CommandError and changes nothing"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::registers::slots_and_rules: on Registers directly, \" follows the last write; 0 is written only by a yank without a named register; a within-line delete writes - and leaves 1; a linewise or multi-line delete shifts 1..9 and drops the old 9; a named delete writes its register and not -, and a named linewise or multi-row delete still shifts 1..9 (as nvim 0.12.4 does, :help quote_number); a delete over % writes 1 even within a line; A appends to a and the result is linewise if either part was; _ writes nothing and leaves \" unchanged; an invalid register name is rejected"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::registers::clipboard_through_the_host: \"+yy calls EditorHost::write_clipboard with the line and its newline; \"*p reads read_clipboard and pastes linewise below when the text ends in a newline and charwise after the caret otherwise; + and * read and write the same host clipboard; \"+p with an empty clipboard changes nothing"
    required: true
    manual: false
  - id: "C4"
    text: "fe-vim test tests::operators::counts_are_bounded: on a 3-row buffer, 99999999dd, 99999999x and 4294967295X each finish within the test; vim.put with count 4294967295 of a 1-byte register fails with CommandError and changes nothing; 3p of a 1-byte register inserts 3 bytes"
    required: true
    manual: false
  - id: "C5"
    text: "fe-vim test operators_match_neovim (tests/oracle.rs): tests/corpus/operators.toml has at least 20 cases covering d, y, \u003e and \u003c over exclusive (w b h l 0 ^), inclusive (e $ f t %) and linewise (j k gg G) motions and dd yy \u003e\u003e \u003c\u003c, the column-0 and exclusive-linewise adjustments, dw at a line's end, counts before and after the operator, x X D with counts, and ; and , after f and t; each matches its nvim 0.12.4 expectation in buffer, cursor, mode and registers"
    required: true
    manual: false
  - id: "C6"
    text: "fe-vim test text_objects_match_neovim (tests/oracle.rs): tests/corpus/text_objects.toml has at least 20 cases covering each of iw aw iW aW i( a( i{ a{ i\" a\" i' a' and the aliases ib ab iB aB i) a) i} a}, nested and multi-line brackets, the caret on a bracket, an escaped quote, the caret before the line's first quote, counted iw and aw, and an object that doesn't exist; each matches its expectation in buffer, cursor, mode and registers"
    required: true
    manual: false
  - id: "C7"
    text: "fe-vim test registers_match_neovim (tests/oracle.rs): tests/corpus/registers.toml has at least 10 cases covering named a-z, A-Z append, _, the numbered shift, -, 0, and p and P charwise and linewise with counts and named registers; each matches its expectation in buffer, cursor, mode and registers"
    required: true
    manual: false
  - id: "C8"
    text: "fe-vim test motions_match_neovim still passes with motions.toml and motions.expected.json unchanged, now also comparing registers; every corpus test compares the registers \" 0-9 - a-z (text and linewise) against the recorded set; the new *.expected.json files are the recorder's unedited output (manual: rerunning record_corpus_with_neovim leaves git status clean)"
    required: true
    manual: false
  - id: "C9"
    text: "vim.operate is registered with context editor \u0026\u0026 vim.mode == normal and mutates_buffer true, and vim.put the same; scripts/check-no-gpui.sh prints ok: fe-vim; no Cargo.toml or Cargo.lock change; no file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim"
    required: true
    manual: false
---

# fe-vim: the operators d, y, > and <, twelve text objects, %, ; and ,, registers, and x X D p P

# fe-vim: the operators d, y, > and <, twelve text objects, %, ; and ,, registers, and x X D p P

Milestone M7 "Vim", card 2 of 3 (see card 1's table). It builds on card 1's crate, module layout,
reducer, `HeadlessVim` and oracle, so read card 1's spec (`docs/specs/` for its lane) first. The
design is `docs/decisions/vim-engine.md` §1 (outline), §4 (the `d3w` and `"ayy` traces: `"-`
for a within-line delete, `"0` untouched with a named register), §9 items 3, 4 and 9.

**Clean room** as card 1: no Vim/Neovim source, no hjkl, no Zed `vim` crate, no Helix.
Behaviour comes from running `nvim --clean` and `:help`, recorded into the corpus.

## Change

### Grammar (`src/reducer.rs`, `src/args.rs`)

- New motions: `match_pair` (`%`), `repeat_find` (`;`) and `repeat_find_reverse` (`,`).
  Each works with `vim.move` and as an operator target.
- New keys: `x` (= `dl`), `X` (= `dh`) and `D` (= `d$`), each committing `vim.operate` with its
  count and register. `p` and `P` commit `vim.put { before: bool, count: Option<u32>, register:
  Option<char> }` (`PutArgs`).

### `vim.operate` (`src/operator.rs`, `src/commands.rs`)

- It is registered with `register_with_args::<OperateArgs>`, context `editor && vim.mode ==
  normal`, and `mutates_buffer(true)`. `change` is accepted by the args but answered with
  `CommandError("change needs insert mode (card 3)")` until card 3.
- **Range classes.** `Motion::class() -> Exclusive | Inclusive | Linewise`. Then come Vim's
  operator adjustments, as `:help exclusive` and `:help exclusive-linewise` describe them:
  - An exclusive range ending at column 0 becomes inclusive, ending on the previous line.
  - If it also starts at or before the first non-blank, it becomes linewise.
  - `dw` on a line's last word doesn't take the line break.
  - The corpus is the judge of each rule.
- **Text objects** resolve against the text at dispatch (§9.4):
  - `iw aw iW aW`, with counts.
  - `i( a( i{ a{`, with the aliases `b ( )` and `B { }`. They take the innermost enclosing
    pair, may span lines, and work with the caret on a bracket.
  - `i" a" i' a'`, on the caret's line. They honour a backslash-escaped quote, and if the caret
    is before the first quote they take the next quoted string. `a"` takes trailing white
    space, or leading white space if there is none trailing.
  - An object that doesn't exist cancels the operator: no edit, no register change, no move.
- **Operators.**
  - `delete` removes the range and fills registers (below).
  - `yank` fills registers, leaves the text alone, and moves the caret to the range's start
    (linewise: it stays on its row).
  - `indent`/`outdent` are always linewise. They shift by `shiftwidth = model.tab_size()`
    spaces (expandtab), skip empty lines, and leave the caret on the first non-blank.
  - The caret after a delete follows Vim, linewise deletes included (first non-blank of the
    line that moves up, with `startofline` off as nvim defaults). The corpus is the judge.
- **A failed motion** (`f` with no match, `%` with no bracket) cancels the operator the same way.
- **Transactions.** Each operate that changes text is one `fe_buffer` transaction. It goes
  through `model.buffer_mut().edit(…)`, stamped with `fe_editor::commands::origin(actor)`, and
  its id is reported in `Dispatched::transactions`. A yank produces none.
- **Several carets.** Each caret's range is operated on in the same transaction, and a caret is
  left at each. The register holds the primary (last) caret's text.
- **Counts** are bounded by the text, as in card 1. A put whose inserted text would exceed
  64 MiB fails with `CommandError` and changes nothing.

### Registers (`src/register.rs`, §1 "Registers … as a service in `fe-vim`", §9.3)

- `Registers` holds `" 0 1-9 - a-z`, each slot `{ text, linewise }`. One instance is shared by
  every editor in a host (`VimTarget` gains `registers: &'a mut Registers`), and non-vim
  commands may use it later.
- **Writes**, by Vim's rules (`:help registers`):
  - a yank writes `0`, unless a register was named
  - a delete or change that is linewise or spans lines shifts `1`→`2`…`9` and writes `1`;
    with a named register, `1` is untouched
  - a smaller delete writes `-`, except a delete over `%`, which always writes `1`
  - `a-z` replace, and `A-Z` append (linewise if either part is)
  - `"` points at whichever register was written last
  - `_` discards and leaves `"` alone
- `+` and `*` are one slot: the host clipboard, through `EditorHost::write_clipboard` and
  `read_clipboard`. Text read from it is linewise when it ends in `\n`. They're tested in unit
  tests only, since the corpus can't use them (card 1's loader).
- `vim.put`: charwise after/before the caret, or linewise below/above its row, `count` times.
  - The caret ends on the last pasted grapheme (charwise), or the first non-blank of the first
    pasted line (linewise).
  - An empty register puts nothing, and that isn't an error.

### Harness (`tests/support/mod.rs`)

From this card on, every corpus test also compares the registers `" 0-9 - a-z`: text and
linewise, the same set as recorded. Card 1's `motions_match_neovim` keeps passing with its
committed expectations unchanged.

### Corpus (recorded with card 1's recorder and nvim 0.12.4)

- `tests/corpus/operators.toml`, with at least 20 cases:
  - `d y > <` over motions of each class: exclusive `w b h l 0 ^`, inclusive `e $ f t %`,
    linewise `j k gg G`, and line `dd yy >> <<`
  - the column-0 and exclusive-linewise adjustments, and `dw` at line end
  - counts before and after the operator
  - `x X D` with counts, and `;`/`,` after `f` and `t`
- `tests/corpus/text_objects.toml`, with at least 20 cases: each of the 12 objects and the
  bracket aliases, nested and multi-line brackets, the caret on a bracket, an escaped quote,
  the caret before the first quote, counted `iw`/`aw`, and an object that doesn't exist.
- `tests/corpus/registers.toml`, with at least 10 cases: `a-z`, `A-Z` append, `_`, the
  numbered shift, `-`, `0`, and `p`/`P` charwise and linewise with counts and named registers.

### Tests (new files)

- `src/tests/operators.rs` and `src/tests/registers.rs`.
- Three corpus tests in `tests/oracle.rs`: `operators_match_neovim`,
  `text_objects_match_neovim` and `registers_match_neovim`.

## Out of scope

- `c`, insert mode, `J r ~`, undo/redo and `.` (card 3).
- Visual modes and the other later M7 cards.
- The GPUI wiring (§10; a separate later card).
- Any change outside `crates/fe-vim`.

## Decided (lead defaults; the user may revisit)

- `Registers` is one shared instance per host, passed in `VimTarget`. With several carets, the
  register holds the primary caret's text.
- `"+` and `"*` share the host clipboard. macOS has one clipboard, and nvim's macOS provider
  also maps both registers to it.
- A put over 64 MiB is refused, so a script's count can't build gigabytes.

## Implementation notes

**Where nvim 0.12.4 and this spec's wording differ, the code follows nvim** (each is a recorded
corpus case):

- A delete into a named register still shifts `1`..`9` and writes `1` when it's linewise or
  spans rows (`"add`), as `:help quote_number` says ("even when the command specified another
  register"). Only `-` is skipped for a named register. `registers.toml`
  `named_delete_line_also_writes_1` records it, and `tests::registers::slots_and_rules`
  asserts it. C2's "a named delete leaves 1 untouched" doesn't hold in nvim.
- `>` and `<` leave the caret on the first shifted row at its display column (nosol), with `$`
  aiming at the row's end, rather than on the first non-blank.
- A charwise put of text spanning rows leaves the caret at the start of the put text; only a
  single-row put ends on its last grapheme.

**The oracle (`tests/support/oracle.lua`)** changed in two ways. Neither changes
`motions.expected.json`.

- `--clean` loads matchit, whose `%` differs from Vim's builtin (`d%` within a row skips `"1`).
  The recorder unmaps it, so the corpus records the builtin `%` that `:help %` describes.
- `nvim_replace_termcodes` now gets `do_lt`, so `<lt>` is `<` as `parse_keys` reads it. Before,
  a case with `<lt>` would have been recorded as the literal keys `<lt>`.

**Rules learned from nvim beyond the spec**, all in the corpus:

- `:help d`: a charwise delete over rows, with only blanks before it and after it, takes the
  rows whole. A range ending on an empty row does too, for any operator, unless an inclusive
  motion's (as `exclusive-linewise`).
- An operator's `h` at column 0, `0` there, or `t` onto the next grapheme isn't a failure.
  `>h` still shifts the row, and `dt.` takes the caret's grapheme.
- A `b` that can't move fails.
- A yank over nothing writes an empty register; a delete over nothing doesn't.
- `""` names register `0`.
- `i(` with no pair around the caret takes the pair opening `count` deep in the brackets after
  it, unless a closing bracket comes first.
- Quote objects pair the row's quotes from its start when the caret is on one.
- `%` also matches `/* */` (with the caret on either end) and `#if`/`#else`/`#endif`, whose rows
  an operator takes whole.

**Counted `iw` and `aw` across rows** follow models fitted to nvim's recordings. Over
random buffers full of empty and blank rows, they match about 7,000 recorded cases of
`iw`/`iW`/`aw`/`aW` with every count.

- `iw`: each further item starts one position past the last. From an empty row, it ends at the
  next row's start, exclusively, and that end is then handled as `exclusive-linewise` handles it.
- `aw`: items alternate blanks-then-word and word-then-blanks. Blanks run across rows, an empty
  row starting them counts as a blank, and an empty row after them is the word.

**Known limits.** About 35,000 random cases were compared with nvim. These classes still
differ:

- `%` from a `*/`, in 2 of 3,000 random comment-marker cases (`**// /*` and `*//x /*`, where a
  `//` right after a stray `*/` still reads as a line comment). The rule, found in one pass: the
  nearest `/*` that counts is the match unless a `*/` follows it, and an earlier one that no
  `*/` follows wins over it. A `/*` after a `//` line comment on its row doesn't count, and
  neither does one right after a `*`, unless it's `/*/`.
- nvim discards queued keys after a command fails (its typeahead flush). `HeadlessVim` reads on,
  so a corpus case shouldn't type past a failure that matters.
- `i[` and `a[` aren't among this card's twelve objects.
