use serde_json::json;

use super::{Measuring, commit, registers};
use crate::{HeadlessVim, Register, Splits, Step, VimState, context};

/// The model's selections as ranges.
fn selections(vim: &HeadlessVim) -> Vec<std::ops::Range<usize>> {
    vim.model()
        .selections()
        .into_iter()
        .map(|selection| selection.range)
        .collect()
}

/// `vim.mode` and the flag `vim.visual` on the stamped context.
fn stamped(state: &VimState) -> (String, bool) {
    let stack = context(state);
    let frame = stack.frames().last().unwrap().clone();
    let mode = frame.values.get("vim.mode").cloned().unwrap();
    (mode.to_string(), frame.flags.contains("vim.visual"))
}

#[test]
fn block_on_the_model() {
    for keys in ["<C-v>", "<C-q>"] {
        let invocation = commit(keys);
        assert_eq!(invocation.command.as_str(), "vim.visual");
        assert_eq!(invocation.args, json!({"kind": "block"}));
    }
    let mut vim = HeadlessVim::new("abcd\nab\n\tx\nabcd", (0, 1));
    vim.feed("<C-q>").unwrap();
    assert_eq!(vim.mode(), "visual_block");
    assert_eq!(stamped(vim.state()), ("visual_block".into(), true));

    // One selection per row over display columns 1 and 2: on the tab row,
    // the tab, which nvim's `d` takes there (leaving its other columns as
    // spaces, as the corpus records).
    vim.feed("3jl").unwrap();
    assert_eq!(vim.cursor(), (3, 2));
    assert_eq!(vim.visual_anchor(), Some((0, 1)));
    assert_eq!(selections(&vim), [1..3, 6..7, 8..9, 12..14]);
    // `$` takes each row to its end, until `h` or `l`.
    vim.feed("$").unwrap();
    assert_eq!(selections(&vim), [1..4, 6..7, 8..10, 12..15]);
    vim.feed("h").unwrap();
    assert_eq!(vim.cursor(), (3, 3));
    assert_eq!(selections(&vim), [1..4, 6..7, 8..9, 12..15]);
    vim.feed("hh").unwrap();
    assert_eq!(selections(&vim), [1..2, 6..7, 8..9, 12..13]);

    // `o` goes to the opposite corner, `O` to the other one on its row.
    let mut vim = HeadlessVim::new("abcd\nab\n\tx\nabcd", (0, 1));
    vim.feed("<C-v>3jlo").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 1), Some((3, 2))));
    vim.feed("o").unwrap();
    vim.feed("O").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((3, 1), Some((0, 2))));
    assert_eq!(selections(&vim), [1..3, 6..7, 8..9, 12..14]);

    // A row that ends before the block's left column takes nothing.
    let mut vim = HeadlessVim::new("abcd\na\n\nabcd", (0, 2));
    vim.feed("<C-v>3jl").unwrap();
    assert_eq!(selections(&vim), [2..4, 10..12]);
    // Taking nothing at all, the model keeps a caret at the cursor.
    let mut vim = HeadlessVim::new("\n\nab", (0, 0));
    vim.feed("<C-v>j").unwrap();
    assert_eq!(selections(&vim), vec![1..1]);

    // In a block `D` and `C` reach the rows' ends; `I` and `A` are its own.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.feed("<C-v>").unwrap();
    let mut state = vim.state().clone();
    for (key, operator, target) in [
        ('D', "delete", "selected_to_end"),
        ('C', "change", "selected_to_end"),
        ('X', "delete", "selected_lines"),
        ('I', "insert", "selection"),
        ('A', "append", "selection"),
    ] {
        let Step::Commit(invocation) = state.feed(crate::VimKey::Char(key)) else {
            panic!("{key} didn't commit");
        };
        assert_eq!(
            invocation.args,
            json!({"operator": operator, "target": target}),
            "{key}"
        );
    }
}

#[test]
fn operators_registers_and_put() {
    // A block `d` is one transaction, and a blockwise register.
    let mut vim = HeadlessVim::new("abcd\nabcd\nabcd", (0, 1));
    vim.feed("<C-v>jld").unwrap();
    assert_eq!(vim.text(), "ad\nad\nabcd");
    assert_eq!(vim.cursor(), (0, 1));
    let unnamed = vim.registers().get('"').unwrap();
    assert_eq!(unnamed.text, "bc\nbc");
    assert_eq!((unnamed.linewise, unnamed.block_width), (false, Some(2)));
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "abcd\nabcd\nabcd");

    // `p` pads a shorter row and adds rows past the end.
    let mut vim = HeadlessVim::new("abcd\nab\nabcd", (0, 1));
    vim.feed("<C-v>jlyjjp").unwrap();
    assert_eq!(vim.text(), "abcd\nab\nabbccd\n  b");
    assert_eq!(vim.cursor(), (2, 2));
    let mut vim = HeadlessVim::new("abcd\nab\nabcd", (0, 1));
    vim.feed("<C-v>jlyjp").unwrap();
    assert_eq!(vim.text(), "abcd\nabbc\nabb cd");

    // `I` types on every row, as one transaction, and `.` repeats it.
    let mut vim = HeadlessVim::new("abcd\nabcd\nabcd\nabcd", (0, 0));
    vim.feed("<C-v>jlIXY<Esc>").unwrap();
    assert_eq!(vim.text(), "XYabcd\nXYabcd\nabcd\nabcd");
    assert_eq!((vim.cursor(), vim.mode()), ((0, 0), "normal"));
    vim.feed("jj.").unwrap();
    assert_eq!(vim.text(), "XYabcd\nXYabcd\nXYabcd\nXYabcd");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "XYabcd\nXYabcd\nabcd\nabcd");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "abcd\nabcd\nabcd\nabcd");

    // `$A` appends to every row's end.
    let mut vim = HeadlessVim::new("abcd\nab\n\tx", (0, 1));
    vim.feed("<C-v>jj$A;<Esc>").unwrap();
    assert_eq!(vim.text(), "abcd;\nab;\n\tx;");

    // Charwise `p` leaves what it replaced in `"`; `P` leaves it alone.
    let mut vim = HeadlessVim::new("xy abcd", (0, 0));
    vim.feed("ywwvlp").unwrap();
    assert_eq!(vim.text(), "xy xy cd");
    assert_eq!(vim.registers().get('"'), Some(&Register::new("ab", false)));
    let mut vim = HeadlessVim::new("xy abcd", (0, 0));
    vim.feed("ywwvlP").unwrap();
    assert_eq!(vim.text(), "xy xy cd");
    assert_eq!(vim.registers().get('"'), Some(&Register::new("xy ", false)));

    // `"+y` of a block puts its rows, joined, on the clipboard, and reading
    // that text back gives the block again.
    let mut vim = HeadlessVim::new("abcd\nab\nabcd", (0, 1));
    vim.feed("<C-v>jjl\"+y").unwrap();
    assert_eq!(vim.host().clipboard.as_deref(), Some("bc\nb\nbc"));
    vim.feed("\"+P").unwrap();
    assert_eq!(vim.text(), "abcbcd\nab b\nabcbcd");
}

#[test]
fn measuring_waits_for_the_whole_put() {
    // The delete and the put go in as one edit: measuring text, which may
    // edit the buffer through another model, runs once both are in, sees
    // all of the put, and what it writes then stays as it wrote it. The
    // block takes columns 2 and 3 of rows 1 to 3, and the delete leaves a
    // space for the tab's column before them.
    let text = "X\nabcd\na\tbc\nabcd";
    let ai = || fe_buffer::Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    for (yank, edit, want) in [
        // A row put in above the block, one between its rows, and one of
        // its rows taken out.
        ("yl", (0..0, "new\n"), "new\nX\nabX\na Xbc\nabX"),
        ("yl", (6..6, "new\n"), "X\nabX\nnew\na Xbc\nabX"),
        ("yl", (6..12, ""), "X\nabX\nabX"),
        // Where the delete left a gap, and in the space it left.
        ("yl", (4..4, "KEEP"), "X\nabKEEPX\na Xbc\nabX"),
        ("yl", (7..8, "P"), "X\nabX\naPXbc\nabX"),
        // Lines go below the block; a block goes on its rows.
        ("yy", (5..10, ""), "X\nab\nab\nX"),
        ("<C-v>jly", (8..8, "KEEP"), "X\nabX\na KEEPabbc\nab"),
    ] {
        let keys = format!("{yank}gg0jll<C-v>jjl");
        let mut plain = HeadlessVim::new(text, (0, 0));
        plain.feed(&keys).unwrap();
        plain.feed("p").unwrap();
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.feed(&keys).unwrap();
        let mut split = vim.model().split();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let (saw, made) = (seen.clone(), edit.clone());
        let _measuring = Measuring::new(move |after_edit| {
            if after_edit {
                let text = split.buffer().snapshot().text().clone();
                saw.borrow_mut().push(text.slice_to_string(0..text.len()));
                if saw.borrow().len() == 1 {
                    split.buffer_mut().edit([made.clone()], ai());
                }
            }
        });
        vim.feed("p").unwrap();
        assert_eq!(*seen.borrow(), [plain.text()], "{yank} then {edit:?}");
        assert_eq!(vim.text(), want, "{yank} then {edit:?}");
    }
}

#[test]
fn block_typing_follows_rows_edited_meanwhile() {
    // `Esc` types on each of the block's other rows that's as it was when
    // the session opened, where the block's column fell on it then: rows
    // another origin put in or took out around it, or an edit of theirs
    // undone, don't matter; a row whose text they changed, or that they
    // joined to another, gets nothing, and their text is never touched.
    let ai = || fe_buffer::Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    let typed = |(text, column): (&str, usize),
                 keys: &str,
                 edits: &[(std::ops::Range<usize>, &str)],
                 undo: bool| {
        let mut vim = HeadlessVim::new(text, (0, column));
        vim.feed(keys).unwrap();
        let mut split = vim.model().split();
        for edit in edits {
            split.buffer_mut().edit([edit.clone()], ai());
        }
        if undo {
            split.buffer_mut().undo(|origin| origin.is_ai()).unwrap();
        }
        vim.feed("<Esc>").unwrap();
        vim.text()
    };
    // Once XY is typed on the first, the rows start at 7, 12 and 17.
    let rows = ("abcd\nefgh\nijkl\nmnop", 1);
    for (edit, want) in [
        // Rows put in or taken out around a row.
        ((12..12, "new\n"), "aXYbcd\neXYfgh\nnew\niXYjkl\nmnop"),
        ((12..17, ""), "aXYbcd\neXYfgh\nmnop"),
        ((7..12, ""), "aXYbcd\niXYjkl\nmnop"),
        // Its text changed: put in at its start or its place, emptied,
        // replaced, or joined to the row before or after.
        ((7..7, "\t"), "aXYbcd\n\tefgh\niXYjkl\nmnop"),
        ((8..8, "ZZ"), "aXYbcd\neZZfgh\niXYjkl\nmnop"),
        ((7..11, ""), "aXYbcd\n\niXYjkl\nmnop"),
        ((7..11, "new"), "aXYbcd\nnew\niXYjkl\nmnop"),
        ((7..12, "new\n"), "aXYbcd\nnew\niXYjkl\nmnop"),
        ((6..7, ""), "aXYbcdefgh\niXYjkl\nmnop"),
        ((11..12, ""), "aXYbcd\nefghijkl\nmnop"),
        // Joined to the text typed on the first row.
        ((3..8, ""), "aXYfgh\niXYjkl\nmnop"),
    ] {
        assert_eq!(
            typed(rows, "<C-v>jjIXY", std::slice::from_ref(&edit), false),
            want,
            "{edit:?}"
        );
    }
    // An edit undone right after leaves the row as it was.
    for edit in [(7..11, ""), (7..9, "zz")] {
        let got = typed(rows, "<C-v>jjIXY", std::slice::from_ref(&edit), true);
        assert_eq!(got, "aXYbcd\neXYfgh\niXYjkl\nmnop", "{edit:?}");
    }
    // A row taken out with its line break, before a row just like it that
    // isn't the block's: that one gets nothing, empty or not, for I, A and c.
    for (text, column, keys, edit, want) in [
        (
            "abcd\nabcd\nabcd\nabcd",
            1,
            "<C-v>jjIXY",
            12..17,
            "aXYbcd\naXYbcd\nabcd",
        ),
        (
            "abcd\nabcd\nabcd\nabcd",
            1,
            "<C-v>jjAXY",
            12..17,
            "abXYcd\nabXYcd\nabcd",
        ),
        (
            "abcd\nabcd\nabcd\nacd",
            1,
            "<C-v>jjcXY",
            10..14,
            "aXYcd\naXYcd\nacd",
        ),
        (
            "abcd\nabcd\n\n",
            0,
            "<C-v>jjIXY",
            12..13,
            "XYabcd\nXYabcd\n",
        ),
        (
            "abcd\nabcd\n\n",
            0,
            "<C-v>jjAXY",
            12..13,
            "aXYbcd\naXYbcd\n",
        ),
    ] {
        assert_eq!(
            typed((text, column), keys, &[(edit, "")], false),
            want,
            "{keys}"
        );
    }
    // A row put in above a place at a row's start isn't the row's, whether
    // it's empty or not; an empty last row is still there when the row
    // above goes with the line break between them.
    for (text, keys, edit, want) in [
        (
            "abcd\nabcd\nabcd",
            "<C-v>jjI// ",
            (8..8, "new\n"),
            "// abcd\nnew\n// abcd\n// abcd",
        ),
        (
            "abcd\n\nabcd",
            "<C-v>jjI// ",
            (8..8, "new\n"),
            "// abcd\nnew\n// \n// abcd",
        ),
        (
            "abcd\nabcd\nabcd\n",
            "<C-v>3jIXY",
            (12..17, ""),
            "XYabcd\nXYabcd\nXY",
        ),
        (
            "abcd\nabcd\nabcd\n",
            "<C-v>3jAXY",
            (12..17, ""),
            "aXYbcd\naXYbcd\n XY",
        ),
    ] {
        assert_eq!(typed((text, 0), keys, &[edit], false), want, "{keys}");
    }

    // Only what goes in counts toward the limit: `I` pads no row.
    let row = format!("{}x", "\t".repeat(100));
    let mut vim = HeadlessVim::new(&vec![row; 700].join("\n"), (0, 100));
    vim.model_mut().set_tab_size(1000);
    vim.feed("<C-v>699jIZ<Esc>").unwrap();
    assert_eq!(vim.text().matches('Z').count(), 700);
}

#[test]
fn a_put_too_big_changes_nothing() {
    let max = crate::operator::MAX_PUT;
    // After an edit, and a visual extent for the marks.
    let setup = |vim: &mut HeadlessVim| {
        vim.feed("xggylvl<Esc><C-v>j").unwrap();
        assert_eq!(vim.text(), "abc\nabc\nyz");
    };
    // Refused before anything changes: the text, its history, the
    // registers, the visual mode with its selection, and the marks.
    let mut vim = HeadlessVim::new("abc\nabc\nxyz", (2, 0));
    setup(&mut vim);
    let selected = (selections(&vim), vim.cursor(), vim.visual_anchor());
    let marks = (vim.mark('<'), vim.mark('>'));
    vim.feed(&format!("{max}p")).unwrap_err();
    assert_eq!(vim.text(), "abc\nabc\nyz");
    assert_eq!(vim.mode(), "visual_block");
    assert_eq!(
        (selections(&vim), vim.cursor(), vim.visual_anchor()),
        selected
    );
    assert_eq!((vim.mark('<'), vim.mark('>')), marks);
    assert_eq!(vim.registers().get('"'), Some(&Register::new("a", false)));
    // The next `u` undoes the edit before it.
    vim.feed("<Esc>u").unwrap();
    assert_eq!(vim.text(), "abc\nabc\nxyz");

    // A smaller put then replaces the same block, as its own undo step.
    let mut vim = HeadlessVim::new("abc\nabc\nxyz", (2, 0));
    setup(&mut vim);
    vim.feed(&format!("{max}p")).unwrap_err();
    vim.feed("p").unwrap();
    assert_eq!(vim.text(), "aac\naac\nyz");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "abc\nabc\nyz");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), "abc\nabc\nxyz");
}

#[test]
fn a_block_register_too_big_is_refused() {
    // Rows ending before the block take spaces as wide as it, built only
    // for a register: here 4,000 of them under a row 17,000 wide, more than
    // any put could put back. A yank or delete writing that is refused
    // before anything changes; into `_` it writes nothing, and goes ahead.
    let text = format!("{}{}xx", "x".repeat(17_000), "\n".repeat(4_000));
    let mut vim = HeadlessVim::new(&text, (0, 1));
    vim.feed("<C-v>G$").unwrap();
    for keys in ["y", "d", "c", "p"] {
        vim.feed(keys).unwrap_err();
        assert_eq!((vim.mode(), vim.text() == text), ("visual_block", true));
        assert_eq!(registers(&vim), []);
    }
    vim.feed("\"_d").unwrap();
    assert_eq!(vim.text(), format!("x{}x", "\n".repeat(4_000)));
}

#[test]
fn block_typing_survives_a_move_along_its_row() {
    // A move along the block's first row while typing splits the session,
    // as the host does for the cursor keys, but `Esc` still types what's
    // been typed there since `I`, `A` or `c` on the other rows (nvim's
    // result): the text typed after the move is an undo step of its own,
    // with the other rows, and `.` types only it, as `i` does.
    let left = |vim: &mut HeadlessVim, movement| {
        vim.split_session(Splits::Moves, |model| model.move_cursors(movement, false));
    };
    let rows = "abcd\nabcd\nabcd\nabcd";
    for (column, keys, want, cursor) in [
        (1, "<C-v>jIXY", "aXZYbcd\naXZYbcd\nabcd\nabcd", (0, 1)),
        (1, "<C-v>jAXY", "abXZYcd\nabXZYcd\nabcd\nabcd", (0, 1)),
        (1, "<C-v>jlcXY", "aXZYd\naXZYd\nabcd\nabcd", (0, 2)),
    ] {
        let mut vim = HeadlessVim::new(rows, (0, column));
        vim.feed(keys).unwrap();
        left(&mut vim, fe_editor::Movement::Left);
        vim.feed("Z<Esc>").unwrap();
        assert_eq!(
            (vim.text().as_str(), vim.cursor()),
            (want, cursor),
            "{keys}"
        );
    }
    let mut vim = HeadlessVim::new(rows, (0, 1));
    vim.feed("<C-v>jIXY").unwrap();
    left(&mut vim, fe_editor::Movement::Left);
    vim.feed("Z<Esc>jj.").unwrap();
    assert_eq!(vim.text(), "aXZYbcd\naXZYbcd\naZbcd\nabcd");
    vim.feed("uu").unwrap();
    assert_eq!(vim.text(), "aXYbcd\nabcd\nabcd\nabcd");
    vim.feed("u").unwrap();
    assert_eq!(vim.text(), rows);

    // Off the row, it types on no other row.
    let mut vim = HeadlessVim::new(rows, (0, 1));
    vim.feed("<C-v>jIXY").unwrap();
    left(&mut vim, fe_editor::Movement::Down);
    vim.feed("Z<Esc>").unwrap();
    assert_eq!(vim.text(), "aXYbcd\nabcZd\nabcd\nabcd");
}

/// Feeds `keys`, running `<Left>`, `<Right>`, `<Up>`, `<Down>`, `<Home>` and
/// `<End>` as the host runs its cursor keys in insert mode, splitting the
/// session around them ([`HeadlessVim::split_session`]).
fn feed_moving(vim: &mut HeadlessVim, mut keys: &str) {
    use fe_editor::Movement;
    let moves = [
        ("<Left>", Movement::Left),
        ("<Right>", Movement::Right),
        ("<Up>", Movement::Up),
        ("<Down>", Movement::Down),
        ("<Home>", Movement::LineStart),
        ("<End>", Movement::LineEnd),
    ];
    loop {
        let next = moves
            .iter()
            .filter_map(|&(name, movement)| Some((keys.find(name)?, name, movement)))
            .min_by_key(|&(at, ..)| at);
        let Some((at, name, movement)) = next else {
            return vim.feed(keys).unwrap();
        };
        vim.feed(&keys[..at]).unwrap();
        vim.split_session(Splits::Moves, |model| model.move_cursors(movement, false));
        keys = &keys[at + name.len()..];
    }
}

#[test]
fn block_typing_takes_what_its_first_row_grew_by() {
    // `Esc` types on the other rows what the first row grew by, taken from
    // where typing began on it, wherever the cursor keys went meanwhile, if
    // the cursor ends on it (nvim's results): leaving the row and coming back
    // keeps the block, and a move past the typed text or before it takes the
    // text there. `A` takes it from where typing began left of it, at or
    // before the block's left column, `$A` as far left of each row's end,
    // and `I` from where typing began right of it, if nothing went in at it.
    // The cursor goes to where typing began (for `A`, left of the block).
    let rows = "abcd\nabcd\nabcd\nabcd";
    let check = |(text, column): (&str, usize), keys: &str, want: &str, cursor| {
        let mut vim = HeadlessVim::new(text, (0, column));
        feed_moving(&mut vim, keys);
        let rest = text.split('\n').skip(want.split('\n').count());
        let want: Vec<&str> = want.split('\n').chain(rest).collect();
        assert_eq!(
            (vim.text(), vim.cursor()),
            (want.join("\n"), cursor),
            "{keys} over {text:?}"
        );
    };
    for (keys, want, cursor) in [
        // Down and back up.
        ("<C-v>jIXY<Down><Up>Z<Esc>", "aXYZbcd\naXYZbcd", (0, 1)),
        ("<C-v>jAXY<Down><Up>Z<Esc>", "abXYZcd\nabXYZcd", (0, 1)),
        ("<C-v>jlcXY<Down><Up>Z<Esc>", "aXYZd\naXYZd", (0, 3)),
        ("<C-v>j$AXY<Down><Up>Z<Esc>", "abcdXYZ\nabcdXYZ", (0, 1)),
        ("<C-v>jIXY<Down><Up><Esc>", "aXYbcd\naXYbcd", (0, 1)),
        ("<C-v>jI<Down><Up>Z<Esc>", "aZbcd\naZbcd", (0, 1)),
        // Past the typed text.
        ("<C-v>jIXY<Right>Z<Esc>", "aXYbZcd\naXYbbcd", (0, 1)),
        ("<C-v>jIXY<End>Z<Esc>", "aXYbcdZ\naXYbbcd", (0, 1)),
        ("<C-v>jAXY<Right>Z<Esc>", "abXYcZd\nabXYccd", (0, 1)),
        ("<C-v>jlcXY<Right>Z<Esc>", "aXYdZ\naXYdd", (0, 4)),
        ("<C-v>jI<Right><Right>Z<Esc>", "abcZd\nabcZd", (0, 3)),
        ("<C-v>jA<Right>Z<Esc>", "abcZd\nabccd", (0, 1)),
        ("<C-v>jlc<Right>Z<Esc>", "adZ\nadd", (0, 2)),
        // Before it.
        (
            "<C-v>jIXY<Left><Left><Left>Z<Esc>",
            "ZaXYbcd\naaXYbcd",
            (0, 0),
        ),
        (
            "<C-v>jAXY<Left><Left><Left>Z<Esc>",
            "aZbXYcd\naZbXbcd",
            (0, 1),
        ),
        ("<C-v>jAXY<Home>Z<Esc>", "ZabXYcd\nZababcd", (0, 0)),
        ("<C-v>jlcXY<Left><Left><Left>Z<Esc>", "ZaXYd\naaXYd", (0, 0)),
        (
            "<C-v>jlAXY<Left><Left><Left>Z<Esc>",
            "abZcXYd\nabccXYd",
            (0, 1),
        ),
        // Typing began where the session did, a backspace before it aside.
        ("<C-v>jIXY<Left><Left><BS>Z<Esc>", "ZXYbcd\naXYbcd", (0, 1)),
        (
            "<C-v>jAXY<Left><Left><Left><BS>Z<Esc>",
            "ZbXYcd\nabXbcd",
            (0, 1),
        ),
        // Off the row at the end, nothing but for `c`, wherever the caret,
        // which keeps its column on a row typed on.
        ("<C-v>jIXY<Down>Z<Esc>", "aXYbcd\nabcZd", (1, 3)),
        ("<C-v>jlcXY<Down><Esc>", "aXYd\naXYd", (1, 1)),
        (
            "<C-v>jlc\u{65e5}<Down><Esc>",
            "a\u{65e5}d\na\u{65e5}d",
            (1, 1),
        ),
    ] {
        check((rows, 1), keys, want, cursor);
    }
    // A backspace left of `I`'s column keeps it there; the session's own
    // join of the row below leaves what the row grew by unknown, unless the
    // session typed that line break; one typed at the row's end doesn't grow
    // it, and the block's own rows get the text.
    for (text, column, keys, want, cursor) in [
        (
            "cbbadac\neafeea\n\ndeaeba",
            1,
            "<C-v>jjjlI<BS><End>XYXY<Esc>",
            "bbadacXYXY\nebadafeea\n\ndbadeaeba",
            (0, 1),
        ),
        (
            rows,
            1,
            "<C-v>jjIXY<Down><Home><BS><Esc>",
            "aXYbcdabcd\nabcd\nabcd",
            (0, 5),
        ),
        (
            rows,
            1,
            "<C-v>jIX<CR><BS>Y<Esc>",
            "aXYbcd\naXYbcd\nabcd\nabcd",
            (0, 1),
        ),
        (
            rows,
            1,
            "<C-v>jjIXY<End><CR>Q<Home><BS><Esc>",
            "aXYbcdQ\naXYbbcd\naXYbbcd\nabcd",
            (0, 1),
        ),
        (
            rows,
            1,
            "<C-v>jjIXY<End><CR><Up><Esc>",
            "aXYbcd\n\naXYbcd\naXYbcd\nabcd",
            (0, 1),
        ),
        (
            rows,
            3,
            "<C-v>jjAXY<CR><Up><Esc>",
            "abcdXY\n\nabcdXY\nabcdXY\nabcd",
            (0, 3),
        ),
        (
            "  abcdef\n  abcdef\n  abcdef",
            3,
            "<C-v>jjIXY<End><CR><Up><Esc>",
            "  aXYbcdef\n\n  aXYbcdef\n  aXYbcdef",
            (0, 3),
        ),
    ] {
        let mut vim = HeadlessVim::new(text, (0, column));
        feed_moving(&mut vim, keys);
        assert_eq!(
            (vim.text().as_str(), vim.cursor()),
            (want, cursor),
            "{keys}"
        );
    }
    // `A` on a row short of the block, and `$A`.
    let left3 = "AXY<Left><Left><Left>Z<Esc>";
    check(
        ("abcd\na\nabcd", 2),
        &format!("<C-v>jj{left3}"),
        "abZcXYd\na\nabZcXcd",
        (0, 2),
    );
    check(
        ("abcd\nabcdef\nab", 1),
        &format!("<C-v>jj${left3}"),
        "abcZdXY\nabcdeZdXf\nabZdX",
        (0, 1),
    );
    check(
        ("abcd\nabcd", 1),
        "<C-v>j$AXY<Home>Z<Esc>",
        "ZabcdXY\nabcd",
        (0, 0),
    );

    // The text typed after the last split is an undo step of its own, the
    // other rows' with it, and `.` types only it, as `i` does.
    for (keys, undone, cursor) in [
        ("<C-v>jIXY<Down><Up>Z<Esc>", "aXYbcd", (0, 3)),
        ("<C-v>jAXY<Down><Up>Z<Esc>", "abXYcd", (0, 4)),
        ("<C-v>jIXY<Right>Z<Esc>", "aXYbcd", (0, 4)),
    ] {
        let mut vim = HeadlessVim::new(rows, (0, 1));
        feed_moving(&mut vim, keys);
        vim.feed("u").unwrap();
        assert_eq!(vim.text(), format!("{undone}\nabcd\nabcd\nabcd"), "{keys}");
        assert_eq!(vim.cursor(), cursor, "{keys}");
        vim.feed("jj.").unwrap();
        assert_eq!(vim.text(), format!("{undone}\nabcd\nabcZd\nabcd"), "{keys}");
        vim.feed("uu").unwrap();
        assert_eq!(vim.text(), rows, "{keys}");
    }
    let mut vim = HeadlessVim::new(rows, (0, 1));
    feed_moving(&mut vim, "<C-v>jlcXY<Down><Up>Z<Esc>jj.");
    assert_eq!(vim.text(), "aXYZd\naXYZd\nabcZd\nabcd");
    // With nothing typed since the last split, `.` repeats the block's
    // change, whatever `Esc` typed on its rows.
    for (keys, want, cursor) in [
        (
            "<C-v>jIXY<Down><Up><Esc>",
            "aXYbcd\naXYbcd\naXYbcd\naXYbcd",
            (2, 1),
        ),
        (
            "<C-v>jIXY<Left><Esc>",
            "aXYbcd\naXYbcd\naXYbcd\naXYbcd",
            (2, 1),
        ),
        (
            "<C-v>jAXY<Down><Up><Esc>",
            "abXYcd\nabXYcd\nabXYcd\nabXYcd",
            (2, 1),
        ),
        (
            "<C-v>j$AXY<Down><Up><Esc>",
            "abcdXY\nabcdXY\nabcdXY\nabcdXY",
            (2, 1),
        ),
        (
            "<C-v>jlcXY<Down><Up><Esc>",
            "aXYd\naXYd\nabXY\nabXY",
            (2, 3),
        ),
        ("<C-v>jlcXY<Down><Esc>", "aXYd\naXYd\nabcd\naXYd", (3, 2)),
    ] {
        let mut vim = HeadlessVim::new(rows, (0, 1));
        feed_moving(&mut vim, keys);
        vim.feed("jj.").unwrap();
        assert_eq!(
            (vim.text().as_str(), vim.cursor()),
            (want, cursor),
            "{keys}"
        );
    }
}

#[test]
fn block_typing_follows_its_first_row() {
    // Another origin's edit on the first row before the end of what `Esc`
    // takes from it leaves where that is unknown: it types on no other row.
    // Rows put in above it or taken out, and edits past that text, don't
    // matter.
    let ai = || fe_buffer::Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    // Once XY is typed, the first row is 2..8.
    let rows = "x\nabcd\nabcd\nabcd";
    for (edit, typing, want) in [
        ((0..0, "new\n"), "", "new\nx\naXYbcd\naXYbcd\nabcd"),
        ((0..2, ""), "", "aXYbcd\naXYbcd\nabcd"),
        ((2..2, "new\n"), "", "x\nnew\naXYbcd\naXYbcd\nabcd"),
        ((8..8, "KEEP"), "", "x\naXYbcdKEEP\naXYbcd\nabcd"),
        ((5..6, "Q"), "", "x\naXYQcd\naXYbcd\nabcd"),
        ((2..2, "Q"), "", "x\nQaXYbcd\nabcd\nabcd"),
        ((4..4, "Q"), "", "x\naXQYbcd\nabcd\nabcd"),
        ((1..2, ""), "", "xaXYbcd\nabcd\nabcd"),
        // Typing past what another origin put in would take that too.
        ((6..6, "Q"), "<End>ZZ", "x\naXYbQcdZZ\nabcd\nabcd"),
        // Characters of more than one byte where its line breaks are looked
        // for.
        ((2..2, "\u{e9}"), "", "x\n\u{e9}aXYbcd\nabcd\nabcd"),
        ((2..2, "\u{e9}\n"), "", "x\n\u{e9}\naXYbcd\naXYbcd\nabcd"),
        ((0..2, "\u{65e5}\n"), "", "\u{65e5}\naXYbcd\naXYbcd\nabcd"),
        ((0..1, "\u{65e5}"), "", "\u{65e5}\naXYbcd\naXYbcd\nabcd"),
        // The session joining a row another origin put in below it.
        (
            (8..8, "\nNEW"),
            "<Down><Home><BS>",
            "x\naXYbcdNEW\nabcd\nabcd",
        ),
    ] {
        let mut vim = HeadlessVim::new(rows, (1, 1));
        vim.feed("<C-v>jIXY").unwrap();
        let mut split = vim.model().split();
        split.buffer_mut().edit([edit.clone()], ai());
        feed_moving(&mut vim, typing);
        vim.feed("<Esc>").unwrap();
        assert_eq!(vim.text(), want, "{edit:?} then {typing}");
    }
    // The line break above it taken out, after a character of three bytes.
    let mut vim = HeadlessVim::new("\u{65e5}\nabcd\nabcd", (1, 1));
    vim.feed("<C-v>jIXY").unwrap();
    let mut split = vim.model().split();
    split.buffer_mut().edit([(3..4, "")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "\u{65e5}aXYbcd\nabcd");
}

#[test]
fn an_empty_row_is_the_one_its_line_break_ends() {
    // A row another origin puts in at an empty row of the block, before its
    // line break, leaves the row that line break ends, which `I`, `A` and `c`
    // then type nothing on, not knowing where its text went; nor on the row
    // put in. The last row's line break is the end of the text.
    let ai = || fe_buffer::Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    for (text, keys, at, want) in [
        (
            "abcd\n\nabcd\nend",
            "<C-v>jjIXY",
            7,
            "XYabcd\n\nKEEP\nXYabcd\nend",
        ),
        (
            "abcd\n\nabcd\nend",
            "<C-v>jjAXY",
            7,
            "aXYbcd\n\nKEEP\naXYbcd\nend",
        ),
        (
            "abcd\n\nabcd\nend",
            "<C-v>jjcXY",
            6,
            "XYbcd\n\nKEEP\nXYbcd\nend",
        ),
        ("abcd\nabcd\n", "<C-v>jjIXY", 12, "XYabcd\nXYabcd\n\nKEEP"),
        ("abcd\nabcd\n", "<C-v>jjAXY", 12, "aXYbcd\naXYbcd\n\nKEEP"),
        ("abcd\nabcd\n", "<C-v>jjcXY", 10, "XYbcd\nXYbcd\n\nKEEP"),
    ] {
        let mut vim = HeadlessVim::new(text, (0, 0));
        vim.feed(keys).unwrap();
        let mut split = vim.model().split();
        split.buffer_mut().edit([(at..at, "\nKEEP")], ai());
        vim.feed("<Esc>").unwrap();
        assert_eq!(vim.text(), want, "{keys} over {text:?}");
    }
}

#[test]
fn black_hole_put_over_lines_keeps_a_row() {
    // `"_` holds empty characters, not nothing: over lines, `p` and `P`
    // leave an empty row, and `p` still keeps what it replaced.
    for (keys, want, cursor) in [
        ("ggV\"_p", "\ndef\nghi", (0, 0)),
        ("ggjV\"_p", "abc\n\nghi", (1, 0)),
        ("GV\"_p", "abc\ndef\n", (2, 0)),
        ("ggVG\"_p", "", (0, 0)),
        ("ggV\"_P", "\ndef\nghi", (0, 0)),
        ("GV\"_P", "abc\ndef\n", (2, 0)),
        // A count puts each copy on a row of its own.
        ("ggjV2\"_p", "abc\n\n\nghi", (1, 0)),
    ] {
        let mut vim = HeadlessVim::new("abc\ndef\nghi", (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(
            (vim.text().as_str(), vim.cursor()),
            (want, cursor),
            "{keys}"
        );
    }
    let mut vim = HeadlessVim::new("abc\ndef\nghi", (1, 0));
    vim.feed("V\"_p").unwrap();
    assert_eq!(
        registers(&vim),
        [('"', "def\n".into(), true), ('1', "def\n".into(), true)]
    );
}

#[test]
fn a_put_over_lines_undoes_to_its_first_row() {
    // With nothing in the register, lines at the end go with the line break
    // before them; undo still puts the cursor on the first of them, and redo
    // on the row now last, as nvim does. The cursor on the first of several
    // keeps its column.
    for (keys, undone, redone) in [
        ("GVp", (2, 0), (1, 0)),
        ("GVP", (2, 0), (1, 0)),
        ("jVGp", (1, 0), (0, 0)),
        ("jVGP", (1, 0), (0, 0)),
        ("GllVkp", (1, 2), (0, 2)),
        ("GllVk\"_p", (1, 2), (1, 0)),
        ("GllVggP", (0, 2), (0, 0)),
        ("jllV\"_p", (1, 0), (1, 0)),
        ("jllVj\"_p", (1, 0), (1, 0)),
    ] {
        let mut vim = HeadlessVim::new("abc\ndef\nghi", (0, 1));
        vim.feed(keys).unwrap();
        vim.feed("u").unwrap();
        assert_eq!(
            (vim.text().as_str(), vim.cursor()),
            ("abc\ndef\nghi", undone),
            "{keys}"
        );
        vim.feed("<C-r>").unwrap();
        assert_eq!(vim.cursor(), redone, "{keys}");
    }
}
