use fe_vim::{Remap, RemapMode, Vimrc, VimrcIssue, parse_vimrc};

const N: RemapMode = RemapMode::Normal;
const X: RemapMode = RemapMode::Visual;
const O: RemapMode = RemapMode::OperatorPending;
const I: RemapMode = RemapMode::Insert;
const C: RemapMode = RemapMode::CmdLine;

fn remap(modes: &[RemapMode], from: &str, to: &str, recursive: bool) -> Remap {
    Remap {
        modes: modes.to_vec(),
        from: from.into(),
        to: to.into(),
        recursive,
    }
}

fn issue(line: usize, message: &str) -> VimrcIssue {
    VimrcIssue {
        line,
        message: message.into(),
    }
}

fn lines(vimrc: &Vimrc) -> Vec<usize> {
    vimrc.issues.iter().map(|i| i.line).collect()
}

#[test]
fn vimrc_reads_map_commands() {
    let text = "\
\" comment
nmap a 1
nnoremap b 2
xmap c 3
xnoremap d 4
omap e 5
onoremap f 6
map g 7
noremap h 8

   \" indented comment
:nm i 9
::nn j 10
 : xm k 11
xn l 12
om m 13
ono n 14
no o 15
nnor p 16
nmap <silent> q 17
nnoremap <nowait><silent>r 18
nnoremap s 19 \" not a comment
nnoremap t u\x20\x20
nnoremap<silent> v w
";
    let vimrc = parse_vimrc(text);
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "a", "1", true),
            remap(&[N], "b", "2", false),
            remap(&[X], "c", "3", true),
            remap(&[X], "d", "4", false),
            remap(&[O], "e", "5", true),
            remap(&[O], "f", "6", false),
            remap(&[N, X, O], "g", "7", true),
            remap(&[N, X, O], "h", "8", false),
            remap(&[N], "i", "9", true),
            remap(&[N], "j", "10", false),
            remap(&[X], "k", "11", true),
            remap(&[X], "l", "12", false),
            remap(&[O], "m", "13", true),
            remap(&[O], "n", "14", false),
            remap(&[N, X, O], "o", "15", false),
            remap(&[N], "p", "16", false),
            remap(&[N], "q", "17", true),
            remap(&[N], "r", "18", false),
            remap(&[N], "s", "19 \" not a comment", false),
            remap(&[N], "t", "u  ", false),
            remap(&[N], "v", "w", false),
        ]
    );

    // `vmap` and `vnoremap` load for Visual mode, with a note.
    let vimrc = parse_vimrc("vmap a b\nvn c d\nvnoremap e f\nvm g h");
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[X], "a", "b", true),
            remap(&[X], "c", "d", false),
            remap(&[X], "e", "f", false),
            remap(&[X], "g", "h", true),
        ]
    );
    assert_eq!(lines(&vimrc), vec![1, 2, 3, 4]);
    assert_eq!(
        vimrc.issues[0].message,
        "vmap applies to Visual mode only: Select mode isn't in fastedit"
    );

    // Windows line endings, and a key-notation map.
    let vimrc = parse_vimrc("nnoremap <leader>w :w<CR>\r\nnnoremap Y y$\r\n");
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "\\w", ":w<CR>", false),
            remap(&[N], "Y", "y$", false),
        ]
    );
}

#[test]
fn vimrc_unmap_and_leader() {
    let text = "\
map a 1
nmap b 2
xmap b 3
omap c 4
nnoremap d 5
unmap a
nunmap b
ounmap c
xunmap d
nun d
";
    let vimrc = parse_vimrc(text);
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(vimrc.remaps, vec![remap(&[X], "b", "3", true)]);

    let vimrc = parse_vimrc("vmap a 1\nmap a 2\nvunmap a\nvu b\nunm a \nmap a 3");
    assert_eq!(lines(&vimrc), vec![1]);
    assert_eq!(vimrc.remaps, vec![remap(&[N, X, O], "a", "3", true)]);

    // `unmap` matches keys, not spellings, and only earlier lines.
    let vimrc = parse_vimrc("nmap <Space>a 1\nnunmap <space>a\nnunmap x\nnmap x 2");
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(vimrc.remaps, vec![remap(&[N], "x", "2", true)]);

    let vimrc = parse_vimrc(
        "map a 1\nnmap b 2\nxmap c 3\nnmapclear\nomapc\nnmap d 4\nmapclear\nnmap e 5\nxmap f 6\nvmapclear",
    );
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(vimrc.remaps, vec![remap(&[N], "e", "5", true)]);
    let vimrc = parse_vimrc("map a 1\nxmapclear");
    assert_eq!(vimrc.remaps, vec![remap(&[N, O], "a", "1", true)]);

    // The leader is read when a line is, so it applies only to the lines
    // after the `let`.
    let text = "\
nnoremap <leader>a 1
let mapleader = \",\"
nnoremap <Leader>b <leader>
let g:mapleader=' '
nnoremap <LEADER>c 3
nunmap <leader>c
nnoremap <leader>c 4
let mapleader = \"\\<Space>\"
nnoremap <leader>d 5
let mapleader = '<'
nnoremap <leader>e 6
nnoremap <<leader>f 7
let mapleader = \"\"
nnoremap <leader>g 8
let mapleader = \"\\\\\" \" a comment
nnoremap <leader>h 9
";
    let vimrc = parse_vimrc(text);
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "\\a", "1", false),
            remap(&[N], ",b", ",", false),
            remap(&[N], "<Space>c", "4", false),
            remap(&[N], "<Space>d", "5", false),
            remap(&[N], "<lt>e", "6", false),
            remap(&[N], "<lt><lt>f", "7", false),
            remap(&[N], "\\g", "8", false),
            remap(&[N], "\\h", "9", false),
        ]
    );

    // `|` ends a command, unless it is `\|` or `<Bar>`; a trailing blank
    // before it stays in the right side, as nvim keeps it.
    let text = "\
nnoremap a b|nnoremap c d
nnoremap e f | nunmap a
nnoremap g h\\|i
nnoremap j k<Bar>l
let mapleader = ','|nnoremap <leader>m n
nnoremap o p|\" a comment
";
    let vimrc = parse_vimrc(text);
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "c", "d", false),
            remap(&[N], "e", "f ", false),
            remap(&[N], "g", "h|i", false),
            remap(&[N], "j", "k<Bar>l", false),
            remap(&[N], ",m", "n", false),
            remap(&[N], "o", "p", false),
        ]
    );

    // The mapclear family takes a trailing comment, and a `|` inside it
    // doesn't start a command.
    let text = "\
map a 1
mapclear \" comment | nmap b 2
nmap c 3
xmap d 4
nmapclear \"| nmap e 5
omap f 6
omapc|\" comment
vmapclear\" comment
mapclear! \" insert and command line
";
    let vimrc = parse_vimrc(text);
    assert_eq!(lines(&vimrc), Vec::<usize>::new());
    assert_eq!(vimrc.remaps, vec![]);

    // A double-quoted leader is the string's value: escapes are decoded
    // and the keys canonical, so equal values unmap each other.
    let text = "\
let mapleader = \"\\x20\"
nnoremap <leader>a 1
let mapleader = \"\\<Space>\"
nunmap <leader>a
let mapleader = \"\\<C-M>\"
nnoremap <leader>b 2
let mapleader = \"\\r\"
nunmap <leader>b
let mapleader = \"\\<CR>\"
nnoremap <leader>c 3
let mapleader = \"\\u002c\\054\\x2C\"
nnoremap <leader>d 4
let mapleader = \"\\b\\f\\e\\t\\x\\q\"
nnoremap <leader>e 5
let mapleader = \"\\<foo>\"
nnoremap <leader>f 6
let mapleader = \"\\<Leader>\"
nnoremap <leader>g 7
let mapleader = \"\\<lt>\\<Bar>\\<Bslash>\\<NL>\"
nnoremap <leader>h 8
let mapleader = '\\<Space>'
nnoremap <leader>i 9
let mapleader = \"a\\x00b\"
nnoremap <leader>j 10
";
    let vimrc = parse_vimrc(text);
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "<CR>c", "3", false),
            remap(&[N], ",,,d", "4", false),
            remap(&[N], "<C-h><C-l><Esc><Tab>xqe", "5", false),
            remap(&[N], "<lt>foo>f", "6", false),
            remap(&[N], "<lt>Leader>g", "7", false),
            remap(&[N], "<lt><Bar>\\<C-j>h", "8", false),
            remap(&[N], "\\<lt>Space>i", "9", false),
            remap(&[N], "aj", "10", false),
        ]
    );

    // Numeric escapes are bytes, read as UTF-8, so each of these leaders is
    // `é` and unmaps the others' maps. `\b` and `\<C-H>` are CTRL-H, not
    // `<BS>`. A NUL ends the value, and what follows it isn't checked.
    let text = "\
let mapleader = \"\\xC3\\xA9\"
nnoremap <leader>a 1
let mapleader = 'é'
nunmap <leader>a
let mapleader = \"\\303\\251\"
nnoremap <leader>b 2
let mapleader = \"\\u00e9\"
nunmap <leader>b
let mapleader = \"\\b\"
nnoremap <leader>c 3
let mapleader = \"\\<BS>\"
nunmap <leader>c
nnoremap <leader>d 4
let mapleader = \"\\<C-H>\"
nunmap <leader>d
let mapleader = \"a\\x00\\<F2>b\"
nnoremap <leader>e 5
let mapleader = \"\\400b\"
nnoremap <leader>f 6
let mapleader = \"\\<Escape>\\<Backspace>\\<NewLine>\\<C-[>\\<S-a>\\<t_ku>\\<char-0x41>\"
nnoremap <leader>g 7
let mapleader = \"\\<C-foo>\\<F999>\\<Nop>\\<SID>\"
nnoremap <leader>h 8
";
    let vimrc = parse_vimrc(text);
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "<C-h>c", "3", false),
            remap(&[N], "<BS>d", "4", false),
            remap(&[N], "ae", "5", false),
            remap(&[N], "\\f", "6", false),
            remap(&[N], "<Esc><BS><C-j><Esc>A<Up>Ag", "7", false),
            remap(&[N], "<lt>C-foo><lt>F999><lt>Nop><lt>SID>h", "8", false),
        ]
    );

    // nvim's `2-`, `3-` and `4-` click modifiers make keys fe-vim can't
    // type, and an unknown name after one is too likely a typo to read as
    // text: each is an issue and the leader stays `,`. `5-` isn't a
    // modifier, so `<5-a>` is text.
    let text = "\
let mapleader = ','
let mapleader = \"\\<2-LeftMouse>\"
let mapleader = \"\\<3-LeftMouse>\"
let mapleader = \"\\<4-LeftMouse>\"
let mapleader = \"\\<2-a>\"
let mapleader = \"\\<C-3-a>\"
let mapleader = \"\\<2-foo>\"
nnoremap <leader>a 1
let mapleader = \"\\<5-a>\"
nnoremap <leader>b 2
";
    let vimrc = parse_vimrc(text);
    assert_eq!(
        vimrc.issues,
        vec![
            issue(2, "mapleader: <2-LeftMouse> isn't supported"),
            issue(3, "mapleader: <3-LeftMouse> isn't supported"),
            issue(4, "mapleader: <4-LeftMouse> isn't supported"),
            issue(5, "mapleader: <2-a> isn't supported"),
            issue(6, "mapleader: <C-3-a> isn't supported"),
            issue(7, "mapleader: <2-foo> isn't supported"),
        ]
    );
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], ",a", "1", false),
            remap(&[N], "<lt>5-a>b", "2", false),
        ]
    );

    // `<char-N>` reads N as nvim does: `0x`, `0o` and `0b` prefixes, a
    // leading `0` octal unless a digit is 8 or 9 (then decimal), and the
    // low 32 bits of the value, so each leader here equals the one that
    // unmaps its map. A sign alone is text; a number nvim rejects fails the
    // `let`.
    let text = "\
let mapleader = \"\\<char-018>\"
nnoremap <leader>a 1
let mapleader = \"\\x12\"
nunmap <leader>a
let mapleader = \"\\<char-017>\"
nnoremap <leader>b 2
let mapleader = \"\\<C-O>\"
nunmap <leader>b
let mapleader = \"\\<char-4294967361>\"
nnoremap <leader>c 3
let mapleader = 'A'
nunmap <leader>c
let mapleader = \"\\<char-0o101>\\<char-0b1000001>\\<char-0101>\\<char-0X41>\\<C-char-97>\"
nnoremap <leader>d 4
let mapleader = \"AAAA\\<C-A>\"
nunmap <leader>d
let mapleader = \"\\<char-0189>\"
nnoremap <leader>e 5
let mapleader = \"\\<char--1>\"
nnoremap <leader>f 6
let mapleader = \"\\<char-0x>\"
let mapleader = \"\\<char-65a>\"
let mapleader = \"\\<char-4294967295>\"
nnoremap <leader>g 7
";
    let vimrc = parse_vimrc(text);
    assert_eq!(
        vimrc.issues,
        vec![
            issue(21, "mapleader: <char-0x> is invalid"),
            issue(22, "mapleader: <char-65a> is invalid"),
            issue(23, "mapleader: <char-4294967295> isn't supported"),
        ]
    );
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "\u{bd}e", "5", false),
            remap(&[N], "<lt>char--1>f", "6", false),
            remap(&[N], "<lt>char--1>g", "7", false),
        ]
    );

    // A `\U` value is a C `int` in nvim, and a negative one writes its low
    // byte, so `"\U80000041"` is `A`. Bytes that aren't UTF-8, and codes
    // that aren't characters, are kept raw by nvim and have no key here:
    // each is an issue and the leader stays `,`.
    let text = "\
let mapleader = \"\\U80000041\"
nnoremap <leader>a 1
let mapleader = 'A'
nunmap <leader>a
let mapleader = ','
let mapleader = \"\\xFF\"
let mapleader = \"\\uD800\"
let mapleader = \"\\U110000\"
let mapleader = \"\\xC3\"
let mapleader = \"\\U40000041\"
let mapleader = \"\\UF00000E9\"
nnoremap <leader>b 2
";
    let vimrc = parse_vimrc(text);
    let invalid = "mapleader isn't valid UTF-8";
    assert_eq!(
        vimrc.issues,
        (6..=11)
            .map(|line| issue(line, invalid))
            .collect::<Vec<_>>()
    );
    assert_eq!(vimrc.remaps, vec![remap(&[N], ",b", "2", false)]);
}

#[test]
fn vimrc_reports_problems() {
    let text = "\
nnoremap a 1
imap jk <Esc>
inoremap jj <Esc>
cnoremap <C-a> <C-b>
map! x y
noremap! x y
smap b c
snoremap b c
nnoremap <buffer> d e
nnoremap <expr> d e
nnoremap <script> d e
nnoremap <unique> d e
nmap
nmap x
nnoremap <silent> x\x20\x20
set number
let g:loaded = 1
syntax on
autocmd BufRead * echo 1
source other.vim
function! F()
\\ continued
nnoremap y <Plug>(thing)
nnoremap <Nop> z
nnoremap z <F13>
nnoremap e 2
nmap! f g
let mapleader = \"\\<F2>\"
let mapleader = \"\\<S-Tab>\"
nnoremap <leader>k 3
mapclear x
nmapclear <buffer>
let mapleader = \"\\<M-a>\"
let mapleader = \"\\<C-@>\"
";
    let vimrc = parse_vimrc(text);
    let select = "Select mode isn't in fastedit";
    assert_eq!(
        vimrc.issues,
        vec![
            issue(7, select),
            issue(8, select),
            issue(9, "<buffer> isn't supported"),
            issue(10, "<expr> isn't supported"),
            issue(11, "<script> isn't supported"),
            issue(12, "<unique> isn't supported"),
            issue(13, "listing maps isn't supported"),
            issue(14, "listing maps isn't supported"),
            issue(15, "listing maps isn't supported"),
            issue(16, "not supported: set"),
            issue(17, "not supported: let g:loaded"),
            issue(18, "not supported: syntax"),
            issue(19, "not supported: autocmd"),
            issue(20, "not supported: source"),
            issue(21, "not supported: function!"),
            issue(22, "line continuation isn't supported"),
            issue(23, "<Plug> is not supported"),
            issue(24, "<Nop> can't be a left side"),
            issue(25, "unknown key <F13>"),
            issue(27, "not supported: nmap!"),
            issue(28, "mapleader: <F2> isn't supported"),
            issue(29, "mapleader: <S-Tab> isn't supported"),
            issue(31, "trailing characters: x"),
            issue(32, "<buffer> isn't supported"),
            issue(33, "mapleader: <M-a> isn't supported"),
            issue(34, "mapleader: <C-@> isn't supported"),
        ]
    );
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "a", "1", false),
            remap(&[I], "jk", "<Esc>", true),
            remap(&[I], "jj", "<Esc>", false),
            remap(&[C], "<C-a>", "<C-b>", false),
            remap(&[I, C], "x", "y", true),
            remap(&[I, C], "x", "y", false),
            remap(&[N], "e", "2", false),
            remap(&[N], "\\k", "3", false),
        ]
    );

    // One issue per command, and the commands around it on the line still
    // load.
    let vimrc = parse_vimrc("nnoremap a b | imap c d | nnoremap e f\nnnoremap g h\n");
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[N], "a", "b ", false),
            remap(&[I], "c", "d ", true),
            remap(&[N], "e", "f", false),
            remap(&[N], "g", "h", false),
        ]
    );
}
