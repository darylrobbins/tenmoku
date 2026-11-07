//! `in( an( il( al(`, and the same for `[ { <` and the quotes: the next or
//! last pair's object, as targets.vim and mini.ai's `n` and `l` forms give
//! it.

use std::time::{Duration, Instant};

use fe_vim::HeadlessVim;
use fe_vim::search::set_time_limit;

/// A wall bound well clear of the search's 2 s default: the work is bounded
/// by reading only the rows it needs, which the tests below check by result;
/// this only catches a runaway, and holds on a loaded machine.
const RUNAWAY: Duration = Duration::from_secs(30);

fn run(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    let _ = vim.feed(keys);
    vim
}

fn text_after(text: &str, cursor: (u32, usize), keys: &str) -> String {
    run(text, cursor, keys).text()
}

/// What `keys` yanked, or the visual selection they leave, yanked.
fn yanked(text: &str, cursor: (u32, usize), keys: &str) -> String {
    let vim = run(text, cursor, keys);
    vim.registers()
        .get('"')
        .map(|r| r.text.clone())
        .unwrap_or_default()
}

fn selected(text: &str, cursor: (u32, usize), keys: &str) -> String {
    yanked(text, cursor, &format!("{keys}y"))
}

#[test]
fn seek_next_and_last_pairs() {
    let two = "x (a) (b) y";
    // From before them: the first; from after them: the last.
    assert_eq!(text_after(two, (0, 0), "din("), "x () (b) y");
    assert_eq!(text_after(two, (0, 0), "dan("), "x  (b) y");
    assert_eq!(text_after(two, (0, 10), "dil("), "x (a) () y");
    assert_eq!(text_after(two, (0, 10), "dal("), "x (a)  y");
    // `)` and `b` name the same pair.
    assert_eq!(text_after(two, (0, 0), "din)"), "x () (b) y");
    assert_eq!(text_after(two, (0, 10), "dilb"), "x (a) () y");
    // Never the pair holding the caret, or the one it's on.
    assert_eq!(text_after(two, (0, 3), "din("), "x (a) () y");
    assert_eq!(text_after(two, (0, 2), "din("), "x (a) () y");
    assert_eq!(text_after(two, (0, 7), "dil("), "x () (b) y");
    assert_eq!(text_after(two, (0, 8), "dil("), "x () (b) y");
    // None that way: nothing.
    assert_eq!(text_after(two, (0, 3), "dil("), two);
    assert_eq!(text_after(two, (0, 7), "din("), two);
    // Quotes, brackets and braces.
    let strings = r#"say "a" and "b""#;
    assert_eq!(
        text_after(strings, (0, 0), "cin\"X<Esc>"),
        r#"say "X" and "b""#
    );
    assert_eq!(
        text_after(strings, (0, 5), "cin\"X<Esc>"),
        r#"say "a" and "X""#
    );
    assert_eq!(
        text_after(strings, (0, 13), "cil\"X<Esc>"),
        r#"say "X" and "b""#
    );
    assert_eq!(text_after(strings, (0, 0), "dan\""), r#"say and "b""#);
    assert_eq!(text_after("x 'a' y", (0, 0), "din'"), "x '' y");
    assert_eq!(text_after("x `a` y", (0, 6), "dil`"), "x `` y");
    assert_eq!(yanked("x [1, 2] y", (0, 0), "yin["), "1, 2");
    assert_eq!(yanked("x [1, 2] y", (0, 0), "yan]"), "[1, 2]");
    assert_eq!(yanked("x <a> y", (0, 6), "yil<"), "a");
    assert_eq!(
        text_after("if x { a } else { b }", (0, 0), "din{"),
        "if x {} else { b }"
    );
    assert_eq!(
        text_after("if x { a } else { b }", (0, 8), "dinB"),
        "if x { a } else {}"
    );
    // Across rows.
    let rows = "x\n(a)\ny";
    assert_eq!(text_after(rows, (0, 0), "din("), "x\n()\ny");
    assert_eq!(text_after(rows, (2, 0), "dil("), "x\n()\ny");
    assert_eq!(text_after(rows, (2, 0), "dal("), "x\n\ny");
    assert_eq!(
        text_after("say\nf(\"a\")", (0, 0), "cin\"X<Esc>"),
        "say\nf(\"X\")"
    );
    assert_eq!(
        text_after("f(\"a\")\nsay", (1, 1), "cil\"X<Esc>"),
        "f(\"X\")\nsay"
    );
    // A pair over rows, its inside as the plain object takes it.
    let block = "f\n(\n  a\n)\n";
    assert_eq!(
        text_after(block, (0, 0), "din("),
        text_after(block, (2, 2), "di(")
    );
    assert_eq!(
        text_after(block, (4, 0), "dal("),
        text_after(block, (2, 2), "da(")
    );
    // Nesting: `n` is by the opening bracket, `l` by the closing one.
    assert_eq!(text_after("x (a (b) c)", (0, 0), "din("), "x ()");
    assert_eq!(text_after("(a (b) c) y", (0, 10), "dil("), "() y");
    // An escaped quote opens a string as `i"` takes it on a quote, though
    // only an unescaped one closes it.
    for q in ['"', '\'', '`'] {
        let opened = format!(r"x \{q}a{q} {q}b{q} y");
        assert_eq!(yanked(&opened, (0, 0), &format!("yin{q}")), "a");
        assert_eq!(yanked(&opened, (0, 12), &format!("yil{q}")), "b");
        assert_eq!(yanked(&opened, (0, 12), &format!("2yil{q}")), "a");
        let inside = format!(r"x {q}a\{q}b{q} {q}c{q} y");
        assert_eq!(
            yanked(&inside, (0, 0), &format!("yin{q}")),
            format!(r"a\{q}b")
        );
        assert_eq!(
            yanked(&inside, (0, 14), &format!("2yil{q}")),
            format!(r"a\{q}b")
        );
        assert_eq!(yanked(&inside, (0, 3), &format!("yin{q}")), "c");
    }
    // A bracket in a string pairs within it.
    assert_eq!(
        text_after(r#"x "(" (a) ")" y"#, (0, 0), "din("),
        r#"x "(" () ")" y"#
    );
}

#[test]
fn seek_counts_and_bounds() {
    // Nothing in this binary relies on the default limit; a loaded machine
    // mustn't run these searches out of it. Never put back.
    set_time_limit(Duration::from_secs(60));
    let three = "x (a) (b) (c) y";
    assert_eq!(text_after(three, (0, 0), "2din("), "x (a) () (c) y");
    assert_eq!(text_after(three, (0, 0), "d3in("), "x (a) (b) () y");
    assert_eq!(text_after(three, (0, 14), "2dil("), "x (a) () (c) y");
    assert_eq!(text_after(three, (0, 0), "4din("), three);
    assert_eq!(text_after("x (a (b) c)", (0, 0), "2din("), "x (a () c)");
    assert_eq!(text_after("(a (b) c) y", (0, 10), "2dil("), "(a () c) y");
    let quotes = r#"x "a" "b" "c""#;
    assert_eq!(text_after(quotes, (0, 0), "2din\""), r#"x "a" "" "c""#);

    // Within 50 rows each way: a pair 50 rows off is found, 51 isn't.
    let below = |gap: usize| format!("x\n{}(a)\n", "\n".repeat(gap - 1));
    assert_eq!(
        text_after(&below(50), (0, 0), "din("),
        format!("x\n{}()\n", "\n".repeat(49))
    );
    assert_eq!(text_after(&below(51), (0, 0), "din("), below(51));
    let above = |gap: usize| format!("(a)\n{}x", "\n".repeat(gap));
    assert_eq!(
        text_after(&above(50), (50, 0), "dil("),
        format!("()\n{}x", "\n".repeat(50))
    );
    assert_eq!(text_after(&above(51), (51, 0), "dil("), above(51));
    // Both ends must be within: a pair opening 49 rows on and closing 51 on
    // isn't.
    let late = format!("x\n{}(\n\n)\n", "\n".repeat(48));
    assert_eq!(text_after(&late, (0, 0), "din("), late);
    // The same for quotes.
    let quote = |gap: usize| format!("x\n{}\"a\"\n", "\n".repeat(gap - 1));
    assert_eq!(text_after(&quote(51), (0, 0), "din\""), quote(51));
    assert_eq!(
        text_after(&quote(50), (0, 0), "din\""),
        format!("x\n{}\"\"\n", "\n".repeat(49))
    );

    // A long row is read whole, within the deadline.
    let long = format!("x{} (a) \"b\" y", " ".repeat(1 << 16));
    let end = long.len() - 1;
    for (keys, cursor, after) in [
        ("din(", 0, "()"),
        ("din\"", 0, "\"\""),
        ("dil(", end, "()"),
        ("dil\"", end, "\"\""),
    ] {
        let mut vim = HeadlessVim::new(&long, (0, cursor));
        let started = Instant::now();
        let _ = vim.feed(keys);
        let took = started.elapsed();
        assert!(took < RUNAWAY, "{keys}: {took:?}");
        assert!(vim.text().contains(after), "{keys}");
        assert_eq!(vim.text().len(), long.len() - 1, "{keys}");
    }

    // A large buffer reads only the rows near the caret.
    for text in ["x\n".repeat(100_000), "(\n".repeat(100_000)] {
        for keys in ["din(", "dil(", "din\"", "dil\""] {
            let mut vim = HeadlessVim::new(&text, (50_000, 0));
            let started = Instant::now();
            let _ = vim.feed(keys);
            let took = started.elapsed();
            assert!(took < RUNAWAY, "{keys}: {took:?}");
            assert_eq!(vim.text(), text, "{keys}");
        }
    }
}

#[test]
fn seek_repeat_and_visual() {
    // `.` seeks again from where the caret is left.
    assert_eq!(text_after("x (a) (b)", (0, 0), "din(."), "x () ()");
    assert_eq!(text_after("(a) (b) x", (0, 8), "dil(."), "() () x");
    assert_eq!(
        text_after(r#"x "a" "b""#, (0, 0), "cin\"X<Esc>."),
        r#"x "X" "X""#
    );
    // In a visual mode: the next or last pair's object.
    assert_eq!(selected("x (ab) y", (0, 0), "vin("), "ab");
    assert_eq!(selected("x (ab) y", (0, 0), "van("), "(ab)");
    assert_eq!(selected("x (ab) (cd) y", (0, 12), "vil("), "cd");
    assert_eq!(selected("x (ab) (cd) y", (0, 0), "v2in("), "cd");
    assert_eq!(selected(r#"x "ab" y"#, (0, 0), "vin\""), "ab");
    assert_eq!(selected(r#"x "ab" y"#, (0, 0), "van\""), r#""ab" "#);
    // A count from one position passes an empty pair on the way.
    assert_eq!(selected("x () (ab) y", (0, 0), "v2in("), "ab");
    assert_eq!(selected("x [] [ab] y", (0, 0), "v2in["), "ab");
    assert_eq!(selected("x {} {ab} y", (0, 0), "v2in{"), "ab");
    assert_eq!(selected("x <> <ab> y", (0, 0), "v2in<"), "ab");
    assert_eq!(selected("x (ab) () y", (0, 10), "v2il("), "ab");
    assert_eq!(selected("x [ab] [] y", (0, 10), "v2il]"), "ab");
    // An empty pair's inside selects nothing; empty quotes select themselves,
    // as `vi"` and ``vi` `` do.
    assert_eq!(selected("x () y", (0, 0), "vin("), "x");
    assert_eq!(selected("x `` y", (0, 0), "vin`"), "``");
    assert_eq!(selected("x `` y", (0, 5), "vil`"), "``");
    assert_eq!(selected("x `` y", (0, 2), "vi`"), "``");
    assert_eq!(selected("x \"\" y", (0, 0), "vin\""), "\"\"");
    assert_eq!(selected("x '' y", (0, 5), "vil'"), "''");
    // Again, it grows as the plain object does.
    let nested = "f(x (ab) y)";
    assert_eq!(selected(nested, (0, 2), "vin("), "ab");
    assert_eq!(
        selected(nested, (0, 2), "vin(i("),
        selected(nested, (0, 5), "vi(i(")
    );
    assert_eq!(selected(nested, (0, 2), "vin(i("), "x (ab) y");
    assert_eq!(selected(nested, (0, 2), "vin(in("), "x (ab) y");
    assert_eq!(selected(nested, (0, 2), "vin(a("), "(ab)");
    assert_eq!(
        selected(nested, (0, 2), "vin(a("),
        selected(nested, (0, 5), "vi(a(")
    );
    // Again over a one-char inside an object selected: the plain object
    // too, not a further seek.
    let ones = "x (a) (b) y";
    assert_eq!(selected(ones, (0, 0), "vin(in("), "a");
    assert_eq!(
        selected(ones, (0, 0), "vin(in("),
        selected(ones, (0, 0), "vin(i(")
    );
    assert_eq!(selected(ones, (0, 10), "vil(il("), "b");
    assert_eq!(selected(ones, (0, 0), "vin(an("), "(a)");
    // A third, and mixed with plain objects, grow as the plain one does.
    for keys in ["vin(in(in(", "vin(i(in(", "vin(in(i(", "vin(i(i("] {
        assert_eq!(
            selected(ones, (0, 0), keys),
            selected(ones, (0, 0), "vin(i(i("),
            "{keys}"
        );
    }
    for keys in ["vil(il(il(", "vil(i(il(", "vil(il(i("] {
        assert_eq!(
            selected(ones, (0, 10), keys),
            selected(ones, (0, 10), "vil(i(i("),
            "{keys}"
        );
    }
    let quoted = r#"x "a" "b" y"#;
    for keys in ["vin\"in\"in\"", "vin\"i\"in\"", "vin\"in\"i\""] {
        assert_eq!(
            selected(quoted, (0, 0), keys),
            selected(quoted, (0, 0), "vin\"i\"i\""),
            "{keys}"
        );
    }
    let wide = "x (é) (b) y";
    assert_eq!(
        selected(wide, (0, 0), "vin(in(in("),
        selected(wide, (0, 0), "vin(i(i(")
    );
    assert_eq!(selected(quoted, (0, 0), "vin\"in\""), "a");
    assert_eq!(selected(quoted, (0, 10), "vil\"il\""), "b");
    assert_eq!(selected("x (é) (b) y", (0, 0), "vin(in("), "é");
    // A row with an odd number of quotes pairs them from its start, as `i"`
    // does on a quote: the lone one at the end pairs with none.
    let lone = r#"x = "a" + "b"#;
    assert_eq!(text_after(lone, (0, 0), "din\""), r#"x = "" + "b"#);
    assert_eq!(text_after(lone, (0, 8), "din\""), lone);
    assert_eq!(text_after(lone, (0, 8), "dil\""), r#"x = "" + "b"#);
    assert_eq!(text_after(lone, (0, 11), "dil\""), r#"x = "" + "b"#);
}

#[test]
fn seek_leaves_plain_objects_alone() {
    assert_eq!(text_after("f(a, b)", (0, 3), "di("), "f()");
    assert_eq!(text_after("x (a) y", (0, 0), "di("), "x () y");
    assert_eq!(text_after(r#"x "a" y"#, (0, 3), "da\""), "x y");
    assert_eq!(text_after(r#"x "a" y"#, (0, 0), "da\""), "x y");
    // A char after `in` that names no pair cancels it, and the next keys
    // start afresh.
    assert_eq!(text_after("x (a) y", (0, 0), "dinzx"), " (a) y");
    assert_eq!(text_after("x (a) y", (0, 0), "din<Esc>x"), " (a) y");
    // `n` and `l` after `i` without an operator, or a visual mode, are
    // nothing new.
    assert_eq!(text_after("x (a) y", (0, 0), "inX<Esc>"), "nXx (a) y");
}
