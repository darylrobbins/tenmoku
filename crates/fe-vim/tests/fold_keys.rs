//! Vim's fold-making keys and fold motions over fastedit's folds: `zf`
//! over a motion, object or visual selection and `{count}zF` close a fold
//! over rows with no undo step; `zd`, `zD` and `zE` delete folds (open
//! them); `zj` and `zk` go to the next candidate's first row and the
//! previous one's last, and work under an operator (`dzj`).
//!
//! The host is headless, with no fold ranges: the candidates are the
//! buffer's indented blocks, each headed by the row above it.

use fe_vim::HeadlessVim;

/// Two functions and a tail, a blank row between the first two. The
/// candidates are rows 0 to 2, 5 to 9 and, nested in it, 7 to 8.
const TEXT: &str = "\
fn a() {
    one
    two
}

fn b() {
    three
    if x {
        four
    }
}
tail";

fn editor(cursor: (u32, usize)) -> HeadlessVim {
    HeadlessVim::new(TEXT, cursor)
}

/// The closed folds, by their first and last rows.
fn folds(vim: &HeadlessVim) -> Vec<(u32, u32)> {
    let mut folds: Vec<_> = (vim.model().display().folds().iter())
        .map(|fold| (fold.start, fold.end))
        .collect();
    folds.sort();
    folds
}

/// `keys` from `cursor`, after an `x` on the last row: the folds they leave
/// and the caret. They change no text, and `u` then undoes the `x`.
fn folding(cursor: (u32, usize), keys: &str) -> (Vec<(u32, u32)>, (u32, usize)) {
    let mut vim = editor((11, 0));
    vim.feed("x").unwrap();
    let typed = vim.text();
    let offset = vim.model().text().row_start(cursor.0) + cursor.1;
    vim.model_mut().set_cursor(offset);
    vim.feed(keys).unwrap();
    assert!(!vim.state().failed(), "{keys}");
    assert_eq!(vim.text(), typed, "{keys} changes no text");
    assert_eq!(vim.mode(), "normal", "{keys}");
    let (made, caret) = (folds(&vim), vim.cursor());
    vim.feed("u").unwrap();
    assert_eq!(
        vim.text(),
        TEXT,
        "{keys} left no undo step: `u` undid the `x`"
    );
    (made, caret)
}

#[test]
fn fold_keys_make_and_remove_folds() {
    // `zfj` takes the caret's row and the next; the caret stays put.
    assert_eq!(folding((1, 6), "zfj"), (vec![(1, 2)], (1, 6)));
    // Upward, from the row above at the same column.
    assert_eq!(folding((2, 6), "zfk"), (vec![(1, 2)], (1, 6)));
    // A count on either side.
    assert_eq!(folding((6, 4), "2zfj"), (vec![(6, 8)], (6, 4)));
    assert_eq!(folding((6, 4), "zf2j"), (vec![(6, 8)], (6, 4)));
    // An object: the paragraph's rows.
    assert_eq!(folding((1, 4), "zfip"), (vec![(0, 3)], (0, 4)));
    // An exclusive motion ending at column 0 stops at the row before.
    assert_eq!(folding((0, 0), "zf}"), (vec![(0, 3)], (0, 0)));
    // `{count}zF`: `count` rows from the caret's.
    assert_eq!(folding((5, 3), "3zF"), (vec![(5, 7)], (5, 3)));
    // Visual `zf`, linewise or charwise, over the selection's rows, the
    // caret at its start.
    assert_eq!(folding((6, 4), "Vjjzf"), (vec![(6, 8)], (6, 0)));
    assert_eq!(folding((8, 8), "vkkzf"), (vec![(6, 8)], (6, 8)));
    // An upper endpoint past a short row's last character: the caret lands
    // on that character, not the newline.
    assert_eq!(folding((6, 8), "vkzf"), (vec![(5, 6)], (5, 7)));
    // One row hides nothing, so makes no fold.
    assert_eq!(folding((6, 4), "zfl"), (vec![], (6, 4)));
    assert_eq!(folding((6, 4), "zF"), (vec![], (6, 4)));

    // `.` doesn't repeat `zf`: it repeats the change before it.
    let mut vim = editor((11, 0));
    vim.feed("x2Gzfj.").unwrap();
    assert_eq!(folds(&vim), [(1, 2)]);
    assert!(vim.text().contains("\n   one\n"));

    // `zf` on rows already folded: the fold over them takes the inner one in.
    let mut vim = editor((7, 4));
    vim.feed("zfj").unwrap();
    assert_eq!(folds(&vim), [(7, 8)]);
    vim.feed("kzf3j").unwrap();
    assert_eq!(folds(&vim), [(6, 9), (7, 8)]);

    // Nested closed folds, and one elsewhere.
    let nested = || {
        let mut vim = editor((0, 0));
        vim.model_mut().fold_rows(0..=2);
        vim.model_mut().fold_rows(7..=8);
        vim.model_mut().fold_rows(5..=9);
        let offset = vim.model().text().row_start(5);
        vim.model_mut().set_cursor(offset);
        assert_eq!(folds(&vim), [(0, 2), (5, 9), (7, 8)]);
        vim
    };
    // `zd` deletes the fold at the cursor, the one shown: what it held
    // stays closed.
    let mut vim = nested();
    vim.feed("zd").unwrap();
    assert_eq!(folds(&vim), [(0, 2), (7, 8)]);
    assert_eq!(vim.cursor(), (5, 0));
    assert_eq!(vim.text(), TEXT);
    vim.feed("2jzd").unwrap();
    assert_eq!(folds(&vim), [(0, 2)]);
    // With no fold at the cursor it does nothing.
    vim.feed("zd").unwrap();
    assert_eq!(folds(&vim), [(0, 2)]);
    // `zD` every fold nested there too.
    let mut vim = nested();
    vim.feed("zD").unwrap();
    assert_eq!(folds(&vim), [(0, 2)]);
    assert_eq!(vim.cursor(), (5, 0));
    // `zE` every fold.
    let mut vim = nested();
    vim.feed("zE").unwrap();
    assert_eq!(folds(&vim), []);
    assert_eq!(vim.text(), TEXT);
}

/// `keys` from `cursor`: where the caret goes, and whether the key failed.
fn moving(cursor: (u32, usize), keys: &str) -> ((u32, usize), bool) {
    let mut vim = editor(cursor);
    vim.feed(keys).unwrap();
    assert_eq!(vim.text(), TEXT, "{keys}");
    (vim.cursor(), vim.state().failed())
}

/// `keys` from `cursor`: the text, the caret, and the unnamed register's
/// text and whether it's linewise.
fn deleting(cursor: (u32, usize), keys: &str) -> (String, (u32, usize), String, bool) {
    let mut vim = editor(cursor);
    vim.feed(keys).unwrap();
    let register = vim.registers().get('"').unwrap();
    (
        vim.text(),
        vim.cursor(),
        register.text.clone(),
        register.linewise,
    )
}

/// `TEXT` without `rows`.
fn without(rows: std::ops::RangeInclusive<usize>) -> String {
    let mut lines: Vec<&str> = TEXT.lines().collect();
    lines.drain(rows);
    lines.join("\n")
}

#[test]
fn fold_motions_move_between_folds() {
    // `zj`: the next candidate's first row, nested or not, at column 0.
    assert_eq!(moving((0, 3), "zj"), ((5, 0), false));
    assert_eq!(moving((3, 0), "zj"), ((5, 0), false));
    assert_eq!(moving((5, 2), "zj"), ((7, 0), false));
    assert_eq!(moving((0, 0), "2zj"), ((7, 0), false));
    // A count past the last goes as far as there are.
    assert_eq!(moving((0, 0), "9zj"), ((7, 0), false));
    // None below: it fails without moving.
    assert_eq!(moving((7, 4), "zj"), ((7, 4), true));
    assert_eq!(moving((11, 2), "3zj"), ((11, 2), true));
    // `zk`: the previous candidate's last row.
    assert_eq!(moving((11, 2), "zk"), ((9, 0), false));
    assert_eq!(moving((9, 4), "zk"), ((8, 0), false));
    assert_eq!(moving((11, 0), "2zk"), ((8, 0), false));
    assert_eq!(moving((11, 0), "3zk"), ((2, 0), false));
    assert_eq!(moving((11, 0), "9zk"), ((2, 0), false));
    assert_eq!(moving((2, 4), "zk"), ((2, 4), true));
    assert_eq!(moving((0, 0), "2zk"), ((0, 0), true));
    // In a visual mode they move the cursor.
    let mut vim = editor((0, 2));
    vim.feed("vzjd").unwrap();
    assert_eq!(
        vim.text(),
        TEXT.replacen("fn a() {\n    one\n    two\n}\n\nf", "fn", 1)
    );

    // A closed fold counts as one: the candidate nested in it is hidden.
    let mut vim = editor((0, 0));
    vim.model_mut().fold_rows(5..=9);
    vim.feed("zj").unwrap();
    assert_eq!(vim.cursor(), (5, 0));
    vim.feed("zj").unwrap();
    assert!(vim.state().failed());
    vim.feed("Gzk").unwrap();
    assert_eq!(vim.cursor(), (5, 0));
    vim.feed("zk").unwrap();
    assert_eq!(vim.cursor(), (2, 0));
    // And a fold `zf` made counts though it's no candidate.
    let mut vim = editor((11, 0));
    vim.feed("kkzfj").unwrap();
    vim.feed("ggzjzjzj").unwrap();
    assert_eq!(vim.cursor(), (9, 0));

    // `dzj` is exclusive and charwise. From the first non-blank (or
    // before) it takes whole rows, up to the one before the candidate's
    // first.
    assert_eq!(
        deleting((1, 4), "dzj"),
        (
            without(1..=4),
            (1, 0),
            "    one\n    two\n}\n\n".into(),
            true
        )
    );
    assert_eq!(
        deleting((0, 0), "d2zj"),
        (without(0..=6), (0, 4), without(7..=11) + "\n", true)
    );
    // From further along, the rest of the row and the rows up to it.
    assert_eq!(
        deleting((1, 6), "dzj"),
        (
            TEXT.replacen("e\n    two\n}\n", "", 1),
            (1, 5),
            "e\n    two\n}\n".into(),
            false
        )
    );
    // `dzk` from the middle of a row: back to the candidate's last row's
    // start.
    assert_eq!(
        deleting((11, 2), "dzk"),
        (
            without(9..=11) + "\nil",
            (9, 0),
            "    }\n}\nta".into(),
            false
        )
    );
    // From column 0, the rows before.
    assert_eq!(
        deleting((11, 0), "dzk"),
        (without(9..=10), (9, 0), "    }\n}\n".into(), true)
    );
    // A failed `zj` deletes nothing.
    let mut vim = editor((7, 4));
    vim.feed("dzj").unwrap();
    assert_eq!(vim.text(), TEXT);
    assert!(vim.state().failed());
    // `.` repeats it.
    let mut vim = editor((0, 0));
    vim.feed("dzj.").unwrap();
    assert_eq!(vim.text(), without(0..=6));
    // `yzj` takes the same rows, the caret staying.
    let mut vim = editor((1, 4));
    vim.feed("yzj").unwrap();
    assert_eq!(vim.cursor(), (1, 4));
    assert_eq!(
        vim.registers().get('"').unwrap().text,
        "    one\n    two\n}\n\n"
    );
    // Under an operator `z` takes only `j` and `k`.
    let mut vim = editor((1, 4));
    vim.feed("dzt").unwrap();
    assert_eq!(vim.text(), TEXT);
    assert_eq!(vim.mode(), "normal");
    vim.feed("x").unwrap();
    assert!(vim.text().contains("\n    ne\n"));
}
