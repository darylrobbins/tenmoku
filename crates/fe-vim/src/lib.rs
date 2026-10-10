//! Vim mode for fastedit's editor, headless. Must never depend on GPUI.
//!
//! Keys go through a pure reducer, [`step`], which keeps the whole
//! incomplete command (count, `"{reg}`, operator, a prefix such as `g`, `f`
//! or `r`) in one [`Pending`] value and never sees the buffer. A complete
//! command comes out as an [`Invocation`](fe_command::Invocation) of one of
//! vim's commands (`vim.move`, `vim.operate`, `vim.put`, `vim.insert`,
//! `vim.join`, `vim.replace_char`, `vim.toggle_case`, `vim.undo`, `vim.redo`,
//! `vim.repeat`, `vim.visual`, `vim.visual_exit`, `vim.visual_swap`,
//! `vim.visual_put`, `vim.reselect`, `vim.select` and the command line's
//! `vim.command_line`, `vim.command_line_submit` and
//! `vim.command_line_cancel`, for searches and ex commands), dispatched through
//! [`registry`] against
//! a [`VimTarget`], which carries the host's shared [`Registers`].
//! [`VimState`] holds the mode, the pending value, the goal columns, the
//! last character search, the open insert session, the visual selection and
//! the last one (`gv`, `'<` and `'>`), the last change for `.` and the
//! carets before each change for `u`, and publishes `vim.mode` (plus
//! `vim.pending` and `vim.pending_kind` while a command is incomplete, and
//! the flag `vim.visual` in a visual mode) on the editor's own context
//! frame, so keymap bindings can be scoped to a mode. With vim off nothing
//! stamps the frame and `vim.mode` is never set. The design is
//! `docs/decisions/vim-engine.md` (§1, §4, §9 and §10).
//!
//! An insert session (`i a I A o O`, or the change operator `c`) is one
//! `fe_buffer` transaction with the actor's origin, opened by the command
//! that enters insert mode. In insert mode keys aren't vim's: typed text
//! reaches the editor as ordinary input (`EditorModel::insert`) and joins
//! the transaction, and `vim.normal_mode` (bound to `Esc` and `ctrl-[`)
//! ends it. `.` replays the last change, and the text its session typed, as
//! one transaction.
//!
//! In the visual modes (`v` charwise, `V` linewise; `vim.mode` is `visual`
//! or `visual_line`) keys still go to [`step`], which [`VimState`] tells the
//! mode is on, so an operator acts on the selection at once
//! (`Target::Selection`, or `Target::SelectedLines` for `X D Y C S R`), as do
//! `~ u U J r{char}`, and `i`/`a` select text objects (`vim.select`). The vim
//! cursor and the anchor are anchors on grapheme starts (or a row's end,
//! after `$`), and the model's one selection mirrors them, taking the
//! grapheme under the cursor, so the editor paints and copies what vim
//! would take; hosts paint the block cursor at
//! [`VimState::visual_cursor`] and drive a mouse drag through
//! [`VimState::select_visual`]. Leaving a visual mode by any path keeps its
//! extent for `gv` and as `'<` and `'>` ([`VimState::mark`]). `.` repeats a
//! change over a selection on as much text from the caret.
//!
//! Blockwise visual mode (`<C-v>` or `<C-q>`; `vim.mode` is `visual_block`)
//! selects the rectangle of display columns between the anchor and the
//! cursor, each taking its grapheme's columns, over the rows between them;
//! after `$` each row to its end, until a horizontal motion. On the model
//! it's one selection per row, over the graphemes the rectangle overlaps (a
//! row ending before it gets none), so the editor paints and copies it
//! (`cmd-c` joins the rows with `\n`). `o` goes to the opposite corner and
//! `O` to the other one on the cursor's row. Operators act on each row's
//! part (`:help blockwise-operators`): a tab or wide character only partly
//! inside counts as spaces for its columns there, which a delete leaves for
//! its columns outside. `d x y X Y` and `D` (to the rows' ends) write a
//! blockwise [`Register`], its rows joined by `\n` with its width
//! (`Register::block_width`), and `p` and `P` put one as a block from the
//! caret's column, padding short rows (and each row to the block's width,
//! a tab in it counting a whole tab stop, as nvim measures it) and adding
//! rows past the end. `c s C`
//! delete it and `I` and `A` (`$A` at each row's end) open an insert session
//! on its first row whose text `Esc` types on the other rows too, in the
//! same transaction, where the block's column fell on each as the session
//! opened: on those still as they were then (rows another origin put in or
//! took out around them don't matter), so a row whose text they changed
//! meanwhile, or joined to another, gets none, and their text is never
//! touched. As in nvim, what `Esc` types there is what the first row grew
//! by, taken from where typing began on it, and the cursor keys
//! ([`VimState::split_session`]) keep the block: typing begun at or left of
//! the block's left column for `A` (left of the row's end for `$A`), or
//! right of `I`'s column if none began at it, moves where that text comes
//! from and goes, and it goes in if the cursor is on that row then (`c`:
//! wherever it is). Another origin's edit on that row before the end of
//! that text leaves it unknown, and nothing goes on the other rows. `S R`
//! and `J` take its rows. In
//! every visual mode `p` and `P` (`vim.visual_put`) replace the selection
//! with a register in one edit, `p` leaving what it replaced in the
//! registers as a delete would.
//!
//! # The command line
//!
//! `/` and `?` open a command line, through [`VimState::feed`] ([`step`]
//! alone forwards them, having no line to open): they commit
//! `vim.command_line` with `{"kind": "search_forward"}` (or
//! `search_backward`), carrying the pending count, register and operator
//! (`d2/`, `"ay?`). `vim.mode` is then `cmdline`, with `vim.cmdline_kind`
//! the line's kind, until the line ends; the mode it came from (normal, an
//! operator pending in it, or a visual mode with its selection) is kept.
//!
//! The host owns the line: it shows it, takes the keys typed into it (they
//! don't go to [`step`]) and edits its text, then ends it with
//! `vim.command_line_submit {"text": "…"}` (`Enter`) or
//! `vim.command_line_cancel` (`Esc`, or a backspace on an empty line), both
//! bound in the context `editor && vim.mode == cmdline`. fe-vim owns what
//! ending it means: either one goes back to the mode the line came from,
//! and cancelling drops what it carried. `HeadlessVim` stands in for the
//! host, as it does in insert mode. A host with no line yet still leaves
//! it: [`VimState::feed`] in `cmdline` mode reads `<Esc>` as a cancel and
//! forwards the rest.
//!
//! Submitting a search runs `vim.move` with the motion `{"search":
//! {"pattern": "…", "forward": true}}` ([`Motion::Search`]), or with an
//! operator pending `vim.operate` with that target, with the carried count:
//! the `count`th match after the caret (before it, for `?`), wrapping round
//! the text, as [`search::Pattern::find`] finds them. In a visual mode it
//! moves the vim cursor, keeping the anchor. As a target it's exclusive and
//! charwise, with `:help exclusive`'s rules (`d/^foo` from a row's first
//! non-blank takes whole rows), and a delete writes register `1` even within
//! a row, as after `%`. A match at a row's end, on a row with text, is its
//! last character, as in nvim. The line's pattern ends at an unescaped `/`
//! (`?` for `?`, where `\?` is a `?`) outside a collection (which `[`
//! starts, or `\[` after `\V`); an empty one is the last search's. A
//! search motion is checked however it comes (`/`, `vim.move` or
//! `vim.operate` dispatched directly, `.`): like nvim, its pattern becomes
//! the last search, with its direction, even when nothing matches or Vim
//! rejects it; one fe-vim can't search (a translation error) isn't kept. A
//! search that fails (E35 with no last pattern, E486 with no match, a
//! pattern error, or a search offset) changes nothing else and leaves the
//! error in [`VimState::message`] for the host to show, until the next key.
//! A count past the last match goes round, its work bounded by the text's
//! matches, not the count.
//!
//! `:` opens an ex command line: [`step`] commits `vim.command_line` with
//! `{"kind": "ex"}` (`vim.cmdline_kind` is `ex`), dropping a register, and
//! an operator pending before it cancels. It goes back to normal mode
//! however it ends. After a count the line starts with `.,.+{count-1}`
//! (`.` for 1); from a visual mode, which it leaves (setting `'<` and `'>`
//! and putting the caret where nvim does: a charwise extent's start, a
//! block's top-left corner, and for a linewise one the cursor if it's
//! before the anchor, else the first row's start), with `'<,'>`. That text is
//! [`VimState::command_line_prefill`], for the host to put on its line.
//!
//! The last search is the host's, in its [`Registers`], which every editor
//! shares: the read-only `/` register holds the pattern ([`Registers::get`]
//! and [`Registers::last_search`]). `"/p` puts it (E35 until a search sets
//! it); an operator that writes registers (`d y c`) refuses `"/`, doing
//! nothing (and leaving a visual mode), as nvim does, and the others ignore
//! it.
//!
//! `n` and `N` (`search_next`, `search_previous`) search for the last
//! search's pattern in its direction or against it, `*` and `#`
//! (`search_word_forward`, `search_word_backward`) for the keyword under or
//! after the caret as `\<word\>` (with none on the rest of the row, the
//! non-blank text there) from its start, and make that the last search.
//! They resolve to a search motion, so everything above holds: exclusive
//! after an operator, the visual cursor in a visual mode, E35, E348 and
//! E486 in [`VimState::message`]. [`VimState::highlight`] is the last
//! search's pattern for the host to paint ('hlsearch'). In a visual mode
//! nvim's own mappings make `*` and `#` search the selection; fe-vim's
//! search for the keyword at the cursor.
//!
//! # Ex commands
//!
//! Submitting an ex line parses it ([`Ex::parse`], pure) and runs it. A
//! line is `[range] command [args]`, leading `:`s and blanks skipped. As
//! in nvim the range is read first, its searches and `;` taking effect
//! even when the command then turns out wrong. The range
//! (`:help cmdline-ranges`) is line specifiers separated by `,` or `;`,
//! where `;` moves the cursor to the line (or the nearest there is) before
//! the next is read, and leaves it there even if the line then fails. Of
//! several, the last two count; of one, it's both ends; a separator with
//! nothing on a side stands for `.`. A specifier is a base, `N`, `.`, `$`,
//! `%` (`1,$`), `'<` or `'>`, then any `/pat/` and `?pat?` (the closing
//! delimiter optional at the line's end), each searching from the line
//! before it, then `+N` and `-N` offsets (a bare `+` or `-` is 1, a bare
//! `N` is `+N`, and blanks may come before them: `.2` and `2 2` are `.+2`
//! and `4`). With no base, `.` is meant: `+2` is `.+2` and `/pat/`
//! searches from the cursor's line. Searches and offsets after `%` are
//! ignored, and with no command after them the line is E492, as in nvim.
//! Line numbers are nvim's 32 bits: a number past `2^31 - 1` wraps (and
//! past 64 bits is 0), an offset taking a line past them is E1247, and
//! `:2147483647` alone doesn't move. A pattern search
//! goes to the next line that matches (the previous, for `?`), wrapping
//! round the text, the line it started from last; its pattern, in the
//! syntax below, becomes the last search with its direction and is
//! highlighted, even if no line matches (E486). An empty pattern, `\/` and
//! `\?` are the last search's (E35 with none); `\&` is the last substitute
//! pattern, which nothing sets yet (E33). Line 0 is before the first.
//!
//! - `:{range}` alone goes to the range's last line as `{N}G` does, keeping
//!   the goal column; past the last line it stops there. With no range
//!   (`:` alone) nothing happens.
//! - `:d[elete] [x] [count]` (and the abbreviations `:help :d` lists, `:dl`
//!   and `:dp` among them) deletes the range's lines (by default the
//!   cursor's) into register `x` as `dd` does, writing `"` and `1`; with a
//!   count, that many lines from the range's last, stopping at the end. As
//!   in nvim, `:d "` names no register and `:d #` writes `"` as `""dd`
//!   does. It
//!   is one transaction, and one `u` undoes it, but unlike `dd` it isn't
//!   what `.` repeats. The caret goes to the first non-blank of the line
//!   that took the range's place.
//! - `:noh[lsearch]` clears [`VimState::highlight`] until the next search.
//! - File and window commands aren't run: `:w[rite][!]`, `:q[uit][!]`,
//!   `:wq[!]`, `:x[it][!]`, `:exi[t][!]`, `:e[dit][!] [path]`, `:qa[ll][!]`
//!   (`:quita[ll]`) and `:wa[ll]` leave a [`HostCommand`] for the host to
//!   fetch once with [`VimState::take_host_command`] (and map to its own
//!   commands), changing no text, cursor or register. They take no range,
//!   and no argument but `:e`'s path; `:w {path}` isn't supported yet.
//!
//! What can't run changes nothing (but where a `;` moved the cursor) and
//! leaves nvim's error in [`VimState::message`]: E492 for any other command
//! (`:frobnicate`), E1247 for an offset past 32 bits, E16 for a line
//! before the first or (but for `:{range}`
//! alone) past the last, E493 for a range given backwards (nvim asks
//! whether to swap it, and headless nvim answers no), E20 for a mark not
//! set, E477 for a `!`, E481 for a range on `:noh`, E488 for trailing
//! characters and E939 for a count of 0.
//!
//! Not yet: `:s`, `:&` and
//! `&`, the GPUI line. Not planned for M7's first ex cards: `:g`, `:v`,
//! `:normal`, `:m`, `:t`, `:co`, `:j`, `:y`, `:>`, `:<`, `:pu`, `:r`,
//! `:sort`, `:set`, `:marks`, `:reg`, `:undo`, `:!`, `|` between commands,
//! the line's history and completion, marks other than `'<` and `'>`
//! (`'a` is E20 until marks come), `*` for `'<,'>`, and a count before
//! `:` in a visual mode (nvim drops it too). These are E492, or, for a
//! `|` after `:d`, E488.
//!
//! # Search patterns
//!
//! [`search::Pattern::parse`] translates a subset of Vim's pattern language
//! (`:help pattern`) to a `regex::Regex`. Supported, in the default 'magic'
//! syntax:
//!
//! - Literal text. A backslash before punctuation makes it itself (`\.`
//!   `\*` `\[` `\~` `\^` `\$` `\/` `\\`), and a trailing backslash is
//!   itself.
//! - `.`, any character but a line break.
//! - Collections `[…]`: `^` first negates; `a-z` ranges; `]` first is
//!   itself (and may start a range), as is `-` first or last; the codes `\e \t \r \b \d123 \o40
//!   \x20 \u20AC \U1234abcd` and `\\ \] \^ \-`; and the ASCII classes
//!   `[:alnum:]` `[:alpha:]` `[:blank:]` `[:cntrl:]` `[:digit:]` `[:graph:]`
//!   `[:punct:]` `[:space:]` `[:xdigit:]` `[:return:]` `[:tab:]`
//!   `[:escape:]` `[:backspace:]`. A `[` with no `]` to end it is itself.
//! - The multis `*` `\+` `\=` `\?` `\{n,m}` `\{n}` `\{n,}` `\{,m}` `\{}`
//!   and their as-few-as-possible forms `\{-n,m}` `\{-n}` `\{-n,}` `\{-,m}`
//!   `\{-}` (bounds given the wrong way round are swapped). A `*` with
//!   nothing before it, at the start of a branch or right after `^`, is
//!   itself (not `\*` after `\M` or `\V`, which is misplaced there).
//! - Groups `\(…\)` (at most nine) and `\%(…\)`, and `\|` between
//!   branches.
//! - `^` at the start of a branch and `$` at its end, matching a row's start
//!   and end; elsewhere they're themselves. `\<` and `\>`, a word's start
//!   and end.
//! - The classes `\s \S \d \D \w \W \a \A \l \L \u \U \x \X \o
//!   \O \h \H`, all ASCII, which `\c` doesn't make caseless, as in Vim;
//!   and `\e \t \r \b`.
//! - `\c` and `\C` anywhere (`\c` wins); without either, case matches
//!   exactly ('ignorecase' off). `\c` folds case as nvim does: two characters
//!   match when each one's uppercase is a single character and those have
//!   the same lowercase, so `k` takes `K` and the Kelvin sign, `s` takes
//!   long s and `σ` takes `ς`, while `ß` and `ẞ` (and dotless `ı`) match
//!   only themselves. A collection's members fold the same way, but
//!   for `[:name:]` classes, which fold within ASCII, and for members
//!   spelling out `\l`, `\u`, `\a`, `\h` or `\w` (`[a-z]`, `[A-Z]`,
//!   `[a-zA-Z]`, `[a-zA-Z_]`, `[0-9A-Za-z_]`, in any order) as plain
//!   characters in a collection with no `[:name:]` class, which also fold
//!   within ASCII: `\c[a-z]` doesn't take the Kelvin sign, but `\c[a-y]`,
//!   `\c[\x61-z]` and `\c[a-z[:digit:]]` do. `\v`, `\m`, `\M` and `\V` switch to very
//!   magic, magic, nomagic and very nomagic for what follows.
//! - `~`, as itself: Vim's last substitute string doesn't exist yet.
//!
//! Anything else is an error naming the atom, "pattern not supported: \zs",
//! never a different match: `\n`, `\_x`, `\zs` and the other `\z` atoms,
//! `\%V` and the other `\%` atoms but `\%(`, `\@`, `\&`, back-references
//! `\1`…`\9`, the option-driven classes `\i \k \f \p` and their
//! capitals, `\Z`, a backslash before any other letter or before a
//! non-ASCII character, `[:lower:]` `[:upper:]` `[:print:]` `[:ident:]`
//! `[:keyword:]` `[:fname:]` and any
//! malformed `[:`, `[[=x=]]`, `[[.x.]]` and `\n` in a collection, a range in
//! one that ends at `\\` `\]` `\^` `\-`, at another `-` or at a class, or
//! that starts at a class (Vim reads those its own way), and a `*` right
//! after `\%(`. So is a pattern Vim rejects (an unmatched `\(` or
//! `\)`, a misplaced or nested multi, a malformed `\{…}`, a reverse range,
//! a tenth `\(`), an empty one, and one past `regex`'s size limit or
//! nested over sixty groups deep.
//!
//! Matching is row by row: a match never spans a line break, which is why
//! `\n` and `\_x` aren't translated. [`search::Pattern::find`] goes where
//! `/` and `?` would ('wrapscan' on), visiting each row once, and
//! [`search::matches_in`] lists the matches on some rows, at most a cap of
//! them, for a host to paint. Within a row matches come as Vim's search
//! finds them: each search resumes where the last match ended, or one
//! character on after an empty one, so overlapping matches aren't found,
//! and an empty match at the row's end is found only by a search that
//! starts before it. A character there is a whole one as Vim counts them:
//! an extended grapheme cluster (a letter and its composing characters, an
//! emoji sequence), or a control character with the nonspacing marks after
//! it. `/` starts a whole character on from the cursor, wherever in one it
//! is, so from a row's last character it goes on at the next row. Finding a match on a row walks the matches before it
//! on that row, as Vim does, so the cost grows with them: a 64 MiB row of
//! `a` takes seconds for `a`, and a pattern that scans to the row's end for
//! each match (`.*b\|a` over a long row of `a`) is quadratic in the row's
//! length, as in Vim. Nothing interrupts a search yet.
//!
//! Where the translation differs from Vim:
//!
//! - Where alternatives match at the same place, the first that matches
//!   wins, as with Vim's backtracking engine (`\%#=1`). Its default NFA
//!   engine sometimes takes a later, longer one: `\>\|.\=` at a word's
//!   end matches the next character there, where fe-vim's match is empty.
//! - `\<` and `\>` go by Unicode's word characters (letters, marks, digits,
//!   connector punctuation), which are Vim's default 'iskeyword' on ASCII
//!   text. Elsewhere Vim also starts a word where the character class
//!   changes (CJK after Latin, say), and counts `×` and `÷` as word
//!   characters and `ª` `µ` `º` as not.
//! - Composing characters under literals, `.`, character classes and
//!   collections: there a composing character (or the rest of an emoji
//!   sequence) is a character of its own, where Vim matches a character
//!   and its composing characters as one: `e` matches in `e` followed by
//!   U+0301, `.` and `\s` take the base alone, `[\u0300-\u036f]` matches
//!   the mark, and the next search on the row resumes inside the character.
//! - nvim 0.12.4 misses two folds when the pattern starts with the literal
//!   (`\cã` doesn't match `Ã`, nor `\c` and U+0345 an `ι`), though it folds
//!   them anywhere else (`\cxã`, `\c[ã]`); fe-vim folds them everywhere.
//!
//! # Clean room
//!
//! This crate is written from Vim's documented behaviour, never from
//! source. Don't open Vim's or Neovim's source (C, Lua or Vimscript
//! runtime), hjkl's crates or corpus (any `hjkl-*`), Zed's `vim` crate or
//! Helix while working on it; see `CLEAN_ROOM.md`. Learn behaviour by
//! running `nvim --clean` (0.12.4) and from `:help`. The corpus under
//! `tests/corpus/` is the spec: when unsure, add a case and record what nvim
//! does with the ignored `record_corpus_with_neovim` test. Tables nvim
//! computes per code point are recorded the same way: `src/chars.txt`
//! (word classes and widths), `src/case.txt` (what `~` does), and
//! `src/lower.txt` and `src/upper.txt` (what visual `u` and `U` do).
//!
//! # Cards
//!
//! M7 built this crate in three cards:
//!
//! 1. The keys, the whole pending-input grammar, `vim.mode`, `vim.move` and
//!    its motions (`h j k l w b e W B E 0 ^ $ gg G f t F T`), and the Neovim
//!    oracle with its recorder.
//! 2. `vim.operate` with `d y > <`, text objects, `%`, `;` and `,`,
//!    registers, and `x X D p P`.
//! 3. Insert sessions as one transaction, `c C s S i a I A o O`, `J r ~`,
//!    `u <C-r>` and `.`.
//!
//! The GPUI wiring of `vim-engine.md` §10 came next: key routing (the
//! keymap first, then [`step`] in normal mode, text input in insert mode),
//! cursor shapes, the `Esc` and `ctrl-[` bindings and the `vim.enabled`
//! setting. It also ends a session when a command other than typing runs in
//! insert mode ([`VimState::split_session`], standing in for Vim's
//! `<C-g>u`), and types `Enter` in insert mode through
//! [`VimState::line_break`], as `HeadlessVim` does, so the indentation a
//! line break copies is 'autoindent'.
//!
//! The second batch started with charwise and linewise visual mode: `v V`,
//! operators over the selection, `o`, `gv`, `'<` and `'>`, and `.` after a
//! visual change. Its second card added blockwise visual mode (`<C-v>`),
//! block insert and append, blockwise registers, and `p` and `P` in every
//! visual mode. Its third card started search with the pattern language
//! and a row-by-row matcher ([`search`]); its fourth, the `/` and `?`
//! command line, search as a motion and an operator's target, and the `/`
//! register; its fifth, `n N * #` and [`VimState::highlight`]; its sixth,
//! the `:` line with ranges, `:{range}`, `:d` and `:noh`. Later cards add
//! the host's ex commands and `:s`; the clipboard; marks and macros; replace mode,
//! increment and folds; scrolling; and the GPUI wiring of the visual modes
//! (their key bindings, painting the selection and the block cursor, mouse
//! drags and the status bar).
//!
//! # Known limits
//!
//! - A search motion's pattern is interned, so that the args holding it
//!   stay `Copy`: each distinct pattern searched for (or deserialized) stays
//!   in memory for the life of the process.
//! - Search offsets (`/foo/e`), search history and 'incsearch' aren't
//!   supported: a line with an offset fails, changing nothing.
//! - `'<` and `'>` keep a byte column on a row anchored at its start, as
//!   nvim's marks do: rows put in or taken out above move them, edits
//!   within their row don't. nvim also moves marks on rows that a change
//!   joins or replaces; fe-vim does so for the changes over a selection
//!   (their own `'<` and `'>`, or through `.` earlier ones): rows a
//!   charwise `d`, `c` or `J` joins to the first, rows a linewise `c`
//!   replaced, and rows deleted at the end of the text, where the marks
//!   stay past the new end as nvim's do. Other joins (`J`, `x` at a row's
//!   end in normal mode) leave a mark on the joined row at its old column,
//!   and text with line breaks put in at the start of a mark's row moves
//!   the mark down, as a row opened above does, where nvim keeps it for a
//!   charwise put.
//! - A count before `.` after a visual change replaces the operator's count
//!   (how many times `>` and `<` shift, or `I` and `A` over a block type),
//!   not the amount of text; nvim keeps the change's own count.
//! - In a visual mode, `aw` growing backwards onto a word after the row's
//!   indentation takes all of the indentation.
//! - After `.` of a change over a selection with `>` or `<`, the caret goes
//!   to the column the repeated selection ended at, which can differ from
//!   nvim's past wide characters and tabs.
//! - `I` and `A` in charwise and linewise visual mode do nothing; nvim's
//!   (undocumented) behaviour there isn't followed.
//! - A blockwise register goes in once, at the primary caret, leaving one
//!   caret.
//! - A yank or delete over a block takes at most 64 MiB, the most one put
//!   may put in. Rows ending before a block take spaces as wide as it, so
//!   one over a wide block and many short rows can take far more than the
//!   text holds: past that it's refused before anything changes (into `_`,
//!   which writes nothing, it goes ahead). Appending to a register (`"A`)
//!   isn't bounded so.
//! - Where nvim's blockwise commands look like accidents, fe-vim does the
//!   regular thing: `A` on a block's first row past a wide character
//!   straddling its column types the whole character on the other rows
//!   (nvim types the last bytes of it); `p` and `P` over a block after `$`
//!   with characters put them at the block's left column; and the goal
//!   column after an operator over a block whose corner is a tab is the
//!   tab's.
//! - After the cursor keys split a session over a block, what `Esc` types
//!   on the others is undone with the text typed since the last split, where nvim
//!   undoes `c`'s with the change itself: `fe_buffer` can't add to a closed
//!   undo step. With nothing typed since, undo puts the caret where typing
//!   resumed, not where `Esc` left it. After the session's own edit joins
//!   the row below to the block's first row, `Esc` types nothing on the
//!   others, as nvim does after `I` (after `A` and `c` nvim types the text
//!   the join brought too), unless the session typed that line break. After
//!   a line break typed on the first row, nvim numbers the block's rows
//!   from it, typing on the rows the line break put in and not on the
//!   block's last; fe-vim types on the block's own rows. Edits from another view of the buffer with the
//!   session's origin count as the session's own, as everywhere in insert
//!   mode. Where a tab or a wide character lies between the block's column
//!   and where typing began on its first row, nvim mixes bytes and display
//!   columns (typing nothing, or broken bytes); fe-vim counts columns.
//!
//! - Undo and redo put the carets back from fe-vim's own record of where
//!   each change began, as (row, byte column) pairs. Another origin's edits
//!   between a change and its undo can shift the rows, and then the carets
//!   land on the old rows, clamped to the text.
//! - `fe_buffer` drops a transaction that changed nothing, so a change that
//!   changed no text (`~` on punctuation, `<<` on an unindented row, a `c`
//!   over nothing with nothing typed, `3J` on the last row) leaves no undo
//!   step: `u` then undoes the change before it, where nvim undoes the
//!   no-op and only moves the caret.
//! - After undoing or redoing a `c` over several rows, nvim's caret goes to
//!   the second row at the column its motion reached; fe-vim uses the
//!   caret's own column, which differs when the motion moved it sideways.
//! - Typed `<CR>` and `<BS>` are the editor's (`EditorModel::newline` and
//!   `backspace`), not Vim's 'autoindent' and 'smarttab': a line break
//!   copies the row's indentation as it is (tabs and any other white space
//!   too) and keeps the blanks after the caret, and a backspace in an
//!   indent deletes one blank. On `Esc`, fe-vim drops the indentation the
//!   session made that went unused, as 'autoindent' does: what `o`, `O` or
//!   a linewise `c` opened a row with or kept, and what a line break typed
//!   in insert mode copied, on a row holding only white space, if nothing
//!   was typed on that row since it was made (typing deleted since counts;
//!   a backspace or a line break doesn't). Which edits are typed line
//!   breaks only the host knows: it types `Enter` in insert mode through
//!   [`VimState::line_break`], as `HeadlessVim`'s `<CR>` does. Any other
//!   input, `EditorModel::newline` called directly included, is typed text,
//!   whatever it holds, so a paste of a line break and blanks keeps its
//!   blanks. It follows those bytes through every edit
//!   while the session is open, so it never drops another origin's text or
//!   blanks that were already there, and it leaves indentation another
//!   origin changed (deleted some of, or wrote inside) as it is.
//! - A session's typing is its origin's edits, as its transaction is
//!   (`fe_buffer` groups edits by origin): another view of the buffer (a
//!   split) typing with the same origin while it's open types in it. What
//!   that view types is typed text to the session, line breaks included,
//!   unless typed through this state's [`VimState::line_break`] (or a
//!   clone's). The GPUI wiring can end a session when its editor loses
//!   focus.
//! - `fe_buffer` keeps one open transaction for the whole buffer. While a
//!   session's is open, a vim change through another view's state that
//!   opens its own (an insert session, `c` or `.`) for another origin
//!   nests into the session's (`fe_buffer` asserts against that in debug
//!   builds), so the two aren't undone apart. fe-vim can't see the open
//!   transaction's origin to refuse it.
//! - Another origin's undo or redo while a session is open closes the
//!   session's transaction (`fe_buffer` closes whichever transaction is
//!   open), so what's typed after it, and `Esc`'s cleanup, are undone
//!   apart from it.
//! - `.` and a count type the session's text again as the buffer holds it,
//!   not the keys: a line break in it brings the indentation it copied
//!   then, not the row's own; backspaces over text before the session's
//!   start aren't repeated; and another origin's text written among the
//!   typed characters is typed again with them, as the text `.` keeps is
//!   what lies between the session's start and the caret.
//! - `"{reg}.` ignores the register; a repeated change keeps its own.
//! - `r<CR>` cancels.
//! - A caret another origin's edit left inside a grapheme (or past a row's
//!   last one) is on that grapheme, as nvim's cursor is: `a`, `x`, `cw` and
//!   `p` act on all of it. `i` and `P` put text before it, where nvim,
//!   whose cursor only its API can leave there, puts it at that byte.
//! - Measuring text as the model catches up (`EditorModel`'s char widths)
//!   may stamp the context, or even end the session, through a clone of
//!   the state. A clone ending a session while another is ending it
//!   returns to normal mode, and adopts the session's outcome (its last
//!   change for `.` and its carets record) before its next `.`, `u` or
//!   `<C-r>`.

mod args;
mod cmdline;
mod commands;
mod edit;
mod ex;
mod headless;
mod history;
mod insert;
mod key;
mod motion;
mod operator;
mod reducer;
mod register;
mod repeat;
pub mod search;
mod state;
mod text_object;
mod visual;

pub use args::{
    CommandLineArgs, CommandLineKind, CountArgs, InsertArgs, InsertAt, Motion, MoveArgs,
    OperateArgs, Operator, PutArgs, RangeClass, ReplaceCharArgs, Search, SearchPattern, SelectArgs,
    SubmitArgs, SwapArgs, Target, TextObject, VisualArgs, VisualKind,
};
pub use commands::{VimCommands, VimTarget, registry};
pub use ex::{Address, Base, Ex, ExCommand, ExError, HostCommand, LineSearch};
pub use headless::{FeedError, HeadlessVim};
pub use insert::Splits;
pub use key::{KeyError, VimKey, parse_keys};
pub use reducer::{Pending, PendingKind, Step, step};
pub use register::{Register, RegisterError, Registers, Write};
pub use repeat::{Change, LastChange};
pub use state::{Mode, VimState, context};
pub use visual::Mark;

#[cfg(test)]
mod tests;
