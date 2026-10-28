//! `]e` and `[e`, vim-unimpaired's `:move +{count}` and `:move --{count}`
//! (its `doc/unimpaired.txt` and `plugin/unimpaired.vim`), and the edges of
//! `[<Space>` and `]<Space>` nvim's corpus can't reach (`line_edits_oracle.rs`
//! has the rest). nvim has no `]e` to record: the text, caret, marks,
//! changelist, `'[`, `']` and `'.` a move leaves were probed with nvim 0.12.4
//! running the `:move` unimpaired runs (`nvim --clean --headless` on a file of
//! five rows, `zero` to `four`, with `:let &ul=&ul` between commands so each
//! is its own change). Past the buffer's edge unimpaired's `:move` refuses
//! (E16); these move as far as they can, as mini.move does. In a visual mode
//! the selection is kept, as mini.move keeps it.

use fe_buffer::ResolvedSelection;
use fe_vim::{HeadlessVim, HostCommand};

const ROWS: &str = "zero\none\ntwo\nthree\nfour";

/// Feeds `keys` to a vim on `text` at `cursor`.
fn after(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    vim
}

/// Asserts the text and caret `keys` leave, in normal mode.
#[track_caller]
fn check(text: &str, cursor: (u32, usize), keys: &str, expected: &str, caret: (u32, usize)) {
    let vim = after(text, cursor, keys);
    assert_eq!(vim.text(), expected, "{keys:?} on {text:?}");
    assert_eq!(vim.cursor(), caret, "{keys:?} on {text:?}: the caret");
    assert_eq!(vim.mode(), "normal", "{keys:?} on {text:?}: the mode");
}

/// Asserts `keys` fail and change nothing, the caret staying.
#[track_caller]
fn fails(text: &str, cursor: (u32, usize), keys: &str) {
    let vim = after(text, cursor, keys);
    assert_eq!(vim.text(), text, "{keys:?} on {text:?}");
    assert_eq!(vim.cursor(), cursor, "{keys:?} on {text:?}: the caret");
    assert!(vim.state().failed(), "{keys:?} on {text:?} fails");
}

/// The changelist `:changes` lists, oldest first, as (line, column).
fn changelist(vim: &mut HeadlessVim) -> Vec<(String, String)> {
    vim.feed(":changes<CR>").unwrap();
    match vim.state_mut().take_host_command() {
        Some(HostCommand::Pick(pick)) => pick
            .rows
            .iter()
            .map(|row| (row.cells[1].clone(), row.cells[2].clone()))
            .filter(|(line, _)| !line.is_empty())
            .collect(),
        other => panic!(":changes: {other:?}"),
    }
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|&(line, column)| (line.to_owned(), column.to_owned()))
        .collect()
}

/// Carets at each of `offsets`, in normal mode.
fn carets(vim: &mut HeadlessVim, offsets: &[usize]) {
    vim.model_mut()
        .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
}

fn heads(vim: &HeadlessVim) -> Vec<(u32, usize)> {
    let text = vim.model().text().clone();
    vim.model()
        .selections()
        .iter()
        .map(|selection| {
            let point = text.offset_to_point(selection.head());
            (point.row, point.column as usize)
        })
        .collect()
}

#[test]
fn exchange_moves_the_row() {
    // The caret's row swaps with the next or previous, the caret going
    // with it at its column.
    check(ROWS, (1, 2), "]e", "zero\ntwo\none\nthree\nfour", (2, 2));
    check(ROWS, (2, 1), "[e", "zero\ntwo\none\nthree\nfour", (1, 1));
    // Counted, it goes that many rows, those it passes each moving one row
    // the other way.
    check(ROWS, (1, 2), "2]e", "zero\ntwo\nthree\none\nfour", (3, 2));
    check(ROWS, (3, 4), "3[e", "three\nzero\none\ntwo\nfour", (0, 4));
    // To the first and the last row.
    check(ROWS, (1, 0), "[e", "one\nzero\ntwo\nthree\nfour", (0, 0));
    check(ROWS, (3, 0), "]e", "zero\none\ntwo\nfour\nthree", (4, 0));
    // An empty row moves as any other, and a caret past a short row's end
    // stays on its own row's column.
    check("a\n\nb", (1, 0), "]e", "a\nb\n", (2, 0));
    check("long row\nx", (0, 6), "]e", "x\nlong row", (1, 6));

    // At the edge it goes as far as it can; with no row left that way it
    // fails, changing nothing.
    check(ROWS, (2, 0), "9]e", "zero\none\nthree\nfour\ntwo", (4, 0));
    check(ROWS, (2, 0), "9[e", "two\nzero\none\nthree\nfour", (0, 0));
    fails(ROWS, (4, 1), "]e");
    fails(ROWS, (0, 1), "[e");
    fails(ROWS, (0, 1), "3[e");
    fails("only", (0, 2), "]e");
    fails("only", (0, 2), "[e");
    // A failure stops a macro.
    let vim = after(ROWS, (3, 0), "qq]exq@q");
    assert_eq!(vim.text(), "zero\none\ntwo\nfour\nhree");

    // One undo step: `u` puts the rows and the caret back, `<C-r>` moves
    // them again.
    check(ROWS, (1, 2), "2]eu", ROWS, (1, 2));
    check(ROWS, (3, 2), "2[eu", ROWS, (3, 2));
    check(
        ROWS,
        (1, 2),
        "2]eu<C-r>",
        "zero\ntwo\nthree\none\nfour",
        (1, 2),
    );
    check(ROWS, (1, 2), "]e]eu", "zero\ntwo\none\nthree\nfour", (2, 2));

    // `.` moves it again, as far; a count given to `.` takes its place.
    check(ROWS, (0, 1), "]e.", "one\ntwo\nzero\nthree\nfour", (2, 1));
    check(ROWS, (0, 1), "2]e.", "one\ntwo\nthree\nfour\nzero", (4, 1));
    check(ROWS, (4, 1), "[e2.", "zero\nfour\none\ntwo\nthree", (1, 1));
    check(ROWS, (0, 0), "]ej.u", "one\nzero\ntwo\nthree\nfour", (2, 0));

    // The row's marks go with it, as do those of the rows it passes;
    // others stay. As nvim's `:move +2` from line 2 leaves them.
    let mut vim = after(ROWS, (0, 0), "jllxGx2Gmajmbk2]e");
    assert_eq!(vim.text(), "zero\ntwo\nthree\non\nfur");
    assert_eq!(vim.cursor(), (3, 1));
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((3, 1)), Some((1, 1))));
    // `'[` and `']` on the row moved, `'.` on the first row changed.
    assert_eq!(
        ['[', ']', '.'].map(|name| vim.mark(name)),
        [Some((3, 0)), Some((3, 0)), Some((1, 0))]
    );
    // The changelist's entry for the `x` on the row went with it; the move
    // adds one at the first row it changed.
    assert_eq!(
        changelist(&mut vim),
        pairs(&[("4", "2"), ("5", "1"), ("2", "0")])
    );
    // `:move --2` from line 4: the rows passed go down.
    let mut vim = after(ROWS, (0, 0), "jllxGx4G2[e");
    assert_eq!(vim.text(), "zero\nthree\non\ntwo\nfur");
    assert_eq!(
        ['[', ']', '.'].map(|name| vim.mark(name)),
        [Some((1, 0)), Some((1, 0)), Some((1, 0))]
    );
    assert_eq!(
        changelist(&mut vim),
        pairs(&[("3", "2"), ("5", "1"), ("2", "0")])
    );
    // `'<` and `'>` go with their rows' text too.
    let vim = after(ROWS, (1, 1), "vjl<Esc>k]e");
    // `'<` is the earlier end, as nvim's `getpos("'<")` reads it.
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((1, 2)), Some((2, 1))));

    // Not a jump: the jump mark and the jumplist stay.
    let vim = after(ROWS, (0, 2), "G]e");
    assert_eq!(vim.mark('\''), Some((0, 2)));
    let vim = after(ROWS, (0, 2), "G[e");
    assert_eq!(vim.mark('\''), Some((0, 2)));
    check(
        ROWS,
        (0, 2),
        "G2[e<C-o>",
        "zero\none\nfour\ntwo\nthree",
        (0, 2),
    );
}

#[test]
fn visual_line_moves_the_selection() {
    // `V`: the selected rows move whole, the selection kept on them.
    let vim = after(ROWS, (1, 1), "Vj]e");
    assert_eq!(vim.text(), "zero\nthree\none\ntwo\nfour");
    assert_eq!(vim.mode(), "visual_line");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((2, 1)), (3, 1)));
    let vim = after(ROWS, (3, 2), "Vk2[e");
    assert_eq!(vim.text(), "two\nthree\nzero\none\nfour");
    assert_eq!(vim.mode(), "visual_line");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((1, 2)), (0, 2)));
    // Again and again, then back: `gv` and `'<` `'>` after are its rows.
    let mut vim = after(ROWS, (0, 0), "Vj]e]e[e");
    assert_eq!(vim.text(), "two\nzero\none\nthree\nfour");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((1, 0)), (2, 0)));
    vim.feed("<Esc>").unwrap();
    // A linewise `'>` reads as its row's last byte.
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((1, 0)), Some((2, 2))));
    // Charwise and blockwise the selection's rows move, its columns kept.
    let vim = after(ROWS, (0, 1), "vjl]e");
    assert_eq!(vim.text(), "two\nzero\none\nthree\nfour");
    assert_eq!((vim.mode(), vim.visual_anchor()), ("visual", Some((1, 1))));
    assert_eq!(vim.cursor(), (2, 2));
    let vim = after(ROWS, (1, 0), "<C-v>jl[e");
    assert_eq!(vim.text(), "one\ntwo\nzero\nthree\nfour");
    assert_eq!(vim.mode(), "visual_block");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((0, 0)), (1, 1)));
    // At the edge as far as they can go; none left fails, keeping the
    // selection.
    let vim = after(ROWS, (2, 0), "Vj5]e");
    assert_eq!(vim.text(), "zero\none\nfour\ntwo\nthree");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((3, 0)), (4, 0)));
    let vim = after(ROWS, (3, 0), "Vj]e");
    assert_eq!(vim.text(), ROWS);
    assert!(vim.state().failed());
    assert_eq!(vim.mode(), "visual_line");
    assert_eq!((vim.visual_anchor(), vim.cursor()), (Some((3, 0)), (4, 0)));
    // One undo step, back in normal mode with the rows as they were.
    check(ROWS, (0, 0), "Vj2]e<Esc>u", ROWS, (1, 0));
    // `.` moves as many rows from the caret.
    check(
        ROWS,
        (0, 0),
        "Vj]e<Esc>k.",
        "two\nthree\nzero\none\nfour",
        (2, 0),
    );

    // Several carets: their rows move together, each caret with its row.
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    carets(&mut vim, &[1, 10]);
    vim.feed("]e").unwrap();
    assert_eq!(vim.text(), "one\nzero\nthree\ntwo\nfour");
    assert_eq!(heads(&vim), [(1, 1), (3, 1)]);
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), ROWS);
    assert_eq!(heads(&vim), [(0, 1), (2, 1)]);
    // Next to each other they move as one run; two on one row move it once.
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    carets(&mut vim, &[6, 10, 11]);
    vim.feed("[e").unwrap();
    assert_eq!(vim.text(), "one\ntwo\nzero\nthree\nfour");
    assert_eq!(heads(&vim), [(0, 1), (1, 1), (1, 2)]);
    // They go as far as the one nearest the edge can.
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    carets(&mut vim, &[0, 14]);
    vim.feed("3]e").unwrap();
    assert_eq!(vim.text(), "one\nzero\ntwo\nfour\nthree");
    assert_eq!(heads(&vim), [(1, 0), (4, 1)]);
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    carets(&mut vim, &[0, 14]);
    vim.feed("[e").unwrap();
    assert_eq!(vim.text(), ROWS);
    assert!(vim.state().failed());
}

#[test]
fn operators_cancel() {
    // Neither is a motion or an object: after an operator they cancel,
    // editing nothing and writing no register.
    for keys in [
        "d]e",
        "d[e",
        "c]e",
        "y[e",
        "2d]e",
        "d[<Space>",
        "d]<Space>",
        "y]<Space>",
    ] {
        let vim = after(ROWS, (1, 1), keys);
        assert_eq!(vim.text(), ROWS, "{keys}");
        assert_eq!(vim.cursor(), (1, 1), "{keys}");
        assert_eq!(vim.mode(), "normal", "{keys}");
        assert_eq!(vim.registers().get('"'), None, "{keys}");
    }
    // An empty buffer: `]<Space>` and `[<Space>` add rows; `]e` and `[e`
    // fail.
    check("", (0, 0), "]<Space>", "\n", (0, 0));
    check("", (0, 0), "2[<Space>", "\n\n", (2, 0));
    fails("", (0, 0), "]e");
    fails("", (0, 0), "[e");
    check("", (0, 0), "]<Space>u", "", (0, 0));
    // With several carets each row gets its blank rows once, the carets
    // staying on their text.
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    carets(&mut vim, &[1, 10, 11]);
    vim.feed("[<Space>").unwrap();
    assert_eq!(vim.text(), "\nzero\none\n\ntwo\nthree\nfour");
    assert_eq!(heads(&vim), [(1, 1), (4, 1), (4, 2)]);
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), ROWS);
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    carets(&mut vim, &[1, 10]);
    vim.feed("2]<Space>").unwrap();
    assert_eq!(vim.text(), "zero\n\n\none\ntwo\n\n\nthree\nfour");
    assert_eq!(heads(&vim), [(0, 1), (4, 1)]);
}

#[test]
fn blank_lines_set_change_marks() {
    // As nvim 0.12.4 leaves them (probed on a file of five rows, `zero` to
    // `four`): `'[` and `']` where the caret was, as the `g@l` its default
    // runs sets them; `'.` and the changelist's entry at the first row put
    // in.
    let marks = |vim: &HeadlessVim| ['[', ']', '.'].map(|name| vim.mark(name));
    let mut vim = after(ROWS, (1, 2), "2]<Space>");
    assert_eq!(marks(&vim), [Some((1, 2)), Some((1, 2)), Some((2, 0))]);
    assert_eq!(changelist(&mut vim), pairs(&[("3", "0")]));
    let mut vim = after(ROWS, (2, 2), "2[<Space>");
    assert_eq!(marks(&vim), [Some((2, 2)), Some((2, 2)), Some((2, 0))]);
    assert_eq!(changelist(&mut vim), pairs(&[("3", "0")]));
    let mut vim = after(ROWS, (1, 2), "x2G]<Space>");
    assert_eq!(changelist(&mut vim), pairs(&[("2", "2"), ("3", "0")]));
}

#[test]
fn moving_rows_of_the_same_text_is_an_undo_step() {
    // Empty rows swapped change no text, but `u` takes back the move, not
    // the edit before it.
    check("a\n\n", (0, 0), "rXj]eu", "X\n\n", (1, 0));
    check("\n\na", (2, 0), "rXk[eu", "\n\nX", (1, 0));
    check("a\n\n", (0, 0), "rXj]euu", "a\n\n", (0, 0));
}

#[test]
fn dot_after_a_visual_move_keeps_its_rows() {
    let rows = "a\nb\nc\nd\ne\nf\ng";
    check(rows, (0, 0), "Vj]e<Esc>k.", "c\nd\na\nb\ne\nf\ng", (2, 0));
    check(rows, (0, 0), "Vj]e<Esc>k..", "c\nd\ne\na\nb\nf\ng", (3, 0));
    check(rows, (6, 0), "Vk[e<Esc>..", "a\nb\nf\ng\nc\nd\ne", (2, 0));
}

#[test]
fn undo_puts_named_and_visual_marks_back() {
    // As nvim's `u` after `:move` restores `a` to `z`, `'<` and `'>`, and
    // `<C-r>` moves them again.
    let mut vim = after("one\ntwo", (0, 1), "ma]e");
    assert_eq!(vim.mark('a'), Some((1, 1)));
    vim.feed("u").unwrap();
    assert_eq!(vim.mark('a'), Some((0, 1)));
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.mark('a'), Some((1, 1)));
    // A row passed over, and moves repeated by `.`.
    let mut vim = after(ROWS, (1, 2), "mbggma]e.");
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((2, 2)), Some((0, 2))));
    vim.feed("u").unwrap();
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((1, 2)), Some((0, 2))));
    vim.feed("u").unwrap();
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((0, 2)), Some((1, 2))));
    vim.feed("2<C-r>").unwrap();
    assert_eq!((vim.mark('a'), vim.mark('b')), (Some((2, 2)), Some((0, 2))));
    // `'<` and `'>`, after a visual move too.
    let mut vim = after(ROWS, (0, 0), "vjl<Esc>gg]e");
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((0, 1)), Some((1, 0))));
    vim.feed("u").unwrap();
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((0, 0)), Some((1, 1))));
    let mut vim = after(ROWS, (1, 0), "Vj]e<Esc>u");
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((1, 0)), Some((2, 2))));
    vim.feed("<C-r>").unwrap();
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((2, 0)), Some((3, 2))));

    // What `u` puts back is what the marks were before the change, as
    // nvim's undo header saved them, and `<C-r>` what they were before the
    // `u`: a mark set again since the move goes back too.
    let mut vim = after(ROWS, (0, 1), "ma]ejma");
    assert_eq!(vim.mark('a'), Some((2, 1)));
    vim.feed("u").unwrap();
    assert_eq!(vim.mark('a'), Some((0, 1)));
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.mark('a'), Some((2, 1)));
    // A mark set only after the move stays where it is.
    let vim = after(ROWS, (0, 1), "]ejmbu");
    assert_eq!(vim.mark('b'), Some((2, 1)));
    // A visual extent made since the move gives way to the one saved.
    let mut vim = after(ROWS, (0, 0), "vl<Esc>]eGvl<Esc>");
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((4, 1)), Some((4, 2))));
    vim.feed("u").unwrap();
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((0, 0)), Some((0, 1))));
    vim.feed("<C-r>").unwrap();
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((4, 1)), Some((4, 2))));
    // A move sharing its change with rows put in before it: `u` takes back
    // both, the mark back on its first row.
    let mut vim = after("one\ntwo\nthree", (0, 1), "ma:normal [ ]e<CR>");
    assert_eq!(vim.text(), "\ntwo\none\nthree");
    assert_eq!(vim.mark('a'), Some((2, 1)));
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "one\ntwo\nthree");
    assert_eq!(vim.mark('a'), Some((0, 1)));
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.mark('a'), Some((2, 1)));
    // A move after a put in one change: `u` takes the put row away, and
    // `<C-r>` puts the mark back on it, as it was before the `u`.
    let mut vim = after("one\ntwo", (0, 0), "ma:normal yyp[e<CR>");
    assert_eq!(vim.text(), "one\none\ntwo");
    assert_eq!(vim.mark('a'), Some((1, 0)));
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "one\ntwo");
    assert_eq!(vim.mark('a'), Some((0, 0)));
    vim.feed("<C-r>").unwrap();
    assert_eq!(vim.text(), "one\none\ntwo");
    assert_eq!(vim.mark('a'), Some((1, 0)));
    vim.feed("u").unwrap();
    assert_eq!(vim.mark('a'), Some((0, 0)));
    let mut vim = after("one\ntwo\nthree", (0, 0), "ma:normal yyp2]e<CR>");
    assert_eq!(vim.text(), "one\ntwo\nthree\none");
    vim.feed("u<C-r>").unwrap();
    assert_eq!(vim.text(), "one\ntwo\nthree\none");
    assert_eq!(vim.mark('a'), Some((0, 0)));
}

#[test]
fn moves_note_the_first_row_changed() {
    // `'.` and the changelist's entry on the first row the move spans, even
    // where rows of the same text changed nothing there, as nvim's `:move`
    // leaves them; `'[` and `']` on the rows moved.
    let notes = |text: &str, cursor: (u32, usize), keys: &str, expected: &str| {
        let mut vim = after(text, cursor, keys);
        assert_eq!(vim.text(), expected, "{keys:?} on {text:?}");
        let marks = ['[', ']', '.'].map(|name| vim.mark(name));
        (marks, changelist(&mut vim))
    };
    // `:move +2` from line 1 of `a a b`.
    let (marks, list) = notes("a\na\nb", (0, 0), "2]e", "a\nb\na");
    assert_eq!(marks, [Some((2, 0)), Some((2, 0)), Some((0, 0))]);
    assert_eq!(list, pairs(&[("1", "0")]));
    // `:move --2` from line 3 of `b a a`.
    let (marks, list) = notes("b\na\na", (2, 0), "2[e", "a\nb\na");
    assert_eq!(marks, [Some((0, 0)), Some((0, 0)), Some((0, 0))]);
    assert_eq!(list, pairs(&[("1", "0")]));
    // All the same text: still a change, noted.
    let (marks, list) = notes("a\na\na", (0, 0), "]e", "a\na\na");
    assert_eq!(marks, [Some((1, 0)), Some((1, 0)), Some((0, 0))]);
    assert_eq!(list, pairs(&[("1", "0")]));
    let (marks, list) = notes("a\na\na", (2, 0), "[e", "a\na\na");
    assert_eq!(marks, [Some((1, 0)), Some((1, 0)), Some((1, 0))]);
    assert_eq!(list, pairs(&[("2", "0")]));
}

#[test]
fn blank_lines_past_the_put_budget_fail() {
    let vim = after(ROWS, (1, 0), "1000000000]<Space>");
    assert_eq!(vim.text(), ROWS);
    assert!(vim.state().failed());
    assert!(
        vim.state()
            .message()
            .is_some_and(|message| message.contains("bytes")),
        "{:?}",
        vim.state().message()
    );
}
