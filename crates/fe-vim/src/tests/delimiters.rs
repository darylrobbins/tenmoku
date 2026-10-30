//! Tests of the delimiter objects: ``i[ a[ i< a< it at i` a` `` (see
//! `src/delimiters.rs`).

use crate::HeadlessVim;

/// The text, caret, mode and unnamed register after `keys` on `text` with
/// the caret at `cursor`.
fn after(
    text: &str,
    cursor: (u32, usize),
    keys: &str,
) -> (String, (u32, usize), &'static str, String) {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    let register = vim
        .registers()
        .get('"')
        .map(|register| register.text.clone())
        .unwrap_or_default();
    (vim.text(), vim.cursor(), vim.mode(), register)
}

/// The text a visual selection covers after `keys` (which leave it open).
fn selected(text: &str, cursor: (u32, usize), keys: &str) -> String {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    assert_eq!(vim.mode(), "visual", "{keys} on {text:?}");
    let range = vim.model().primary_selection().range;
    vim.model().text().slice_to_string(range)
}

/// What `d`, `c` and `y` over `object` do, with the delimiters `from`
/// swapped for `to` in the text and the keys: as with `from`, swapped back.
fn alike(text: &str, column: usize, objects: &[&str], from: (char, char), to: (char, char)) {
    let swap = |text: &str, (open, close): (char, char), (new_open, new_close): (char, char)| {
        text.replace(open, &new_open.to_string())
            .replace(close, &new_close.to_string())
    };
    let keys = |object: &str, to: (char, char)| {
        let object = swap(object, from, to);
        object.replace('<', "<lt>")
    };
    for object in objects {
        for operator in ["d", "c", "y2"] {
            let (op, count) = operator.split_at(1);
            let typed = format!("{count}{op}{object}");
            let suffix = if op == "c" { "X<Esc>" } else { "" };
            let want = after(
                text,
                (0, column),
                &format!("{}{suffix}", keys(&typed, from)),
            );
            let got = after(
                &swap(text, from, to),
                (0, column),
                &format!("{}{suffix}", keys(&typed, to)),
            );
            let got = (swap(&got.0, to, from), got.1, got.2, swap(&got.3, to, from));
            assert_eq!(got, want, "{typed} on {text:?} at {column}");
        }
    }
}

#[test]
fn brackets_and_angles_select_as_parens_do() {
    let texts = [
        "x = f(a, (b c), d) + 1",
        "(a (b) c) (d)",
        "x (a) y",
        "() z",
        "f(\"(\", a)",
        "no pairs",
        "((a))",
    ];
    for text in texts {
        for column in (0..text.len()).step_by(2) {
            alike(text, column, &["i(", "a)"], ('(', ')'), ('[', ']'));
            alike(text, column, &["i(", "a)"], ('(', ')'), ('<', '>'));
        }
    }
    // Across rows: an inside on rows of its own goes whole.
    let rows = "f(\n    a,\n    b,\n)";
    for column in 0..3 {
        for row in 0..4 {
            let swap = |text: &str| text.replace('(', "[").replace(')', "]");
            let want = after(rows, (row, column), "di(");
            let got = after(&swap(rows), (row, column), "di[");
            assert_eq!((swap(&want.0), want.1, want.3), (got.0, got.1, got.3));
        }
    }
}

#[test]
fn backticks_select_as_double_quotes_do() {
    let texts = [
        "say \"hi\" there",
        "x \"a\" y \"b\"",
        "\"a\" \"b\"",
        "x \"\" y",
        "a \\\"b\" c",
        "no quotes",
        "run \"ls\"",
    ];
    for text in texts {
        for column in 0..text.len() {
            alike(text, column, &["i\"", "a\""], ('"', '"'), ('`', '`'));
        }
    }
}

#[test]
fn tags_select_inside_or_around_the_count_th_block() {
    let html = "<div><p>one <b>two</b></p></div>";
    // On text, on an opening tag's name and on a closing tag.
    assert_eq!(after(html, (0, 9), "yit").3, "one <b>two</b>");
    assert_eq!(after(html, (0, 14), "yit").3, "two");
    assert_eq!(after(html, (0, 17), "yat").3, "<b>two</b>");
    assert_eq!(after(html, (0, 23), "yit").3, "one <b>two</b>");
    assert_eq!(after(html, (0, 14), "y2it").3, "one <b>two</b>");
    assert_eq!(after(html, (0, 14), "y3at").3, html);
    let (text, cursor, _, deleted) = after(html, (0, 9), "dit");
    assert_eq!(
        (text.as_str(), cursor, deleted.as_str()),
        ("<div><p></p></div>", (0, 8), "one <b>two</b>")
    );
    let (text, _, mode, _) = after(html, (0, 9), "citX<Esc>");
    assert_eq!((text.as_str(), mode), ("<div><p>X</p></div>", "normal"));
    // Names match case-blind, as `\c` folds them.
    assert_eq!(after("<DIV>x</div>", (0, 5), "yit").3, "x");
    assert_eq!(after("<Σ>x</ς>", (0, 3), "yit").3, "x");
    assert_eq!(after("<ẞ>x</ß>", (0, 5), "yit").3, "");
    // Self-closing and unmatched tags don't count; nor do stray closing
    // ones, which close the opening tag before them.
    assert_eq!(after("<p>a<br/>b</p>", (0, 9), "yit").3, "a<br/>b");
    assert_eq!(after("<p>a<br>b</p>", (0, 5), "yit").3, "a<br>b");
    assert_eq!(after("<p>a</b>c</p>", (0, 3), "yit").3, "a</b>c");
    assert_eq!(
        after("<div><p>a<br>b</p></div>", (0, 13), "y2it").3,
        "a<br>b"
    );
    // One name's tags nest; attributes, quoted `>` in them, and rows.
    assert_eq!(after("<a>1<a>2</a>3</a>", (0, 13), "yit").3, "1<a>2</a>3");
    assert_eq!(after("<a href=\"x>y\">link</a>", (0, 15), "yit").3, "link");
    assert_eq!(after("<a\n  href=1>x</a>", (1, 9), "yit").3, "x");
    // From a row's indent, the first non-blank.
    assert_eq!(after("<u>a\n  <a>b</a></u>", (1, 0), "yit").3, "b");
    // An empty element: nothing, at its closing tag.
    let (text, cursor, _, _) = after("<a><b></b></a>", (0, 4), "dit");
    assert_eq!((text.as_str(), cursor), ("<a><b></b></a>", (0, 6)));
}

#[test]
fn the_caret_in_a_tag_goes_to_its_end() {
    // On an opening tag, its `>`: a tag in its attributes counts too.
    let nested = "<a attr=\"<b>x</b>\">y</a>";
    assert_eq!(after(nested, (0, 0), "yit").3, "x");
    assert_eq!(after(nested, (0, 0), "yat").3, "<b>x</b>");
    assert_eq!(after(nested, (0, 0), "y2it").3, "y");
    assert_eq!(after(nested, (0, 17), "yit").3, "y");
    assert_eq!(after("<y\n(]<i>x</i>", (0, 0), "yit").3, "x");
    // On a closing tag in an attribute, back from it.
    assert_eq!(
        after("<b><a t=\"</b>\"><a t=\"</b>\">", (0, 10), "y2it").3,
        "<a t=\""
    );
    // After a `<` on the row with no `>` yet, on to the next `>`, on any
    // row; after a `</`, back from it.
    assert_eq!(after("<b>x<<i>z</i></b>", (0, 4), "yit").3, "z");
    assert_eq!(
        after("<b>x< y>z<i>w</i></b>", (0, 4), "yit").3,
        "x< y>z<i>w</i>"
    );
    assert_eq!(after("<b>x<y\n<i>z</i></b>", (0, 5), "yit").3, "z");
    assert_eq!(after("<b>x<y\nz</b>", (1, 0), "yit").3, "x<y\nz");
    assert_eq!(
        after("<b>x</ <i>w</i></b>", (0, 4), "yit").3,
        "x</ <i>w</i>"
    );
    // Names hold `<`; closing ones `/` and `!` too, opening ones no `!`.
    assert_eq!(after("<x<y>z</x<y>", (0, 5), "yit").3, "z");
    assert_eq!(after("<b><x<y>z</y></b>", (0, 8), "y3it").3, "<x<y>z</y>");
    assert_eq!(after("<i><b></</x>x</b></i>", (0, 12), "yit").3, "");
    assert_eq!(after("<b><!x>z</b>", (0, 7), "y2it").3, "");
    assert_eq!(after("<b></!x>z</b>", (0, 8), "yit").3, "");
}

#[test]
fn names_with_composing_chars() {
    // A lone composing char folds; one composed onto a char doesn't, and
    // matches the same char with at least its composing chars, in any
    // order.
    assert_eq!(after("<\u{399}>x</\u{345}>", (0, 4), "yit").3, "x");
    assert_eq!(after("<a\u{345}>x</A\u{345}>", (0, 5), "yit").3, "");
    assert_eq!(after("<a\u{345}>x</a\u{345}>", (0, 5), "yit").3, "x");
    assert_eq!(after("<a\u{301}>x</a\u{301}\u{302}>", (0, 4), "yit").3, "x");
    assert_eq!(after("<a\u{301}\u{302}>x</a\u{301}>", (0, 6), "yit").3, "");
    assert_eq!(
        after("<a\u{301}\u{302}>x</a\u{302}\u{301}>", (0, 6), "yit").3,
        "x"
    );
    assert_eq!(after("<a>x</a\u{345}>", (0, 3), "yit").3, "");
    assert_eq!(after("<a\u{200d}b>x</A\u{200d}B>", (0, 6), "yit").3, "");
    // Composing chars after an opening tag's `<` are the `<`'s: the name
    // is what follows, and it's no other tag's nested one; with nothing
    // after, the tag fails the object if it's the one taken.
    assert_eq!(after("<\u{345}>x</\u{399}>", (0, 5), "yit").3, "");
    assert_eq!(after("<\u{345}a>x</a>", (0, 4), "yit").3, "x");
    assert_eq!(
        after("<a><\u{345}a>x</a></a>", (0, 8), "y2it").3,
        "<\u{345}a>x"
    );
    assert_eq!(after("<b><\u{345}>y</b>", (0, 7), "yit").3, "");
    assert_eq!(after("<b><\u{345}><br>y</b>", (0, 11), "yit").3, "");
    assert_eq!(
        after("<i><b><\u{345}>x</b></i>", (0, 11), "y3it").3,
        "<b><\u{345}>x</b>"
    );
}

#[test]
fn any_closing_tag_on_one_row_counts_back() {
    // `</` then anything but `>` up to a `>` on the row closes the opening
    // tag before it, going back from the caret; it pairs with none.
    let blank = "<a>x</a b></a>";
    assert_eq!(after(blank, (0, 3), "yit").3, "x</a b>");
    assert_eq!(after(blank, (0, 10), "yit").3, "");
    assert_eq!(after("<a>x</ <b></a>", (0, 7), "yit").3, "");
    assert_eq!(after("<a>x</></a>", (0, 7), "yit").3, "");
    assert_eq!(after("<a>x</a\nb></a>", (1, 2), "yit").3, "x</a\nb>");
}

#[test]
fn names_are_patterns() {
    // A name pairs as a pattern, case-blind: `</name>` closes, `<name`
    // nests if it ends in a word char.
    assert_eq!(after("<a.b>x</axb>z</a.b>", (0, 0), "yit").3, "x");
    assert_eq!(
        after("<a.b>x<axb>y</axb>z</a.b>", (0, 0), "yit").3,
        "x<axb>y</axb>z"
    );
    assert_eq!(after("<a.>x<a*>y</aa>z</aa>", (0, 0), "yit").3, "x<a*>y");
    assert_eq!(after("<a*>x</>y", (0, 0), "yit").3, "x");
    assert_eq!(after("<a\\d>x</A5>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a.></a>>", (0, 4), "yat").3, "<a.></a>");
    // Plain names nest only if they end in a word char too.
    assert_eq!(after("<a->x<a->y</a->z</a->", (0, 0), "yit").3, "x<a->y");
    // One that isn't a pattern on its own pairs with nothing.
    for name in ["a[", "a~", "a\\(b", "a\\|b"] {
        let text = format!("<b><{name}>x</{name}></b>");
        let at = name.len() + 5;
        assert_eq!(
            after(&text, (0, at), "yit").3,
            format!("<{name}>x</{name}>")
        );
    }
}

#[test]
fn pattern_names_pair_as_nvim_reads_them() {
    // Groups and branches inside them are fine; flags in the name don't
    // undo the case folding or the magic of what follows it.
    assert_eq!(after("<a\\(b\\)>x</ab>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a\\%(b\\)>x</ab>", (0, 0), "yit").3, "x");
    assert_eq!(
        after("<a\\(b\\|c\\)>x<ab>y</ab>z</ac>", (0, 0), "yit").3,
        "x<ab>y</ab>z"
    );
    assert_eq!(after("<a\\C>x</A>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a\\M>x<a>y</a>z</a>", (0, 0), "yit").3, "x<a>y</a>z");
    assert_eq!(after("<a\\V>x<a>y</a>z</a>", (0, 0), "yit").3, "x<a>y</a>z");
    // Matches overlap: a greedy name still finds the `<a>` inside its
    // match, and a name ends where its own pattern does.
    assert_eq!(after("<a.*>x<a>y</a>z</a>", (0, 0), "yit").3, "x<a>y</a>z");
    assert_eq!(after("<a.>x<a>>y</a>>z</a>>", (0, 0), "yit").3, "x<a>>y");
    assert_eq!(after("<a.>x<a >y</a >z</a >", (0, 0), "yit").3, "x<a >y");
    // Ending very magic, a name pairs with nothing.
    assert_eq!(after("<b><a\\v>x</a></b>", (0, 8), "yit").3, "<a\\v>x</a>");
    assert_eq!(after("<b><a\\vb\\m>x</ab></b>", (0, 11), "yit").3, "x");
}

#[test]
fn pattern_names_read_collections_and_magic_as_nvim_does() {
    // What's in a collection is its own; the magic modes decide what's
    // special.
    assert_eq!(after("<a[\\|]>x</a|>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a[~]>x</a~>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a[]b]>x</ab>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a\\V[>x</a[>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a\\V~>x</a~>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a\\M\\[b]>x</ab>", (0, 0), "yit").3, "x");
    // A branch after a collection holding `\(` is still outside a group;
    // a collection with no `]` and a very magic `~` aren't patterns alone.
    for text in ["<a[\\(]\\|b>x</b>", "<a[]>x</a]>", "<a\\v~>x</a~>"] {
        assert_eq!(after(text, (0, 0), "yit").3, "", "{text}");
    }
}

#[test]
fn pattern_names_keep_joined_chars_whole() {
    // A ZWJ or emoji modifier joins a char nvim won't fold; the chars
    // around it still fold.
    assert_eq!(after("<a\u{200d}b.>x</A\u{200d}Bx>", (0, 0), "yit").3, "");
    assert_eq!(after("<a\u{200d}b.>x</a\u{200d}Bx>", (0, 0), "yit").3, "x");
    assert_eq!(after("<a\u{1f3fb}.>x</A\u{1f3fb}x>", (0, 0), "yit").3, "");
    assert_eq!(after("<a\u{1f3fb}.>x</a\u{1f3fb}x>", (0, 0), "yit").3, "x");
    assert_eq!(
        after(
            "<a\u{200d}.>x<a\u{200d}z>y</a\u{200d}z>w</a\u{200d}x>",
            (0, 0),
            "yit"
        )
        .3,
        "x<a\u{200d}z>y</a\u{200d}z>w"
    );
}

#[test]
fn joined_chars_are_in_a_collection_or_class_just_as_their_base_is() {
    // Uppercase class atoms leave out digits, joined ones too; a class or a
    // range of private-use chars takes a joined char only if it takes its
    // base (a `^` the other way); a `-` last in a collection is itself.
    let cases = [
        ("<1\u{200d}\\K>x</1\u{200d}1\u{200d}>", ""),
        ("<1\u{200d}\\I>x</1\u{200d}1\u{200d}>", ""),
        ("<1\u{200d}\\k>x</1\u{200d}1\u{200d}>", "x"),
        ("<-\u{200d}\\k>x</-\u{200d}-\u{200d}>", ""),
        (
            "<a\u{200d}[\\U000f0000-\\U0010fffd]>x</a\u{200d}a\u{200d}>",
            "",
        ),
        (
            "<a\u{200d}[^\\U000f0000-\\U0010fffd]>x</a\u{200d}a\u{200d}>",
            "x",
        ),
        ("<a\u{200d}[a-]>x</a\u{200d}b>", ""),
        ("<a\u{200d}[^a-]>x</a\u{200d}b>", "x"),
    ];
    let got: Vec<(&str, String)> = cases
        .iter()
        .map(|&(text, _)| (text, after(text, (0, 0), "yit").3))
        .collect();
    let want: Vec<(&str, String)> = cases
        .iter()
        .map(|&(text, taken)| (text, taken.to_string()))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn many_unnested_tags_pair_in_one_pass() {
    // 30,000 `<a->` (which nest nothing) before the caret: each finds its
    // partner from one pass back, not a search of its own.
    let many = "<a->".repeat(30_000);
    let text = format!("{many}x</a->");
    assert_eq!(after(&text, (0, many.len()), "yit").3, "x");
    let text = format!("{many}x");
    assert_eq!(after(&text, (0, many.len()), "yit").3, "");
}

#[test]
fn long_runs_of_lt_are_read_once() {
    // Past each `<` of a run the name runs to the same end: read once for
    // the run, not once per `<`, and not kept per `<`.
    let run = "<".repeat(30_000);
    crate::delimiters::STEPS.set(0);
    let text = format!("<a>x{run}></a>");
    assert_eq!(after(&text, (0, 3), "yit").3, format!("x{run}>"));
    let text = format!("<a>x{run}</a>");
    assert_eq!(after(&text, (0, 3), "yit").3, format!("x{run}"));
    // Each tag's name is looked up a step on from the last: linear, not a
    // search per tag.
    let steps = crate::delimiters::STEPS.get();
    assert!(steps < 4 * 30_000, "{steps} lookup steps");
}

#[test]
fn a_count_passes_over_openings_unpaired() {
    // The 30,000 `<a.b>` (pattern names) a count passes over aren't paired.
    let many = "<a.b>".repeat(30_000);
    let text = format!("<b>{many}x</axb></b>");
    let at = 3 + many.len();
    assert_eq!(
        after(&text, (0, at), "30001yit").3,
        format!("{many}x</axb>")
    );
}

#[test]
fn the_caret_in_a_closing_name_keeps_its_tag() {
    // On or past a `<` in a closing tag's name, that tag's block isn't
    // around the caret; one further out is.
    for (column, keys) in [(9, "yit"), (9, "yat"), (10, "yit")] {
        assert_eq!(
            after("<x<y>z</x<y>", (0, column), keys).3,
            "",
            "{keys} at {column}"
        );
    }
    assert_eq!(
        after("<a><x<y>z</x<y></a>", (0, 12), "yit").3,
        "<x<y>z</x<y>"
    );
}

#[test]
fn pattern_pairing_out_of_time_fails() {
    let text = "<b><a.b>x</axb></b>";
    let (got, cursor, _, register) = stopped(|| after(text, (0, 8), "dit"));
    assert_eq!(
        (got.as_str(), cursor, register.as_str()),
        (text, (0, 8), "")
    );
}

/// `run`'s result with every selection's clock run out from the start.
fn stopped<T>(run: impl FnOnce() -> T) -> T {
    crate::delimiters::STOPPED.set(true);
    let result = run();
    crate::delimiters::STOPPED.set(false);
    result
}

#[test]
fn out_of_time_fails_every_tag_object() {
    for keys in ["dit", "dat", "vit"] {
        let (got, _, mode, register) = stopped(|| after("<a>x</a>", (0, 3), keys));
        assert_eq!(
            (got.as_str(), register.as_str()),
            ("<a>x</a>", ""),
            "{keys}"
        );
        assert_ne!(mode, "insert", "{keys}");
    }
}

#[test]
fn a_long_name_is_read_in_bounded_room() {
    // A 50,000-byte name, then one in both tags of a pair: their
    // tables hold an entry per place a name can start, not per byte.
    let long = "b".repeat(50_000);
    crate::delimiters::ENTRIES.set(0);
    let text = format!("<a>x<{long}></a>");
    assert_eq!(after(&text, (0, 3), "yit").3, format!("x<{long}>"));
    let text = format!("<{long}>x</{long}>");
    assert_eq!(after(&text, (0, long.len() + 2), "yit").3, "x");
    assert!(
        crate::delimiters::ENTRIES.get() < 100,
        "{}",
        crate::delimiters::ENTRIES.get()
    );
}

#[test]
fn sparse_lts_in_a_name_keep_a_few_entries_each() {
    let piece = format!("<{}", "b".repeat(1_000));
    let run = piece.repeat(100);
    crate::delimiters::ENTRIES.set(0);
    let text = format!("<a>x{run}></a>");
    assert_eq!(after(&text, (0, 3), "yit").3, format!("x{run}>"));
    assert!(
        crate::delimiters::ENTRIES.get() < 1_000,
        "{}",
        crate::delimiters::ENTRIES.get()
    );
}

#[test]
fn a_missing_pair_fails_with_nothing_changed() {
    let cases = [
        ("abc", (0, 1), "di["),
        ("a > b", (0, 0), "di<lt>"),
        ("no ticks", (0, 1), "di`"),
        ("plain text", (0, 2), "dit"),
        ("x <a>b</a>", (0, 0), "dit"),
        ("<a><b>x</b></a>", (0, 6), "d3it"),
        ("[a [b] c]", (0, 4), "d3i["),
        ("<a><a>1</a>", (0, 1), "dat"),
        ("<p>a</b>c</p>", (0, 8), "cit"),
    ];
    for (text, cursor, keys) in cases {
        let mut vim = HeadlessVim::new(text, cursor);
        let version = vim.model().snapshot().version();
        vim.feed(keys).unwrap();
        assert_eq!(
            (
                vim.text().as_str(),
                vim.cursor(),
                vim.mode(),
                vim.registers().get('"')
            ),
            (text, cursor, "normal", None),
            "{keys} on {text:?}"
        );
        assert_eq!(
            vim.model().snapshot().version(),
            version,
            "{keys} on {text:?}"
        );
    }
}

#[test]
fn visual_objects_grow() {
    let html = "<a><b>ab</b></a>";
    assert_eq!(selected(html, (0, 6), "vit"), "ab");
    assert_eq!(selected(html, (0, 6), "vitit"), "<b>ab</b>");
    assert_eq!(selected(html, (0, 6), "vititit"), html);
    assert_eq!(selected(html, (0, 6), "vatat"), html);
    // One char inside is the selection already: the tags too.
    assert_eq!(selected("<a>x</a>", (0, 3), "vit"), "<a>x</a>");
    assert_eq!(selected("[[a]]", (0, 2), "va[a["), "[[a]]");
    assert_eq!(selected("x [ab cd] y", (0, 4), "viwi["), "ab cd");
    assert_eq!(selected("f<a, b>", (0, 3), "vi<lt>"), "a, b");
    assert_eq!(selected("x `ab` y", (0, 3), "vi`i`"), "`ab`");
    // An empty inside: `i[` fails, `i`` takes the quotes.
    let mut vim = HeadlessVim::new("x [] y", (0, 2));
    vim.feed("vi[").unwrap();
    assert_eq!((vim.cursor(), vim.visual_anchor()), ((0, 2), Some((0, 2))));
    assert_eq!(selected("x `` y", (0, 2), "vi`"), "``");
}

#[test]
fn dot_repeats_a_tag_change() {
    let (text, ..) = after("<b>one</b>\n<b>two</b>", (0, 4), "citX<Esc>j.");
    assert_eq!(text, "<b>X</b>\n<b>X</b>");
}
