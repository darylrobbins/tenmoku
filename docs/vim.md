# Vim mode

Tenmoku's vim mode is fe-vim (`crates/fe-vim`), an engine written from
Neovim's documented behaviour and checked against Neovim itself: no Vim or
Neovim code runs inside the editor. This page says what it supports, how to
set it up and where it differs from Neovim. Everything here comes from the
code and its tests; the file each part comes from is named beside it.

## Enabling

Vim mode is off by default. Turn it on with **View > Toggle Vim Mode** (the
command `vim.toggle`, also in the command palette; it has no default key),
which flips `[vim] enabled` and saves it to your `settings.toml`
(`crates/fe-gpui/src/vim.rs`, `crates/fe-keymap/menus/fastedit.toml`). The
first-run setup wizard asks the same thing as its "Keys" choice, Standard
or Vim, and then which maps vim starts with: plain vim or LazyVim's
(`crates/fe-workbench/src/setup_wizard_model.rs`).

With vim on, the status bar's left side shows the mode as a coloured pill:
NORMAL (also while an operator waits), INSERT, REPLACE, VISUAL, VISUAL
LINE, VISUAL BLOCK or CMDLINE, and JUMP while a jump waits for its chars or
labels (`crates/fe-gpui/src/pane_item.rs`). Beside it, `recording @a` shows
while a macro records, and vim's last message (`E486: Pattern not found`)
shows until the next key (`crates/fe-workbench/src/status_bar.rs`).

On the Start page, with vim on, LazyVim's dashboard keys run the rows, each
key's letter underlined in its label: `n` new file, `f` open file, `p` open
folder, `g` find text, `r` recent files, `s` settings, `q` quit; `j` and `k`
move as down and up do (`crates/fe-workbench/src/start_page.rs`,
`start_page_model.rs`).

## Settings

Every key of `[vim]` in `settings.toml` (`VimSettings` in
`crates/fe-config/src/settings.rs`), with its default. The settings
template, `crates/fe-config/settings-template.toml`, lists the same keys
commented out. `crates/fe-config/tests/vim_doc.rs` fails if this table
misses a key, names one that isn't there, or gives another default.

| Key | Values | Default | What it does |
| --- | --- | --- | --- |
| `enabled` | bool | `false` | Start editors in normal mode, where keys run vim commands; View > Toggle Vim Mode flips it and saves it here. |
| `clipboard` | `""` or `"unnamedplus"` | `""` | `"unnamedplus"` makes yanks, deletes and puts that name no register use the system clipboard, as nvim's `clipboard=unnamedplus`; empty, only `"+` and `"*` reach it. |
| `jump_on_s` | bool | `false` | `s` and `S` start the forward and backward jump, as leap and flash map them, instead of changing a char or a line (`cl` and `cc` still do); `gs` and `gS` start it either way. It also takes the `s` of surround's `ds`, `cs`, `ys` and visual `S`. |
| `jump_style` | `"leap"` or `"flash"` | `"leap"` | `"leap"`: the jump takes two chars, then labels the matches in view; `"flash"`: one char, then labels every match in view before and after the caret (`gS`: before only). |
| `ignorecase` | bool | `false` | `/`, `?`, `n` and `N` ignore case, as nvim's 'ignorecase'; `\c` and `\C` in a pattern override it. |
| `smartcase` | bool | `false` | With `ignorecase`, a typed pattern with an uppercase letter matches case (`*` and `#` still ignore it), as 'smartcase'. |
| `incsearch` | bool | `true` | A search shows its first match as the pattern is typed, as 'incsearch'. |
| `inccommand` | bool | `true` | A `:s` shows its matches and each changed row as it is typed, as 'inccommand' (`nosplit`); `Esc` puts everything back. |
| `replace_with_register` | bool | `false` | `gr{motion}`, `grr` and visual `gr` replace text with a register, as ReplaceWithRegister, instead of nvim 0.12's language-server `gr` keys (`grr`, `grn`, `gra`, `gri`, `grt`). |
| `subword_motions` | bool | `false` | `w`, `b`, `e` and `ge` stop at each part of a camelCase, PascalCase, snake_case or digit-joined name, as vim-wordmotion; `W B E gE` still move by WORDs. |
| `regex_time_limit` | milliseconds, at least 1 | `2000` | How long a search or `:s` may run on one command before it stops with Interrupted and changes nothing (nvim's 'redrawtime' default). |
| `shell` | string | `""` | The program `!` filters and `:r !` run commands with, as `shell -c command`; empty is `$SHELL`, else `/bin/sh` (`%COMSPEC%` on Windows). A project file can't set it. |
| `filter_time_limit` | milliseconds, at least 100 | `10000` | How long a `!` filter or `:r !` may run before it is killed and nothing changes. |
| `which_key` | bool | `true` | A popup lists the keys that go on from a pending prefix (`g`, `z`, `<leader>`, a remap's first keys), as which-key.nvim's does. |
| `which_key_delay_ms` | milliseconds, 0 to 5000 | `200` | How long a prefix waits for its next key before the which-key popup shows; 0 shows it at once. |
| `highlight_yank` | bool | `true` | A yank briefly highlights the text it took, as nvim's `vim.hl.on_yank`. |
| `highlight_yank_ms` | milliseconds, 1 to 5000 | `150` | How long the yank highlight stays. |
| `quick_scope` | `"off"`, `"on_keys"` or `"always"` | `"on_keys"` | Underlines the char of each word on the caret's row that `f` or `F` reaches in one or two presses, as quick-scope: `"always"` in normal and visual mode, `"on_keys"` only while `f`, `F`, `t` or `T` waits for its char. |
| `leader` | vim notation | `"\\"` | What `<leader>` stands for in a remap (`"<Space>"` works). Left at `"\\"`, a preset's leader applies. |
| `preset` | `"none"` or `"lazyvim"` | `"none"` | A bundled set of remaps loaded beneath the vimrc and `[[vim.remap]]`: plain vim, or LazyVim's with `<Space>` as the leader unless `leader` is set. |
| `remap` | `[[vim.remap]]` tables | `[]` | Key remaps, as `:map` makes them; see [Keymaps](#keymaps). |
| `which_key_group` | `[[vim.which_key_group]]` tables | `[]` | Names (and icons) for groups of remaps, shown in the which-key popup as `+{name}`; see [Keymaps](#keymaps). |

The search options and `clipboard` can also be changed for the session with
`:set` (below); that changes the editor, not `settings.toml`.

## Modes and editing

The keys below are fe-vim's own (`crates/fe-vim/src/reducer.rs` and the
feature modules `crates/fe-vim/src/feature.rs` lists). Each module's docs
give the details, and the tests in `crates/fe-vim/tests/` check them, most
against recorded Neovim.

**Modes.** Normal; insert (`i a I A o O gi`, and `c s S C`); replace (`R`)
and virtual replace (`gR`); visual charwise (`v`), linewise (`V`) and
blockwise (`<C-v>` or `<C-q>`, with `o`, `O`, block `I`, `A` and `$A`);
operator-pending (after `d`, `c`, `y` and the other operators); and the
command line (`:`, `/`, `?`). `Esc` or `<C-[>` returns to normal mode.
`<C-o>` in insert or replace mode runs one normal-mode command and comes
back. In insert mode `<C-t>`, `<C-d>`, `0<C-d>`, `<C-w>`, `<C-u>`,
`<C-r>{register}`, `<C-n>` and `<C-p>` (keyword completion from the buffer)
are vim's on every platform (`crates/fe-vim/src/insert/`,
`crates/fe-keymap/tests/vim_insert_keys.rs`).

**Motions.** `h j k l`, `w b e ge W B E gE`, `0 ^ $ g_ _ + - |`, `gg G`
(and `{count}%`), `f F t T ; ,`, `H M L`, `gj gk g$ gm`, `{ } ( )`, `%` and
`g%`, `[( [{ ]) ]}`, `]] [[ ][ []`, `]m [m ]M [M`, `'{mark}` and
`` `{mark} ``, `/ ? n N * # g* g#`, `zj zk`, and `<C-o> <C-i> <Tab>` through
the jumplist. A count goes before any of them.

**Operators.** `d c y > < =`, `gu gU g~ g?` (case and rot13), `gq gw`
(format), `gc` (comment), `!` (filter), `zf` (fold), `cx` (exchange), `ys`
(surround), and `gr` with `replace_with_register`. Doubled (`dd`, `gUU`,
`gcc`) each takes `count` lines. Also `x X D C s S Y`, `p P gp gP ]p [p`,
`J gJ`, `r{char}`, `~`, `<C-a>` `<C-x>` (and `g<C-a>` `g<C-x>` over a
selection), `]<Space>` `[<Space>` (blank lines) and `]e` `[e` (move a line).

**Text objects** (after an operator or in a visual mode, with `i` or `a`):
`w W` words, `S` subwords, `s p` sentences and paragraphs, `( ) b`, `{ } B`,
`[ ]`, `< >`, `t` tags, `"`, `'` and `` ` `` quotes, `a` arguments, `e` the whole
buffer, `i` and `I` indent levels, `f` and `c` functions and classes, `n`
and `l` before a bracket or quote for the next or last pair (`in(`, `al"`),
and `gc` a run of commented lines.

**Registers** (`crates/fe-vim/src/register.rs`): the unnamed `"`; named `a`
to `z`, with `A` to `Z` appending; numbered `0` (last yank) and `1` to `9`
(deletes); `-` (small deletes); `_` (black hole); `+` and `*`, which share
the system clipboard; and the read-only `/` (last search), `.` (last
inserted text) and `:` (last command line). `clipboard = "unnamedplus"`
makes `"` the clipboard. `:reg` lists them.

**Marks, jumplist and changelist** (`crates/fe-vim/src/marks.rs`,
`jumplist.rs`, `feature/changelist.rs`): `m{a-z}` and `m{A-Z}`, `''` and
`` `` `` (the jump mark), `'<` `'>`, `'[` `']` (last change, put or yank),
`'.` (last change) and `'^` (where insert mode was left). `<C-o>` and
`<C-i>` walk the jumplist, `g;` and `g,` the changelist (100 entries each).
`:marks`, `:delmarks`, `:jumps`, `:changes` and `:clearjumps` work on them.

**Macros, repeat and undo.** `q{a-z}` records (`q{A-Z}` appends), `q`
stops, `@{a-z}` plays, `@@` plays the last one, `@:` repeats the
last command line, and `Q` plays the last recorded macro
(`crates/fe-vim/src/macros.rs`, `crates/fe-vim/tests/repeat_command_line_oracle.rs`). `.` repeats the
last change, with a new count if given. `u` and `<C-r>` undo and redo,
`U` undoes the last changed line, and `g-` `g+` and `:earlier` `:later`
walk the history.

**Folds** (`crates/fe-vim/src/folds.rs`): `za zA zc zC zo zO zR zM zv zd
zD zE`, `zf{motion}`, `zF`, visual `zf`, and `zj zk`.

**Scrolling** (`crates/fe-vim/src/scroll.rs`): `<C-d> <C-u> <C-e> <C-y>
<C-f> <C-b>` and `zt zz zb z<CR> z. z-`.

**Other keys.** `gv` reselects, `ZZ` and `ZQ` save-and-close and close,
`]c` `[c` go to the next and previous change and `do` `dp` take or give a
diff hunk (`crates/fe-vim/src/changes.rs`), and `&` and `g&` repeat `:s`.

## Ex commands

An ex line is `[range] command[!] [args]`, read as nvim reads it
(`crates/fe-vim/src/ex.rs`). Several commands can share a line with `|`,
except after commands that take the rest of the line (`:g`, `:normal`, the
file, buffer, window and capitalised ones), and `"` starts a comment.

**Ranges.** Line specifiers separated by `,` or `;` (`;` moves the cursor
before the next is read): `N`, `.`, `$`, `%`, a mark's line (`'a`, `''`,
`'<`, `'>`), `/pat/` and `?pat?` searches, and `+N`/`-N` offsets. `:{range}`
alone goes to the range's last line, as `{N}G`. A count before `:` fills in
`.,.+{count-1}`, and `:` from a visual mode fills in `'<,'>`.

**Search and `:s`.** `/`, `?`, `n`, `N`, `*`, `#`, `g*` and `g#` search with
Vim's pattern syntax (magic, `\v`, `\m`, `\M`, `\V`, `\c`, `\C`), including
search offsets (`/foo/e+1`) and patterns that match across rows (`\n`,
`\_x`). `:s/{pattern}/{string}/[flags] [count]` takes the flags `& g i I n e
r c` (`p # l` are accepted and ignored), and `c` asks `y/n/a/q/l` for each
match. The replacement takes `& \0 \1`–`\9 ~ \r \t \u \l \U \L \E \e`.
`:s` with no pattern, `:&`, `:&&`, `:~`, normal-mode `&` and `g&` repeat the
last substitute. With `inccommand` on, the line previews its changes as it
is typed (`crates/fe-vim/src/lib.rs`, "Ex commands" and "Search patterns").
What the patterns can't do yet is under
[Differences from Neovim](#differences-from-neovim).

**`:g`.** `:g/{pattern}/[cmd]`, `:g!` and `:v` run a command on each
matching (or non-matching) line, as one undo step; the default command,
`:p`, only moves the cursor (`:p` alone is E492). A `:g` inside a `:g` is
E147, and buffer, window, tab and quickfix commands are refused there, as
in `:normal` (`a buffer command in :g or :normal: not supported yet`), as
are shell commands (`crates/fe-vim/src/ex/global.rs`,
`crates/fe-vim/src/ex.rs`).

The commands fe-vim runs itself. Brackets mark the optional part of a name;
`crates/fe-vim/tests/ex_unsupported_doc.rs` runs each one by its shortest and
full name and fails on E492.

| Commands | What they do |
| --- | --- |
| `:d[elete]` | Delete lines into a register, as `dd` (`:dl` and `:dp` too). |
| `:y[ank]` | Yank lines into a register. |
| `:j[oin]` | Join lines, as `J`, or as they are with `!`. |
| `:m[ove]` `:t` `:co[py]` | Move or copy lines below an address. |
| `:>` `:<` | Shift lines, repeated for more (`:>>>`). |
| `:sor[t]` | Sort lines; `!` reverses, flags `i n u r` and a `/pattern/`. |
| `:s[ubstitute]` `:&` `:&&` `:~` | Substitute, or repeat the last substitute. |
| `:g[lobal]` `:v[global]` | Run a command on matching lines. |
| `:norm[al]` | Run keys as typed on each line; `:normal!` ignores remaps. |
| `:noh[lsearch]` | Clear the search highlight until the next search. |
| `:pu[t]` | Put a register as whole lines below (`!`: above) a line. |
| `:se[t]` | Set `wrap`, `number`/`nu`, `relativenumber`/`rnu`, `cursorline`/`cul`, `scrolloff`/`so`, `tabstop`/`ts`, `shiftwidth`/`sw`, `expandtab`/`et`, `clipboard`/`cb`, `ignorecase`/`ic`, `smartcase`/`scs` or `incsearch`/`is` for this session (`crates/fe-vim/src/ex/set.rs`). |
| `:u[ndo]` `:red[o]` | Undo and redo, as `u` and `<C-r>`. |
| `:ea[rlier]` `:lat[er]` | Go back or forward through the undo history by changes, time or file writes. |
| `:marks` `:delm[arks]` | List marks in a picker; delete marks. |
| `:ju[mps]` `:changes` `:cle[arjumps]` | List the jumplist or changelist in a picker; clear the jumplist. |
| `:reg[isters]` `:di[splay]` | List registers in a picker; choosing one puts it. |
| `:w[rite]` `:sav[eas]` `:wa[ll]` | Save the file, save it as a path, save every modified file. |
| `:q[uit]` `:qa[ll]` `:quita[ll]` `:wq` `:x[it]` `:exi[t]` | Close the window or all of them, saving first for `:wq` and `:x`. |
| `:e[dit]` `:r[ead]` | Open or reload a file; put a file's lines below a line. |
| `:Ex[plore]` `:ol[dfiles]` | Reveal the file in the project tree; pick from recent files (`:browse oldfiles` too). |
| `:b[uffer]` `:bn[ext]` `:bN[ext]` `:bp[revious]` `:bd[elete]` `:bf[irst]` `:br[ewind]` `:bl[ast]` `:ls` `:buffers` `:files` | The open files as Vim's buffer list (`crates/fe-vim/src/ex/buffers.rs`). |
| `:sp[lit]` `:vs[plit]` `:new` `:vne[w]` `:on[ly]` `:clo[se]` | Split, close and keep panes (`crates/fe-vim/src/ex/windows.rs`). |
| `:tabnew` `:tabe[dit]` `:tabn[ext]` `:tabN[ext]` `:tabp[revious]` `:tabc[lose]` `:tabo[nly]` `:tabfir[st]` `:tabr[ewind]` `:tabl[ast]` | Tabs (`crates/fe-vim/src/ex/tabs.rs`). |
| `:cn[ext]` `:cp[revious]` `:cN[ext]` `:cnf[ile]` `:cpf[ile]` `:cNf[ile]` `:cc` `:cfir[st]` `:cr[ewind]` `:cla[st]` `:col[der]` `:cnew[er]` `:cl[ist]` | Step through the quickfix list (`crates/fe-vim/src/ex/quickfix.rs`). |
| `:lne[xt]` `:lp[revious]` `:lN[ext]` `:lnf[ile]` `:lpf[ile]` `:lNf[ile]` `:ll` `:lfir[st]` `:lr[ewind]` `:lla[st]` `:lol[der]` `:lnew[er]` `:lli[st]` | The same for the window's location list. |
| `:gr[ep]` `:lgr[ep]` `:vim[grep]` `:lv[imgrep]` `:mak[e]` `:lmak[e]` | Fill the quickfix or location list. |
| `:cope[n]` `:cw[indow]` `:ccl[ose]` `:lop[en]` `:lw[indow]` `:lcl[ose]` | Open and close the list's window. |
| `:diffg[et]` `:diffpu[t]` `:diffs[plit]` | Take or give a diff hunk; diff against a file in a split. |
| `:Replace` | Replace the project search's matches with the text in every file. |

Two more forms have no name: `:{range}!{cmd}` filters lines through a shell
command and `:[line]r !{cmd}` puts a command's output below a line. Only
your own keys run them, not `:g` or `:normal`
(`crates/fe-vim/src/ex/filter.rs`). The file, buffer, window, tab, list and
diff commands are carried out by the workbench, not fe-vim. Any other
command that starts with a capital letter (`:Gitsigns stage_hunk`) goes to
the workbench as a user command, which answers E492 for one it doesn't
know.

### Unsupported ex commands

fe-vim has no Vim script, options beyond those `:set` takes, help, syntax
or autocommands. These are some common commands it doesn't run: each gives
`E492: Not an editor command`, or the refusal quoted beside it, and changes
nothing. `crates/fe-vim/tests/ex_unsupported_doc.rs` runs each one.

- `abbreviate`
- `autocmd`
- `augroup`
- `call`
- `cd`
- `center`
- `colorscheme`
- `command`
- `digraphs`
- `echo`
- `execute`
- `file`
- `filetype`
- `find`
- `function`
- `help`
- `highlight`
- `history`
- `let`
- `lua`
- `map`
- `nnoremap`
- `mark`
- `messages`
- `mksession`
- `next`
- `packadd`
- `print`
- `pwd`
- `resize`
- `retab`
- `silent`
- `source`
- `startinsert`
- `syntax`
- `tag`
- `terminal`
- `undojoin`
- `undolist`
- `update`
- `vertical`
- `windo`
- `bufdo`
- `wincmd`
- `!ls` — refused: `:!{cmd} without a range isn't run here`
- `set all` — refused: `:set all: not supported`
- `set foldmethod=indent` — refused: `foldmethod: not supported yet`
- `set clipboard=unnamed` — refused: `clipboard=unnamed: not supported`
- `sort x` — refused: `:sort x: not supported yet`
- `undo 3` — refused: `:undo 3: not supported`
- `put =` — refused: `the = register: not supported`
- `on 2` — refused: `a window number: not supported yet`
- `sp +3 file` — refused: `+cmd and ++opt: not supported yet`
- `ls u` — refused: `:ls flags: not supported yet`

Maps are made in the vimrc or `settings.toml`, not on the command line
([Keymaps](#keymaps)); `:map` and its family there are E492.

## Emulated plugins

Neovim users get much of their editing from plugins. fe-vim builds in the
behaviour of the ones below, each in its own module, with no plugin code
and no plugin manager. Paths are under `crates/fe-vim/`.

| Plugin behaviour | Keys | Setting | Module | Tests |
| --- | --- | --- | --- | --- |
| vim-surround | `ds{char}`, `cs{old}{new}`, `ys{motion}{new}`, `yss`, `yS`, `ySS`, visual `S` | none, but `jump_on_s` turns these `s` and `S` keys into the jump (`src/jump.rs`) | `src/feature/surround.rs` | `tests/surround.rs`, `tests/surround_add.rs` |
| Commenting (nvim 0.12's built-in, as vim-commentary) | `gc{motion}`, `gcc`, visual `gc`, object `gc` | none | `src/feature/comment.rs` | `tests/commentary_oracle.rs` |
| ReplaceWithRegister | `["x]gr{motion}`, `grr`, visual `gr` | `replace_with_register` | `src/feature/replace_register.rs` | `tests/replace_with_register.rs` |
| vim-exchange | `cx{motion}` marks, a second `cx` swaps; `cxx` for lines; `cxc` clears (no visual `X`, which stays vim's) | none | `src/feature/exchange.rs` | `tests/exchange.rs` |
| vim-abolish (case coercion only) | `crs` `cr_` snake, `crm` `crp` Mixed, `crc` camel, `cru` `crU` UPPER, `cr-` `crk` dash, `cr.` dot, `cr<Space>` space, `crt` Title | none | `src/feature/abolish.rs` | `tests/abolish.rs` |
| Argument objects (as VSCodeVim's) | `ia`, `aa` | none | `src/feature/argument.rs` | `tests/argument_object.rs` |
| vim-wordmotion subwords | `w b e ge` by subword; `iS`, `aS` either way | `subword_motions` | `src/feature/subword.rs` | `tests/subword_motions.rs`, `tests/subword_objects.rs` |
| matchit | `%` and `g%` through `if`/`else`/`end`-style groups in Lua, Bash and Make, and through tags in HTML, XML and Markdown | none | `src/feature/matchit.rs`, `src/feature/matchit/tags.rs` | `tests/matchit.rs`, `tests/matchit_tags.rs` |
| quick-scope | underlines `f`/`F` targets on the caret's row | `quick_scope` | `src/quick_scope.rs` | `tests/quick_scope.rs` |
| leap / flash jump | `gs`, `gS` (and `s`, `S` with `jump_on_s`), then chars and a label; `<Space>` and `<BS>` page labels; `<CR>` repeats the last jump | `jump_on_s`, `jump_style` | `src/jump.rs`, `src/jump/label.rs` | `tests/jump_spec.rs`, `tests/jump_flash.rs`, `tests/jump_oracle.rs`, `tests/jump_fold.rs` |
| Sections | `]]`, `[[`, `][`, `[]` | none | `src/feature/sections.rs` | `tests/sections_oracle.rs` |
| Function motions (nvim-treesitter-textobjects) | `]m`, `[m`, `]M`, `[M` | none | `src/feature/definition_motions.rs` | `tests/definition_motions.rs` |
| Function and class objects (nvim-treesitter-textobjects) | `if`, `af`, `ic`, `ac` | none | `src/feature/definition.rs` | `tests/definition_objects.rs` |
| Next and last pair objects (as targets.vim's and mini.ai's `n` and `l`) | `in(`, `an(`, `il(`, `al(`, and the same with `[`, `{`, `<`, `"`, `'` or `` ` `` | none | `src/feature/seek.rs` | `tests/seek_objects.rs` |
| textobj-entire and vim-indent-object | `ie`, `ae`; `ii`, `ai`, `aI` | none | `src/feature/objects.rs` | `tests/entire_and_indent_objects.rs` |
| vim-unimpaired (two pairs) | `]e`, `[e` move a line; `]<Space>`, `[<Space>` add blank lines | none | `src/feature/line_edits.rs` | `tests/line_edits.rs`, `tests/line_edits_oracle.rs` |
| which-key | a popup after `g`, `z`, `Z`, `[`, `]`, `<leader>` or a remap's first keys | `which_key`, `which_key_delay_ms`, `[[vim.which_key_group]]` | `src/which_key.rs` | `tests/which_key.rs`, `tests/which_key_groups.rs`, `tests/which_key_order.rs` |

The function motions and objects need the buffer's language to have
syntax; without it they do nothing. The jump labels only what is in view,
and a jump with a count goes to the `count`th match with no labels.

**Not implemented:** easymotion, vim-sneak, targets.vim beyond the `n` and
`l` pair objects and the argument object, mini.ai's other objects,
vim-abolish's `:Abolish` and `:Subvert`, the rest of vim-unimpaired (its
`]b`, `[b`, `]q` and `[q` are workbench keys, below), and any Vim or Lua
plugin: there is no `:packadd`, `:source` or `:lua`.

## Keymaps

Vim key remaps come from three places, loaded in this order, a later map for
the same keys in the same mode replacing an earlier one
(`crates/fastedit/src/vim_remaps.rs`): the preset, your vimrc, and
`[[vim.remap]]` in `settings.toml`. All three hot-reload. One engine runs
them in front of vim's own keys (`crates/fe-vim/src/remap.rs`; the design is
`docs/decisions/vim-remaps.md`).

### `[[vim.remap]]`

```toml
[[vim.remap]]
mode = "n"          # "n", "x" or "v", "o", "i", "c", or a list of them
from = "<leader>w"  # vim notation
to = ":w<CR>"
recursive = false   # true: the keys `to` gives are remapped too, as :map
```

The modes (`VimRemapMode` in `crates/fe-config/src/settings.rs`) are `n`
normal, `x` visual (`v` is the same: there is no select mode), `o`
operator-pending (so `dH` needs an `o` map for `H`), `i` insert and replace,
and `c` the command line. A recursive chain stops with `E223: recursive
mapping` after 100 maps in a row, where nvim's 'maxmapdepth' is 1000
(`crates/fe-vim/src/remap.rs`). When typed keys could still become a longer
map, they wait for the next key, or for the editor's chord timeout (one
second, `kiln_platform::keys::CHORD_TIMEOUT`) with none
(`crates/fe-gpui/src/vim.rs`). A right side can hold `<Cmd>{ex line}<CR>`,
run without opening the command line, and `<Cmd>fe:{command id}<CR>`, which
runs a Tenmoku command as a key binding would
(`<Cmd>fe:workbench.save<CR>`; `crates/fe-vim/src/remap.rs`).

### The vimrc

Your vimrc is `vimrc` beside `settings.toml`, in Tenmoku's config folder:
`$TENMOKU_CONFIG_DIR`, else `$XDG_CONFIG_HOME/tenmoku`, else
`~/.config/tenmoku`, on every platform (`crates/fe-config/src/paths.rs`).
**Open Config File…** lists it. It reads a subset of Vim's
(`crates/fe-vim/src/vimrc.rs`):

- `map`, `noremap`, `unmap` and `mapclear`, and their `n`, `x`, `o`, `i`
  and `c` forms (`nnoremap`, `xmap`, `iunmap`, `cmapclear`…), plus `map!`,
  `noremap!`, `unmap!` and `mapclear!` for insert and command-line mode.
  `vmap` and `vnoremap` map visual mode only, with a note.
- `<silent>` and `<nowait>` before the keys, which are ignored.
- `let mapleader = "…"`.
- `"` comments and `|` between commands.
- `:fegroup {keys} {name} [icon={icon}]`, Tenmoku's own, naming a group of
  normal-mode maps for the which-key popup.

Anything else is reported and never run: select (`smap`), language
(`lmap`) and terminal (`tmap`) maps; `<buffer>`, `<expr>`, `<script>`,
`<unique>` and `<special>`; any other `let`; and any other command. Each
problem is a warning at its line in the config problems list, which the
status bar's "N config problems" item opens; so is a map the keymap hides
(below) (`crates/fastedit/src/vim_remaps.rs`).

### Leader and preset

`<leader>` is `\` unless `leader` or `let mapleader` sets it. `preset =
"lazyvim"` loads `crates/fastedit/presets/lazyvim.vimrc` beneath your maps,
with `<Space>` as the leader while `leader` is left at `"\\"`
(`crates/fastedit/src/vim_remaps.rs`). It maps LazyVim's keys to Tenmoku's
commands through `<Cmd>fe:…<CR>`: `<leader>ff` and `<leader><space>` quick
open, `<leader>fr` recent files, `<leader>/` and `<leader>sg` search,
`<leader>e` the project tree, `<leader>bd` close tab, `<leader>ca` code
actions, `<leader>cr` rename, `<leader>gg` git changes, `<leader>xx`
problems, `<leader>-` and `<leader>|` splits, `<C-h>` `<C-j>` `<C-k>`
`<C-l>` between panes, `<leader>qq` quit, and more; the file's end lists
the LazyVim keys it leaves out.

### Which-key groups

```toml
[[vim.which_key_group]]
keys = "<leader>f"
name = "file/find"
icon = "search"     # a Lucide icon name; optional
mode = "n"          # as vim.remap's; "n" unless set
```

The popup shows the group as `+file/find` in place of `+{count}`. Names
load after the preset's and the vimrc's `:fegroup`s, and a later name for
the same keys wins (`crates/fe-config/src/settings.rs`). The popup is drawn
by `crates/fe-gpui/src/vim/which_key.rs` from `crates/fe-vim/src/which_key.rs`;
it lists nothing while an operator waits (`d`, `y`, `c`).

### Vim keys and the editor's keymap

The editor's keymap, `crates/fe-keymap/keymaps/fastedit.toml`, is matched
first, in every mode, unless a vim command or a remap prefix is waiting;
only keys it doesn't take reach vim (`crates/fe-gpui/src/vim.rs`,
`crates/fe-gpui/src/keymap.rs`, `docs/decisions/vim-engine.md` §10). So:

- `cmd-` shortcuts on macOS work in every mode, and off macOS the `ctrl-`
  keys vim uses in normal and visual mode (`ctrl-v`, `ctrl-a`, `ctrl-x`,
  `ctrl-f`, `ctrl-b` and the rest) are bound to `vim.key`, which hands them
  to vim (the vim layer at the end of `fastedit.toml`).
- In insert and replace mode the editor's text keys stay, but nvim's
  insert-mode `ctrl-w`, `ctrl-u`, `ctrl-r`, `ctrl-t`, `ctrl-d`, `ctrl-o`,
  `ctrl-n`, `ctrl-p` and `ctrl-a` reach vim: off macOS they are bound to
  `vim.key` there, after (so over) the workbench's close tab, open file,
  new file, quick open and select all; on macOS no `ctrl-` binding applies
  in the editor (`fastedit.toml`, insert-mode section;
  `crates/fe-keymap/tests/vim_insert_keys.rs`).
- The editor's multi-cursor keys give way to vim's, but in normal mode,
  with nothing pending, `gb` adds a caret at the word's next occurrence (as
  VSCodeVim's), as do `cmd-d` on macOS, with `cmd-shift-l` for every
  occurrence; `cmd-alt-up` and `cmd-alt-down` on macOS, `alt-shift-up` and
  `alt-shift-down` elsewhere, add one above or below. Vim's commands act at
  each caret, and `Esc` with nothing pending goes back to one
  (`fastedit.toml`; `crates/fe-keymap/tests/vim_multi_cursor_keys.rs`).
- In normal mode `K` shows the hover, and `grr`, `grn`, `gra`, `gri` and
  `grt` are nvim 0.12's language-server keys unless `replace_with_register`
  is on (`fastedit.toml`).
- `]b` `[b` step through tabs and `]q` `[q` through the quickfix list, as
  nvim 0.11's defaults, as keymap bindings in normal mode
  (`crates/fe-keymap/tests/vim_bracket_pairs.rs`).
- Bindings can be scoped to vim with the contexts `vim.enabled`,
  `vim.mode` (`normal`, `insert`, `replace`, `visual`, `visual_line`,
  `visual_block`, `operator_pending`, `cmdline`), `vim.pending` and
  `vim.pending_kind` (`crates/fe-vim/src/lib.rs`, `crates/fe-vim/src/state.rs`).

An insert or command-line map whose keys the keymap binds in that mode
never runs; the config problems list names it
(`crates/fe-gpui/src/keymap_remap_conflicts.rs`).

## Differences from Neovim

### How fe-vim is checked

fe-vim is written from Neovim's documented and observed behaviour, never
from Vim's or Neovim's source (`CLEAN_ROOM.md`). Its oracle is Neovim
**0.12.4**: the corpus under `crates/fe-vim/tests/corpus/` holds hand-written
cases (a buffer, a cursor and keys, with optional maps, `:set` options,
folds and window size), and each `*.expected.json` holds what
`nvim --clean --headless` did with them, stamped `"nvim": "0.12.4"`. The
`*_oracle.rs` tests compare fe-vim with that recording, byte for byte: the
buffer text, the cursor's row and byte column, the mode, every non-empty
register among `" 0-9 - a-z /` (text, linewise or blockwise), the visual
anchor, the marks `a-z`, `'<` and `'>`, and the window's top row where a
case sets its size. Messages are checked by separate tests, not the
recording. The gate needs no Neovim; re-recording is the ignored test
`record_corpus_with_neovim`, which refuses any version but `NVIM v0.12.4`
(`crates/fe-vim/tests/support/mod.rs`, `crates/fe-vim/tests/oracle.rs`).
The character classes, display widths and case tables are recorded from
Neovim the same way (`crates/fe-vim/src/chars.txt`, `case.txt`,
`lower.txt`, `upper.txt`). The jump has no Neovim equivalent; its cases
in `corpus/jump_labels.toml` are checked by `tests/jump_spec.rs`.

### Search patterns

Patterns are translated to the Rust `regex` crate and matched a row at a
time; those Rust's engine can't match as Vim's backtracking engine would
(a repeated group whose branches end differently) run on fe-vim's own
backtracking matcher (`crates/fe-vim/src/search.rs`,
`crates/fe-vim/src/search/backtrack.rs`,
`docs/decisions/4mx0rf2akm-vim-search-priority.md`). One search or `:s`
may run for `regex_time_limit` (2 seconds by default, nvim's 'redrawtime');
past that it stops with `Interrupted` and changes nothing
(`crates/fe-vim/src/search.rs`). Lookarounds (`\@=` and the rest), position
atoms such as `\%V` and `\%23l`, and back-references are refused with
"pattern not supported", never matched differently
(`crates/fe-vim/src/search.rs`). Chained searches (`/foo/;/bar`) are
refused as an unsupported offset (`crates/fe-vim/src/lib.rs`).

### Deliberate differences

- **Remaps.** A recursive map stops after 100 levels, not 1000; the
  editor's one-second chord timeout stands in for 'timeoutlen'; there is no
  select mode; the keymap beats a map for the same keys; `<Cmd>fe:…<CR>` is
  Tenmoku's own (`crates/fe-vim/src/remap.rs`,
  `docs/decisions/vim-remaps.md` §2–§6). Maps live in the vimrc and
  `settings.toml`, not on the command line.
- **Marks.** Uppercase marks are kept per editor view; a mark in another
  file is the workbench's. There are no numbered marks (`'0`–`'9`) and no
  `'"` (`crates/fe-vim/src/marks.rs`). Undo doesn't put back a mark its
  change deleted or moved (`crates/fe-vim/src/lib.rs`, "Known limits").
  Marks on rows `:m` moves stay behind on their rows (`crates/fe-vim/src/ex.rs`).
- **Undo.** The history is linear, not Neovim's tree, so `:earlier`,
  `:later`, `g-` and `g+` can't reach an abandoned branch, and `:undo {N}`
  is refused (`crates/fe-vim/src/ex/earlier.rs`,
  `crates/fe-vim/src/ex/undo.rs`, `crates/fe-vim/src/g_operators.rs`). A
  change that changes no text leaves no undo step (`crates/fe-vim/src/lib.rs`).
- **Macros.** `q` starts recording only in normal mode with nothing
  pending, `@` plays only from normal mode, and only `a`–`z`, `@` and `:`
  play (`crates/fe-vim/src/reducer.rs`). A macro's changes are undone one at
  a time, not as one (`crates/fe-vim/src/feature/changelist.rs`). A replay
  stops after 1,000,000 keys in place of `<C-c>` (`crates/fe-vim/src/macros.rs`).
- **Folds** are Tenmoku's, from syntax or indentation, not
  `foldmethod=indent`: a fold starts at its header row, exists only while
  closed, and `zj` `zk` ignore nesting (`crates/fe-vim/src/folds.rs`).
- **Registers.** `"+` and `"*` are one register, the system clipboard
  (`crates/fe-vim/src/register.rs`); `clipboard` takes `unnamedplus` or
  nothing, not `unnamed` (`crates/fe-vim/src/ex/set.rs`). Command-line
  history isn't kept between runs: there is no ShaDa (`crates/fe-vim/src/lib.rs`).
- **`:set`** knows only the twelve options in [Ex commands](#ex-commands),
  changes the editor rather than `settings.toml`, and refuses `:set` alone,
  `:set all`, `opt&`, `+=`, `-=` and `^=` (`crates/fe-vim/src/ex/set.rs`).
- **Insert mode.** `<CR>` and `<BS>` are the editor's: a line break copies
  the row's indentation as it is, a backspace in an indent takes one blank,
  and unused indentation is dropped on `Esc` as 'autoindent' does
  (`crates/fe-vim/src/lib.rs`). `<C-n>` and `<C-p>` complete from the
  current buffer only (`crates/fe-vim/src/insert/complete.rs`).
- **Visual `*` and `#`** search the keyword at the cursor, not the
  selection (`crates/fe-vim/src/lib.rs`, "The command line").
- **Commenting** uses the editor's comment toggle and the language's
  tokens, not 'commentstring', so blank rows, mixed indentation and
  block-comment-only languages differ (`crates/fe-vim/src/feature/comment.rs`).
- **Shell commands** run only from your own keys: under `:g` or `:normal`
  a filter gets `Shell commands run only from your own keys`, a macro's
  replay runs none where Neovim's does, and `:!{cmd}` with no range is
  refused (`crates/fe-vim/src/ex/filter.rs`,
  `crates/fe-vim/src/feature/filter.rs`).
- **Workbench commands.** `:w`, `:e`, `:q`, the buffer, window, tab and
  quickfix commands are read as Neovim reads them but carried out by the
  workbench over its tabs and panes; window numbers, `+cmd` and `++opt`
  are refused (`crates/fe-vim/src/ex/windows.rs`, `ex/buffers.rs`,
  `ex/tabs.rs`, `ex/quickfix.rs`).

- **`:normal`** reads its keys in vim notation, so `<Esc>` there is the
  Escape key, where nvim's reads them as typed, five characters
  (`crates/fe-vim/src/ex/normal.rs`).
- **`=`** re-indents with the language's indent rules; a row they have no
  suggestion for keeps its indentation, as there is no C-indenting fallback
  (`crates/fe-vim/src/feature/reindent.rs`).
- **Sentence objects.** In a visual mode `is` and `as` grow differently from
  nvim's on empty rows, one-char sentences, punctuation and blanks, and
  they leave the jump mark alone (`crates/fe-vim/src/prose.rs`).
- **Scrolling.** `zt`, `zz` and `zb` with a count in a visual mode put the
  cursor at its goal column, where nvim's keep its byte column
  (`crates/fe-vim/src/scroll.rs`).
- **Fold motions.** `dzj` is always charwise and exclusive, where nvim's
  takes the class of the motion typed before it (`crates/fe-vim/src/folds.rs`).
- **Changelist.** Each change of a macro is an entry of its own, and `:s`
  leaves `'.` at its first match, where nvim's is at the row's column 0
  (`crates/fe-vim/src/feature/changelist.rs`).
- **Macros.** A register holding `<BS>`, `<Left>`, `<Right>`, `<Up>` or
  `<Down>` as text plays it as that key; `"ap` of a macro with `<CR>`
  breaks the line where nvim puts a `^M`; a macro goes on after `.` repeats
  a put, where nvim drops the rest (`crates/fe-vim/src/lib.rs`, "Known
  limits").
- **Visual and block repeats.** A count before `.` after a visual change
  replaces the operator's count rather than the amount of text; a
  blockwise register is put once, at the primary caret; a block yank or
  delete takes at most 64 MiB (`crates/fe-vim/src/lib.rs`, "Known limits").
- **Shell expansion.** `%` and `#` in a filter's command are the file names,
  each shell-escaped, where nvim's `%` isn't; a `!` in the command stays
  literal rather than the last command (`crates/fe-vim/src/ex/filter.rs`).
- **Plugins.** Surround's `yS` and `ySS` indent the wrapped rows one
  'shiftwidth' rather than by the buffer's indent rules
  (`crates/fe-vim/src/feature/surround.rs`). Abolish's `cr` writes no
  register, where the plugin leaves the old word in `"0`, `"-` and `""`
  (`crates/fe-vim/src/feature/abolish.rs`). Exchange doesn't highlight the
  marked region, works at the primary caret only, and reads the keys after
  `c` as operator-pending, so an `omap` of `x` applies
  (`crates/fe-vim/src/feature/exchange.rs`). ReplaceWithRegister has no
  `"=` register, takes no blockwise motion and replaces at the primary
  caret only (`crates/fe-vim/src/feature/replace_register.rs`). `]e` and
  `[e` in a visual mode keep the selection, as mini.move's, where
  unimpaired leaves visual mode, and move as far as they can near the
  buffer's edge rather than fail with E16
  (`crates/fe-vim/src/feature/line_edits.rs`). matchit compares tag names as
  plain text, so `<div>` doesn't count `<divx>`, and a self-closing tag
  opens nothing (`crates/fe-vim/src/feature/matchit/tags.rs`).
