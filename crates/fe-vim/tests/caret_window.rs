//! A motion or text object from the caret reads a bounded window of its row,
//! not the row: on a 100 MB row they cost what they do on a short one, and
//! land where they do on a short row with the same text around the caret,
//! including where the window's edge falls inside a grapheme that joins
//! across it (a Prepend char before ASCII, regional indicators, a ZWJ
//! sequence).

use std::time::{Duration, Instant};

use fe_vim::HeadlessVim;
use unicode_segmentation::UnicodeSegmentation;

/// Roughly how far the window reaches each side of the caret
/// (`CARET_WINDOW` in motion.rs); rows more than four times as long are read
/// in windows.
const REACH: usize = 16 * 1024;

/// What a key may take on the huge row: the row's copy and segmentation
/// alone took far longer than this in release, and in debug builds.
fn limit() -> Duration {
    match cfg!(debug_assertions) {
        true => Duration::from_millis(1000),
        false => Duration::from_millis(50),
    }
}

/// What keys left: the cursor's column, the row's length and the unnamed
/// register.
type Outcome = (usize, usize, String);

fn run(row: &str, caret: usize, keys: &str) -> (Outcome, Duration) {
    let mut vim = HeadlessVim::new(row, (0, caret));
    let started = Instant::now();
    vim.feed(keys).unwrap();
    let took = started.elapsed();
    let register = vim
        .registers()
        .get('"')
        .map(|register| register.text.clone())
        .unwrap_or_default();
    let col = vim.cursor().1;
    ((col, vim.text().len(), register), took)
}

#[test]
fn keys_from_the_middle_of_a_100_mb_row_land_as_on_a_short_row() {
    let local = "xx foo bar(baz qux) quux, wib wob; end xx";
    // The caret on the `a` of `bar`.
    let at = local.find("bar").unwrap() + 1;
    let (short_pad, huge_pad) = ("xx ".repeat(40), "xx ".repeat(33_333_333));
    let short = format!("{short_pad}{local}{short_pad}");
    let huge = format!("{huge_pad}{local}{huge_pad}");
    assert!(huge.len() > 99_000_000);
    let (short_at, huge_at) = (short_pad.len() + at, huge_pad.len() + at);
    let shift = huge_pad.len() - short_pad.len();
    for keys in [
        "w",
        "3w",
        "b",
        "2b",
        "e",
        "3e",
        "ge",
        "2ge",
        "W",
        "B",
        "E",
        "fq",
        "fb",
        "Fo",
        "tq",
        "Tf",
        "f(%",
        "l%",
        "yiw",
        "yaw",
        "x",
        "dw",
        "db",
        "dge",
        "d2e",
        "diw",
        "daw",
        "ciwz<Esc>",
    ] {
        let (want, _) = run(&short, short_at, keys);
        let (got, took) = run(&huge, huge_at, keys);
        assert_eq!(
            (got.0 - shift, got.2),
            (want.0, want.2),
            "{keys:?} from the middle of a huge row"
        );
        assert!(took < limit(), "{keys:?} took {took:?}");
    }
}

/// A place where a window's edge may fall: units of `a ` up to `edge` bytes
/// (one more space if it's odd), then `middle`.
fn padded(edge: usize, middle: &str) -> String {
    let mut pad = "a ".repeat(edge / 2);
    if edge % 2 == 1 {
        pad.push(' ');
    }
    pad + middle
}

/// Each sequence in a row of the same text as a short one, but long enough
/// that it is read in windows, with the sequence `REACH` bytes (give or take
/// its length) from the caret so that the window's edge falls somewhere in
/// it. Keys that go over the edge land, and take, as on the short row.
#[test]
fn graphemes_joining_across_the_windows_edge_stay_whole() {
    let sequences = [
        // A Prepend char joins the ASCII after it (GB9b).
        "\u{600}\u{600}\u{600}ab c",
        // Regional indicators pair up from the run's start.
        "\u{1f1e6}\u{1f1e7}\u{1f1e8}\u{1f1e9}\u{1f1ea} z",
        // A ZWJ sequence, and one with a modifier and a variation selector.
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466} z",
        "\u{1f44d}\u{1f3fd} a\u{301}\u{301} \u{2764}\u{fe0f}\u{200d}\u{1f525} z",
    ];
    let tail = "a b ".repeat(REACH);
    for middle in sequences {
        for edge in REACH - middle.len() - 2..REACH + 6 {
            let short = padded(edge, middle) + "end " + &"a b ".repeat(8);
            let long = padded(edge, middle) + "end " + &tail;
            assert!(long.len() > 4 * REACH);
            let pad = padded(edge, "");
            let (words, graphemes) = (pad.split_whitespace().count(), pad.len());
            let keys = [
                format!("{}lx", graphemes + 1),
                format!("{}lx", graphemes + 2),
                format!("{}lx", graphemes + 3),
                format!("{}w", words + 2),
                format!("{words}wx"),
                format!("{words}wyiw"),
                format!("{words}wyaw"),
                format!("{words}wd2w"),
                format!("{}e", words + 1),
            ];
            for keys in keys {
                let (want, _) = run(&short, 0, &keys);
                let (got, _) = run(&long, 0, &keys);
                assert_eq!(
                    (got.0, got.2),
                    (want.0, want.2),
                    "{keys:?} over {middle:?} at {edge}"
                );
            }
        }
    }
}

/// The same from the other side: keys that go back across the window's
/// left edge.
#[test]
fn graphemes_joining_across_the_windows_left_edge_stay_whole() {
    let sequences = [
        "\u{600}\u{600}ab c",
        "\u{1f1e6}\u{1f1e7}\u{1f1e8}\u{1f1e9}\u{1f1ea} z",
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} z",
    ];
    let head = "a b ".repeat(REACH);
    for middle in sequences {
        for edge in REACH - 4..REACH + middle.len() + 4 {
            let after = padded(edge, "");
            let short = format!("{}{middle} {after}", "a b ".repeat(8));
            let long = format!("{head}{middle} {after}");
            assert!(long.len() > 4 * REACH);
            let words = after.split_whitespace().count();
            // Back over the pad and the sequence's graphemes.
            let over = after.len() + 1 + middle.graphemes(true).count();
            let keys = [
                format!("{}hx", over - 1),
                format!("{over}hx"),
                format!("{}hx", over + 1),
                format!("{words}b"),
                format!("{words}bx"),
                format!("{words}byiw"),
                format!("{words}ge"),
            ];
            for keys in keys {
                let (want, _) = run(&short, short.len() - 1, &keys);
                let (got, _) = run(&long, long.len() - 1, &keys);
                // Columns count from the end, which both rows share.
                let (want_col, got_col) = (want.1 - want.0, got.1 - got.0);
                assert_eq!(
                    (got_col, got.2),
                    (want_col, want.2),
                    "{keys:?} over {middle:?} at {edge}"
                );
            }
        }
    }
}

/// What `keys` leave of `rows` fed from `caret` (a row and byte column): the
/// text, the cursor and the unnamed register.
fn leave(rows: &str, caret: (u32, usize), keys: &str) -> (String, (u32, usize), String) {
    let mut vim = HeadlessVim::new(rows, caret);
    vim.feed(keys).unwrap();
    let register = vim
        .registers()
        .get('"')
        .map(|register| register.text.clone())
        .unwrap_or_default();
    (vim.text(), vim.cursor(), register)
}

/// Panics, saying where, if two outcomes differ.
fn assert_same(
    got: &(String, (u32, usize), String),
    want: &(String, (u32, usize), String),
    why: &str,
) {
    let first = |a: &str, b: &str| {
        let at = a.bytes().zip(b.bytes()).position(|(a, b)| a != b);
        (at.unwrap_or(a.len().min(b.len())), a.len(), b.len())
    };
    assert_eq!(got.1, want.1, "cursor: {why}");
    assert!(
        got.2 == want.2,
        "register (first difference, lengths) {:?}: {why}",
        first(&got.2, &want.2)
    );
    // The long row's text goes on past the short one's.
    let keep = got.0.len().min(want.0.len()) - 12;
    let (a, b) = (&got.0[..keep], &want.0[..keep]);
    assert!(
        a == b,
        "text (first difference, lengths) {:?}: {why}",
        first(a, b)
    );
}

/// `short` and `long` (the same text round a pad of `edge` bytes, the long
/// one on to over four reaches) give the same for each of `keys` from the
/// start, for each edge near the window's.
fn same_near_the_edge(
    local: &str,
    edges: std::ops::Range<usize>,
    keys: impl Fn(usize) -> Vec<String>,
) {
    let tail = "a b ".repeat(REACH);
    for edge in edges {
        let pad = padded(edge, "");
        let short = format!("{pad}{local}end a b a b");
        let long = format!("{pad}{local}end {tail}");
        assert!(long.len() > 4 * REACH);
        for keys in keys(edge) {
            let (want, got) = (leave(&short, (0, 0), &keys), leave(&long, (0, 0), &keys));
            assert_same(&got, &want, &format!("{keys:?} at {edge}"));
        }
    }
}

#[test]
fn number_increment_finds_its_number_past_the_windows_edge() {
    same_near_the_edge("7 -19 0x0f ", REACH - 3..REACH + 9, |_| {
        ["<C-a>", "5<C-a>", "<C-x>", "w<C-a>", "2w<C-a>", "3w<C-x>"]
            .map(String::from)
            .to_vec()
    });
}

#[test]
fn sentence_objects_and_motions_go_over_the_windows_edge() {
    let tail = "Ab cd. ".repeat(REACH);
    for edge in REACH - 1..REACH + 4 {
        let pad = "Ab cd. ".repeat(edge / 7);
        let short = format!("{pad}Zz yy. Ab. Cd ee.  Ff.");
        let long = format!("{pad}Zz yy. Ab. Cd ee.  Ff. {tail}");
        let n = edge / 7;
        for keys in [
            format!("{n})"),
            format!("{})", n + 1),
            format!("{})b", n + 2),
            format!("y{n}as"),
            format!("{n})yis"),
            format!("{n})yas"),
            format!("{n})y2is"),
            format!("{n})d2as"),
            format!("{n})("),
        ] {
            let (want, got) = (leave(&short, (0, 0), &keys), leave(&long, (0, 0), &keys));
            assert_same(&got, &want, &format!("{keys:?} at {edge}"));
        }
    }
}

#[test]
fn counted_word_objects_go_over_the_windows_edge() {
    same_near_the_edge("p q  r ", REACH - 3..REACH + 9, |edge| {
        let n = edge / 2;
        [
            format!("y{n}iw"),
            format!("y{}iw", n + 2),
            format!("y{}aw", n / 2 + 1),
            format!("d{}aw", n / 2 + 2),
            format!("{n}wy3iw"),
            format!("{n}wy2aw"),
            format!("{n}wd3aw"),
        ]
        .to_vec()
    });
}

#[test]
fn counted_replace_and_toggle_case_go_over_the_windows_edge() {
    same_near_the_edge("Mid dle ", REACH - 3..REACH + 9, |edge| {
        [
            format!("{edge}rx"),
            format!("{}rx", edge + 4),
            format!("{edge}~"),
            format!("{}~", edge + 6),
            format!("{}g~l", edge),
            format!("{edge}Rxy<Esc>"),
        ]
        .to_vec()
    });
}

#[test]
fn a_visual_counted_l_goes_over_the_windows_edge() {
    same_near_the_edge("Mid dle ", REACH - 1..REACH + 3, |edge| {
        [
            format!("v{edge}ly"),
            format!("v{}ly", edge + 3),
            format!("v{}lhd", edge + 2),
            format!("v{edge}lo{}hy", edge / 2),
        ]
        .to_vec()
    });
}

#[test]
fn a_visual_cursor_keeps_its_start_and_goal_column_on_a_huge_row() {
    // Rows of 70,000 bytes are read in windows; the cursor is 40,000 in.
    let (top, bottom) = ("a".repeat(70_000), "b".repeat(69_000));
    let rows = format!("{top}\n{bottom}\ntail");
    let col = 40_000;
    for (keys, want) in [
        ("vjy", format!("{}\n{}", &top[col..], &bottom[..=col])),
        ("vjjy", format!("{}\n{}\ntail", &top[col..], bottom)),
        ("vjhy", format!("{}\n{}", &top[col..], &bottom[..col])),
        ("v$y", format!("{}\n", &top[col..])),
    ] {
        let (_, _, register) = leave(&rows, (0, col), keys);
        assert_eq!(register.len(), want.len(), "{keys:?}");
        assert!(register == want, "{keys:?}");
    }
    // From the row below, up.
    let (_, _, register) = leave(&rows, (1, col), "vky");
    assert!(register == format!("{}\n{}", &top[col..], &bottom[..=col]));
    // A block, which keeps its columns by display.
    let (_, _, register) = leave(&rows, (0, col), "<C-v>jly");
    assert_eq!(
        register,
        format!("{}\n{}", &top[col..=col + 1], &bottom[col..=col + 1])
    );
}

#[test]
fn a_mark_jump_onto_another_long_row_lands_where_it_does_on_a_short_one() {
    // Row 1 is long, and indented past a window.
    for (indent, tail) in [(20_000, 60_000), (REACH - 1, REACH * 4), (3, 80_000)] {
        let row = format!("{}x{}", " ".repeat(indent), "y".repeat(tail));
        let rows = format!("top\n{row}\nend");
        for (keys, want) in [
            ("jllmakgg'a", (1, indent)),
            ("jllmakgg`a", (1, 2)),
            ("jlllllmakgg`a", (1, 5)),
            ("j$makgg`a", (1, indent + tail)),
            ("j$makgg'a", (1, indent)),
        ] {
            let (_, cursor, _) = leave(&rows, (0, 0), keys);
            assert_eq!(cursor, want, "{keys:?} with {indent} blanks");
        }
    }
}

#[test]
fn percent_beside_a_multibyte_character_on_a_huge_row_does_not_panic() {
    // Brackets and multibyte chars, the match farther than a window.
    let far = "é ".repeat(REACH);
    for (open, close) in [("(", ")"), ("[", "]"), ("{", "}")] {
        let rows = format!(
            "é{open}é{far}é{close}é{} and é{} more",
            "ü".repeat(3),
            "a ".repeat(REACH)
        );
        let end = rows.find(close).unwrap();
        let start = rows.find(open).unwrap();
        for (caret, want) in [
            (0, end),
            (start, end),
            (start + 1, start),
            (end, start),
            (end + 3, start),
        ] {
            let (_, cursor, _) = leave(&rows, (0, caret), "%");
            // To the match of the first bracket at or after the caret; past
            // the closer, none.
            if caret > end {
                assert_eq!(cursor, (0, caret), "{caret}");
            } else {
                assert_eq!(cursor, (0, want), "{caret}");
            }
        }
    }
    // Before a long run of multibyte chars, and with no bracket at all.
    let rows = format!("{}x", "é".repeat(REACH * 4));
    let (_, cursor, _) = leave(&rows, (0, 0), "%");
    assert_eq!(cursor, (0, 0));
}

#[test]
fn percent_over_a_huge_row_matches_across_span_edges_and_quotes() {
    let inner = "a ".repeat(REACH * 3);
    let rows = format!("({inner})  tail {}", "b ".repeat(REACH));
    let close = 1 + inner.len();
    assert_eq!(leave(&rows, (0, 0), "%").1, (0, close));
    assert_eq!(leave(&rows, (0, close), "%").1, (0, 0));
    // From inside, the first bracket after the caret is the closer.
    assert_eq!(leave(&rows, (0, 5), "%").1, (0, 0));
    // Quotes make a bracket in a string not count, as on a short row: the
    // state runs from the row's start.
    let quoted = format!("\"( \" x ({inner}\") \"){}", " c".repeat(REACH * 2));
    let at = quoted.find("x ").unwrap() + 2;
    let want = quoted.find("\") \")").unwrap() + 4;
    assert_eq!(leave(&quoted, (0, at), "%").1, (0, want));
    assert_eq!(leave(&quoted, (0, 0), "%").1, (0, 0));
    // A quote that never closes, so none count.
    let odd = format!("\"( x {inner}) y{}", " c".repeat(REACH * 2));
    let close = odd.find(") y").unwrap();
    assert_eq!(leave(&odd, (0, 1), "%").1, (0, close));
}

/// A visual cursor past a row's last character (after `$`) searches for an
/// unmatched bracket from there, not from the character it is shown on
/// (probed with `nvim --clean --headless`).
#[test]
fn an_unmatched_bracket_motion_in_visual_mode_starts_where_the_cursor_is() {
    for (row, keys, want) in [
        // The `)` before the cursor closes the `(`: nothing is unmatched.
        ("(x)", "v$[(", 3),
        ("(x)", "v$])", 3),
        ("x(", "v$[(", 1),
        ("x(", "v$])", 2),
        ("a(b", "v$[(", 1),
        ("a)b", "v$])", 3),
    ] {
        let mut vim = HeadlessVim::new(row, (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.cursor(), (0, want), "{keys:?} on {row:?}");
    }
}

/// A WORD that is longer than a window, between short words, with the caret
/// in its middle: `iW`, `aW` and `cW` take all of it.
#[test]
fn big_word_objects_and_changes_take_a_word_longer_than_the_window() {
    let big = "a.a.".repeat(REACH);
    let (head, tail) = ("w ".repeat(REACH), " z ".repeat(REACH));
    let row = format!("{head}{big}{tail}");
    let start = head.len();
    for caret in [
        start,
        start + 3,
        start + big.len() / 2,
        start + big.len() - 1,
    ] {
        for (keys, want) in [
            ("yiW", big.clone()),
            ("yaW", format!("{big} ")),
            ("diW", big.clone()),
            ("daW", format!("{big} ")),
            ("y2iW", format!("{big} ")),
        ] {
            let (text, _, register) = leave(&row, (0, caret), keys);
            assert!(register == want, "{keys:?} from {caret}");
            if keys.starts_with('d') {
                assert_eq!(text.len(), row.len() - want.len(), "{keys:?} from {caret}");
            }
        }
        let (text, _, _) = leave(&row, (0, caret), "cWq<Esc>");
        assert_eq!(
            text,
            format!("{head}{}q{tail}", &big[..caret - start]),
            "cW from {caret}"
        );
    }
}

/// Columns are the row's own, not the window's: an operator's goal, a
/// block put's column, and a tab's width, far into rows read in windows.
#[test]
fn columns_far_along_a_huge_row_are_the_rows_own() {
    for unit in ["a b ", "a\t", "ab\t "] {
        let row = unit.repeat(70_000 / unit.len());
        let rows = format!("{row}\n{row}\n{row}");
        let col = 40_000 - 40_000 % unit.len();
        for (from, keys, want) in [
            ((0, col), "j", (1, col)),
            ((2, col), "kk", (0, col)),
            ((0, col), "jj", (2, col)),
            // The caret keeps its column over a linewise operator.
            ((0, col), "dj", (0, col)),
            ((0, col), "dd", (0, col)),
            ((1, col), "dk", (0, col)),
            // A block puts at the caret's display column.
            ((0, col), "<C-v>jlyP", (0, col)),
            ((0, col), "<C-v>jlyjP", (1, col)),
        ] {
            let (_, cursor, _) = leave(&rows, from, keys);
            assert_eq!(cursor, want, "{keys:?} over {unit:?}");
        }
        // The put block lands in the same columns on both rows.
        let (text, _, register) = leave(&rows, (0, col), "<C-v>jly2jP");
        let block = register.replace('\n', "");
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(
            &lines[2][col..col + block.len() / 2],
            &row[col..col + block.len() / 2]
        );
    }
}

/// A WORD that is the whole of a huge row, with no blank anywhere: `iW` and
/// `aW` take all of it, though the window holds only part.
#[test]
fn a_whole_row_word_with_no_blank_is_taken_whole() {
    let row = "a.a.".repeat(REACH * 3);
    for caret in [0, 7, row.len() / 2, row.len() - 1] {
        for (keys, want) in [
            ("yiW", row.clone()),
            ("yaW", row.clone()),
            ("diW", row.clone()),
        ] {
            let (text, _, register) = leave(&row, (0, caret), keys);
            assert!(register == want, "{keys:?} from {caret}");
            assert_eq!(
                text.len(),
                if keys == "yiW" || keys == "yaW" {
                    row.len()
                } else {
                    0
                }
            );
        }
    }
}

/// A preprocessor conditional on a huge row after more blanks than a window
/// holds, or with blanks between its `#` and keyword.
#[test]
fn percent_finds_a_directive_after_long_blanks_on_a_huge_row() {
    let blanks = " ".repeat(REACH * 2);
    let tail = " x".repeat(REACH * 2);
    for row in [
        format!("{blanks}#endif{tail}"),
        format!("#{blanks}endif{tail}"),
        format!("  #  {blanks}endif{tail}"),
    ] {
        let rows = format!("#if A\nfoo\n{row}\nbar");
        let (_, cursor, _) = leave(&rows, (2, 0), "%");
        assert_eq!(cursor, (0, 0), "{:?}", &row[..10]);
        let (_, cursor, _) = leave(&rows, (0, 0), "%");
        assert_eq!(cursor.0, 2, "{:?}", &row[..10]);
    }
}

/// `cW` over a WORD that is the whole of a huge row with no blank in it.
#[test]
fn change_big_word_takes_a_whole_row_word_with_no_blank() {
    // Longer than a window and the room it looks in for a blank.
    let row = "a.a.".repeat(REACH * 10);
    for caret in [0, 5, row.len() / 2] {
        let (text, _, _) = leave(&row, (0, caret), "cWq<Esc>");
        assert_eq!(text, format!("{}q", &row[..caret]), "from {caret}");
    }
}

/// An upward linewise yank to a mark far along a huge row lands on the
/// row's first non-blank, not its window's.
#[test]
fn a_linewise_yank_up_to_a_mark_lands_on_the_rows_first_non_blank() {
    let row = format!("  {}", "a b ".repeat(REACH));
    let rows = format!("{row}\nx");
    let (_, cursor, register) = leave(&rows, (0, 0), "40000lmajy'a");
    assert_eq!(register, format!("{row}\nx\n"));
    assert_eq!(cursor, (0, 2));
    let (_, cursor, _) = leave(&rows, (0, 0), "40000lmajd'a");
    assert_eq!(cursor, (0, 0));
}

/// A Prepend char joins the blank or punctuation after it, so these rows are
/// one word run (the class is the Prepend char's): `iw` and `aw` take all of
/// it, though the ASCII bytes in it change class.
#[test]
fn word_objects_take_a_run_joined_by_prepend_chars() {
    for unit in [
        "\u{600} a",
        "\u{600}.a",
        "\u{600}\u{600} a",
        "\u{600}\u{600}.bc",
    ] {
        let row = unit.repeat(REACH * 3);
        for caret in [
            0,
            row.len() / 2 - row.len() / 2 % unit.len(),
            row.len() - unit.len(),
        ] {
            for keys in ["yiw", "yaw", "diw", "daw"] {
                let (text, _, register) = leave(&row, (0, caret), keys);
                assert!(register == row, "{keys:?} from {caret} over {unit:?}");
                assert_eq!(
                    text.is_empty(),
                    keys.starts_with('d'),
                    "{keys:?} from {caret} over {unit:?}"
                );
            }
        }
    }
}
