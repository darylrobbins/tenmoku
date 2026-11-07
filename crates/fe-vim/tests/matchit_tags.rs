//! matchit's `%` and `g%` through html, xml and markdown tags
//! (`feature/matchit/tags.rs`). Each case is written out from nvim 0.12.4:
//! `nvim --clean --headless --cmd 'packadd matchit' --cmd 'filetype plugin
//! on' --cmd 'syntax off' -l probe.lua` (with syntax, matchit skips matches
//! in strings and comments; fe-vim doesn't), `vim.bo.filetype` set per case
//! and the keys fed with `:normal`. Where fe-vim differs on purpose (names
//! as plain text) the unit tests in `tags.rs` say so; here only html's
//! self-closing tags of an open tag's name, which the spec has open
//! nothing, say so.

use std::time::Duration;

use fe_vim::{
    HeadlessVim, TAG_BUDGET,
    search::{set_call_limit, set_time_limit},
    set_tag_budget, take_tag_work,
};

const HTML: Option<&str> = Some("html");
const XML: Option<&str> = Some("xml");
const MARKDOWN: Option<&str> = Some("markdown");

fn open(language: Option<&'static str>, rows: &[&str], cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = HeadlessVim::new(&rows.join("\n"), cursor);
    vim.set_language(language);
    vim
}

/// Where the cursor is after each of `keys`, fed one after another.
fn path(
    language: Option<&'static str>,
    rows: &[&str],
    cursor: (u32, usize),
    keys: &[&str],
) -> Vec<(u32, usize)> {
    let mut vim = open(language, rows, cursor);
    keys.iter()
        .map(|keys| {
            vim.feed(keys).unwrap();
            vim.cursor()
        })
        .collect()
}

/// The text after `keys` from `col` on a one-row buffer.
fn edited(language: Option<&'static str>, row: &str, col: usize, keys: &str) -> String {
    let mut vim = open(language, &[row], (0, col));
    vim.feed(keys).unwrap();
    vim.text()
}

/// Asserts `%` and `g%` from `cursor` fail, leaving the cursor and text.
fn fails(language: Option<&'static str>, rows: &[&str], cursor: (u32, usize)) {
    for keys in ["%", "g%"] {
        let mut vim = open(language, rows, cursor);
        vim.feed(keys).unwrap();
        let what = format!("{language:?} {rows:?} {cursor:?} {keys}");
        assert!(vim.state().failed(), "{what}");
        assert_eq!(
            (vim.cursor(), vim.text()),
            (cursor, rows.join("\n")),
            "{what}"
        );
    }
}

#[test]
fn matchit_tags_pair() {
    // From div's name to the `/` of `</div>` and back to its `d`, in all
    // three; `g%` the same.
    let rows = ["<div class=\"a\">", "  <p>hi</p>", "</div>"];
    for language in [HTML, XML, MARKDOWN] {
        let to_and_fro = [(2, 1), (0, 1)];
        assert_eq!(path(language, &rows, (0, 2), &["%", "%"]), to_and_fro);
        assert_eq!(path(language, &rows, (0, 2), &["g%", "g%"]), to_and_fro);
    }
    // On `<` the `<:>` pair comes first; from the name, `br/` isn't `div`.
    let row = ["<div><br/><div>x</div></div>"];
    assert_eq!(path(HTML, &row, (0, 0), &["%", "%"]), [(0, 4), (0, 0)]);
    assert_eq!(path(HTML, &row, (0, 1), &["%", "%"]), [(0, 23), (0, 1)]);
    // Anywhere in the tag after `<`: its name's pair, from an attribute
    // too; on its `>` the `<`.
    let row = ["<div class=\"a\">x</div>"];
    assert_eq!(path(HTML, &row, (0, 6), &["%"]), [(0, 17)]);
    assert_eq!(path(HTML, &row, (0, 14), &["%", "%"]), [(0, 0), (0, 14)]);
    // A closing tag's `/` or name goes back to the name; its `<` to its
    // `>`.
    let row = ["<a>x</a>"];
    assert_eq!(path(HTML, &row, (0, 5), &["%"]), [(0, 1)]);
    assert_eq!(path(HTML, &row, (0, 6), &["%"]), [(0, 1)]);
    assert_eq!(path(HTML, &row, (0, 4), &["%"]), [(0, 7)]);
    assert_eq!(path(HTML, &row, (0, 7), &["%"]), [(0, 4)]);
    assert_eq!(path(HTML, &row, (0, 0), &["g%", "g%"]), [(0, 2), (0, 0)]);
    // From text, the row's next match: the `<` of a tag, to its `>`.
    assert_eq!(path(HTML, &["  <p>hi</p>"], (0, 0), &["%"]), [(0, 4)]);
    assert_eq!(path(HTML, &["  <p>hi</p>"], (0, 5), &["%"]), [(0, 10)]);
    let prose = ["# Title", "Some <b>bold</b> text"];
    assert_eq!(path(MARKDOWN, &prose, (1, 0), &["%"]), [(1, 7)]);
    let row = ["<b>x</b><b>y</b>"];
    assert_eq!(path(HTML, &row, (0, 9), &["%", "%"]), [(0, 13), (0, 9)]);

    // Same-name tags nest by depth, over rows too.
    let rows = ["<div>", "<div>", "</div>", "</div>"];
    assert_eq!(path(HTML, &rows, (0, 1), &["%", "%"]), [(3, 1), (0, 1)]);
    let row = ["<a><a></a><b></b></a>"];
    assert_eq!(
        path(XML, &row, (0, 1), &["%", "%", "g%"]),
        [(0, 18), (0, 1), (0, 18)]
    );
    let details = [
        "<details>",
        "<summary>x</summary>",
        "<details>",
        "y",
        "</details>",
        "</details>",
    ];
    assert_eq!(
        path(MARKDOWN, &details, (0, 3), &["%", "%"]),
        [(5, 1), (0, 1)]
    );
    assert_eq!(
        path(MARKDOWN, &details, (2, 1), &["%", "%"]),
        [(4, 1), (2, 1)]
    );

    // `<!--` and `-->`: from html's `<` (or `!`), xml's `!`; xml's `<` is
    // the `<:>` pair.
    let row = ["<!-- a -->x"];
    for language in [HTML, MARKDOWN] {
        assert_eq!(path(language, &row, (0, 0), &["%", "%"]), [(0, 7), (0, 0)]);
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 7), (0, 0)]);
    }
    assert_eq!(path(HTML, &row, (0, 9), &["%", "%"]), [(0, 0), (0, 7)]);
    assert_eq!(path(HTML, &row, (0, 0), &["g%", "g%"]), [(0, 7), (0, 0)]);
    assert_eq!(path(XML, &row, (0, 0), &["%", "%"]), [(0, 9), (0, 0)]);
    assert_eq!(path(XML, &row, (0, 1), &["%", "%"]), [(0, 7), (0, 1)]);
    assert_eq!(path(XML, &row, (0, 7), &["%", "%"]), [(0, 1), (0, 7)]);
    // They nest.
    let row = ["<!-- <!-- -->x-->"];
    assert_eq!(path(HTML, &row, (0, 0), &["%"]), [(0, 14)]);

    // html folds case, xml doesn't.
    for language in [HTML, MARKDOWN] {
        let row = ["<DIV>x</div>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 7), (0, 1)]);
    }
    let row = ["<DIV>x</div></DIV>"];
    assert_eq!(path(XML, &row, (0, 1), &["%", "%"]), [(0, 13), (0, 1)]);
    assert_eq!(path(HTML, &["<É>x</é>"], (0, 1), &["%"]), [(0, 6)]);
    assert_eq!(
        path(HTML, &["é<é>x</é>"], (0, 3), &["%", "%"]),
        [(0, 8), (0, 3)]
    );
    // A char at a time, as `\c` folds it, whatever its length: `Σ` and
    // final `ς`, the Kelvin sign and `k`, nested too; `ı` only itself.
    for language in [HTML, MARKDOWN] {
        let row = ["<Σ>x</ς>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 6), (0, 1)]);
        assert_eq!(path(language, &["<ς>x</Σ>"], (0, 6), &["%"]), [(0, 1)]);
        let row = ["<\u{212a}>x</k>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 7), (0, 1)]);
        assert_eq!(
            path(language, &["<k>x</\u{212a}>"], (0, 5), &["%"]),
            [(0, 1)]
        );
        let row = ["<\u{212a}a>x</Ka>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 8), (0, 1)]);
        let row = ["<k><\u{212a}></k>x</K>"];
        assert_eq!(path(language, &row, (0, 1), &["%"]), [(0, 14)]);
    }
    // A char with composing chars after it (an accent, a zero-width
    // joiner) matches only itself with them; the chars around it fold.
    for language in [HTML, MARKDOWN] {
        let row = ["<a\u{301}>x</A\u{301}>"];
        fails(language, &row, (0, 1));
        fails(language, &row, (0, 7));
        let row = ["<a\u{301}>x</a\u{301}>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 7), (0, 1)]);
        assert_eq!(
            path(language, &["<Ka\u{301}>x</ka\u{301}>"], (0, 1), &["%"]),
            [(0, 8)]
        );
        fails(language, &["<KA\u{301}>x</ka\u{301}>"], (0, 1));
        let row = ["<a\u{301}><A\u{301}></a\u{301}>x</a\u{301}>"];
        assert_eq!(path(language, &row, (0, 1), &["%"]), [(0, 11)]);
        assert_eq!(edited(language, row[0], 1, "d%"), "<x</a\u{301}>");
        fails(language, &["<a\u{200d}b>x</A\u{200d}b>"], (0, 1));
        let row = ["<a\u{200d}b>x</a\u{200d}B>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 9), (0, 1)]);
        fails(language, &["<a>x</a\u{301}>"], (0, 1));
    }
    fails(XML, &["<\u{212a}>x</k>"], (0, 1));
    fails(HTML, &["<ı>x</I>"], (0, 1));
    fails(HTML, &["<ß>x</ẞ>"], (0, 1));

    // A bracket first keeps `%`'s own: before the tag, or under the caret
    // in an attribute.
    assert_eq!(path(HTML, &["f(x) <p>y</p>"], (0, 0), &["%"]), [(0, 3)]);
    let row = ["<p on=\"f(x)\">y</p>"];
    assert_eq!(path(HTML, &row, (0, 8), &["%"]), [(0, 10)]);
    assert_eq!(path(HTML, &row, (0, 4), &["%"]), [(0, 15)]);

    // After an operator a closing tag ahead is taken whole, to its `>`;
    // one behind from the name.
    let row = "x<div>y</div>z";
    assert_eq!(edited(HTML, row, 2, "d%"), "x<z");
    assert_eq!(edited(HTML, row, 2, "dg%"), "x<z");
    assert_eq!(edited(HTML, row, 10, "d%"), "x<v>z");
    assert_eq!(edited(HTML, row, 1, "d%"), "xy</div>z");
    assert_eq!(edited(HTML, row, 2, "v%d"), "x<div>z");
    assert_eq!(edited(HTML, "x<!-- y -->z", 1, "d%"), "xz");
    assert_eq!(edited(XML, "x<!-- y -->z", 2, "d%"), "x<z");
    assert_eq!(edited(XML, "<br a=\"1\"/>z", 1, "d%"), "<z");
}

#[test]
fn matchit_tags_edges() {
    // A self-closing tag opens nothing: in html its name is `br/` and its
    // `>` follows a `/`, in xml `<br/>` has none (and its `/>` finds no
    // name with attributes).
    for language in [HTML, XML] {
        let row = ["<p><br/>x</p>"];
        fails(language, &row, (0, 4));
        fails(language, &row, (0, 6));
        assert_eq!(path(language, &row, (0, 1), &["%"]), [(0, 10)]);
    }
    assert_eq!(path(XML, &["<p><p/>x</p></p>"], (0, 1), &["%"]), [(0, 9)]);
    // One of the open tag's name, with attributes or not, is passed over
    // both ways, and `%` from it fails. Here fe-vim differs on purpose:
    // nvim counts `<div class="x"/>` as an opener of `div` (and fails from
    // the outer `<div>`, lands at `(0, 6)` from `</div>`) and from it goes
    // to `</div>`; `<div/>` it counts too.
    for language in [HTML, MARKDOWN] {
        let row = ["<div><div class=\"x\"/></div>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 22), (0, 1)]);
        assert_eq!(path(language, &row, (0, 22), &["%"]), [(0, 1)]);
        fails(language, &row, (0, 6));
        fails(language, &row, (0, 12));
        let row = ["<div><div/></div>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 12), (0, 1)]);
        assert_eq!(path(language, &row, (0, 13), &["%"]), [(0, 1)]);
        fails(language, &row, (0, 6));
        // Its end is the first `>` outside quotes, on a later row too; a
        // tag in an attribute value ends at its own (nvim takes the first
        // `>` for both, and from these outer divs fails).
        let row = ["<div><div title=\">\"/></div>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 22), (0, 1)]);
        assert_eq!(path(language, &row, (0, 22), &["%"]), [(0, 1)]);
        fails(language, &row, (0, 6));
        let row = ["<a title=\"<b/>\">x</a>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 18), (0, 1)]);
        fails(language, &row, (0, 11));
        let row = ["<a title='<a />'>x</a></a>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 19), (0, 1)]);
        let rows = [
            "<div>",
            "<div class=\"x\"",
            "  title='a\"",
            "b'/>",
            "</div>",
        ];
        assert_eq!(path(language, &rows, (0, 1), &["%", "%"]), [(4, 1), (0, 1)]);
        fails(language, &rows, (1, 1));
        let rows = ["<div>", "<div title=\"/>", "\">", "</div>", "</div>"];
        assert_eq!(path(language, &rows, (0, 1), &["%", "%"]), [(4, 1), (0, 1)]);
        // Attributes that run to the end of the text open.
        fails(language, &["<div>", "<div title=\"/>", "</div>"], (0, 1));
    }
    // xml pairs a name followed by blanks with `/>`; `/>` goes back to the
    // last such name, its own tag's or not.
    let row = ["<p><br a=\"1\"/>x</p>"];
    assert_eq!(path(XML, &row, (0, 4), &["%", "%"]), [(0, 12), (0, 4)]);
    let row = ["<p><br />x</p>"];
    assert_eq!(path(XML, &row, (0, 4), &["%", "%"]), [(0, 7), (0, 4)]);
    fails(HTML, &row, (0, 4));
    assert_eq!(
        path(XML, &["<r x=\"1\"><b/></r>"], (0, 11), &["%"]),
        [(0, 1)]
    );
    // An html void element with no closing tag fails; one inside another
    // isn't counted.
    let row = ["<p><br>x</p>"];
    fails(HTML, &row, (0, 4));
    assert_eq!(path(HTML, &row, (0, 1), &["%"]), [(0, 9)]);
    // A closing tag with no opener.
    for language in [HTML, XML] {
        fails(language, &["x</p>y"], (0, 3));
        fails(language, &["x</p>y"], (0, 2));
    }
    // A tag split over rows opens from its name's row; a closing tag split
    // isn't one; on the attribute row, the `>` goes back to the `<`.
    let rows = ["<div", "  class=\"a\">", "x", "</div>"];
    for language in [HTML, XML] {
        assert_eq!(path(language, &rows, (0, 1), &["%", "%"]), [(3, 1), (0, 1)]);
    }
    assert_eq!(path(HTML, &rows, (1, 3), &["%"]), [(0, 0)]);
    fails(HTML, &["<div>x</div", ">"], (0, 1));
    fails(HTML, &["<a>x</a >"], (0, 1));
    // A tag inside an attribute value counts: one of the same name nests,
    // another name's is passed over; from the inner one, its own pair.
    for language in [HTML, XML] {
        let row = ["<a title=\"<a>\">x</a></a>"];
        assert_eq!(path(language, &row, (0, 1), &["%", "%"]), [(0, 21), (0, 1)]);
    }
    let row = ["<a title=\"<b>\">x</a>"];
    assert_eq!(path(HTML, &row, (0, 1), &["%", "%"]), [(0, 17), (0, 1)]);
    fails(HTML, &row, (0, 11));
    let row = ["<a t=\"</a>\">x</a>"];
    fails(HTML, &row, (0, 15));
    assert_eq!(path(HTML, &row, (0, 1), &["%"]), [(0, 7)]);
    fails(XML, &["<a c=\"<a>\">x</a>"], (0, 1));
    // So does each of a run of names before one `>`.
    fails(XML, &["<a <a <a >x</a>"], (0, 1));
    assert_eq!(path(XML, &["<a <b <c >x</a>"], (0, 1), &["%"]), [(0, 12)]);
    // Names compare as plain text: a `*` or `$` in one is just that char,
    // a namespace's `:` too, and `divx` isn't `div` in xml.
    assert_eq!(path(HTML, &["<a*>x</aa></a*>"], (0, 1), &["%"]), [(0, 11)]);
    assert_eq!(path(HTML, &["<a$b>x</a$b>"], (0, 1), &["%"]), [(0, 7)]);
    assert_eq!(path(HTML, &["<x-y>x</x-y>"], (0, 1), &["%"]), [(0, 7)]);
    assert_eq!(
        path(XML, &["<a:b>x</a:b>"], (0, 1), &["%", "%"]),
        [(0, 7), (0, 1)]
    );
    let row = ["<div><divx>y</divx></div>"];
    assert_eq!(path(XML, &row, (0, 1), &["%"]), [(0, 20)]);

    // `<` and `>` count wherever they are: in text, in an attribute.
    fails(HTML, &["a < b <p>x</p>"], (0, 0));
    fails(HTML, &["a <b"], (0, 0));
    assert_eq!(path(HTML, &["x <p>y</p>"], (0, 0), &["%"]), [(0, 4)]);
    fails(HTML, &["<a t=\"<\">x"], (0, 0));
    // html's name is anything after `<` but `/` or `!`, to a blank: `< b`
    // is one, `<!DOCTYPE` isn't (its `>` goes back to `<`), `<?php` is.
    fails(HTML, &["a < b <p>x</p>"], (0, 4));
    assert_eq!(path(HTML, &["< b>x</ b>"], (0, 2), &["%"]), [(0, 6)]);
    assert_eq!(path(HTML, &["<!DOCTYPE html>"], (0, 3), &["%"]), [(0, 0)]);
    fails(HTML, &["<?php x ?>"], (0, 1));
    // xml's is anything but `/` to a blank or `>`, then `>`, the row's end,
    // or attributes whose last char before `>` isn't `/` (a lone blank
    // isn't an attribute).
    fails(XML, &["<!DOCTYPE x>"], (0, 3));
    fails(XML, &["<a >x</a>"], (0, 1));
    assert_eq!(path(XML, &["<a  >x</a>"], (0, 1), &["%"]), [(0, 7)]);
    assert_eq!(path(XML, &["<a b/>>x</a>"], (0, 1), &["%"]), [(0, 9)]);
    // No match on the caret's row: nothing on other rows is looked at.
    fails(HTML, &["<p>", "text", "</p>"], (1, 1));
}

#[test]
fn matchit_tags_are_bounded() {
    // The work is bounded by the counts below, not the clock: a loaded
    // machine mustn't run the search out of its 2 s. Nothing else in this
    // binary relies on the default, so it is never put back.
    set_time_limit(Duration::from_secs(60));
    // 15,000 nested divs and their closing tags: `%` reads each row once
    // (twice a key: the jumplist asks whether it lands before it goes).
    let mut rows = vec!["<div>"; 15_000];
    rows.extend(vec!["</div>"; 15_000]);
    let bytes = rows.join("\n").len();
    let work = |vim: &mut HeadlessVim, keys: &str| {
        take_tag_work();
        vim.feed(keys).unwrap();
        take_tag_work()
    };
    let mut vim = open(HTML, &rows, (0, 1));
    let spent = work(&mut vim, "%");
    assert_eq!(vim.cursor(), (29_999, 1));
    let linear = 2 * 4 * (bytes + rows.len());
    assert!(spent <= linear && spent <= TAG_BUDGET, "{spent}");
    let spent = work(&mut vim, "g%");
    assert_eq!(vim.cursor(), (0, 1));
    assert!(spent <= linear, "{spent}");
    // From the middle, only the rows between.
    let mut vim = open(XML, &rows, (7_000, 1));
    let spent = work(&mut vim, "%");
    assert_eq!(vim.cursor(), (22_999, 1));
    assert!(spent <= 2 * 4 * 16_000 * 7, "{spent}");
    // A pair two rows apart reads about two rows, in a 30,000-row file.
    let mut sections = Vec::new();
    for i in 0..10_000 {
        sections.extend([
            format!("<section id=\"s{i}\">"),
            "  <p>x <b>y</b></p>".into(),
            "</section>".into(),
        ]);
    }
    let sections: Vec<&str> = sections.iter().map(String::as_str).collect();
    let mut vim = open(HTML, &sections, (15_000, 2));
    let spent = work(&mut vim, "%");
    assert_eq!(vim.cursor(), (15_002, 1));
    assert!(spent < 2 * 200, "{spent}");
    vim.feed("%").unwrap();
    assert_eq!(vim.cursor(), (15_000, 1));
    // With no closing tag it reads to the end and fails, still bounded.
    let unbalanced = &rows[..rows.len() - 1];
    let mut vim = open(HTML, unbalanced, (0, 1));
    let spent = work(&mut vim, "%");
    assert!(vim.state().failed());
    assert!(spent <= linear, "{spent}");
    // A row of 50,000 `<a` with no `>`: each name ends at the next blank
    // or `>`, found once, not once a tag.
    for (language, row) in [
        (HTML, format!("{}>x</a>", "<a".repeat(50_000))),
        (XML, format!("{}>x</a>", "<a ".repeat(50_000))),
    ] {
        let mut vim = open(language, &[row.as_str()], (0, 1));
        let spent = work(&mut vim, "%");
        assert!(vim.state().failed(), "{language:?}");
        assert!(spent <= 2 * 6 * row.len(), "{language:?} {spent}");
    }

    // Tags whose quotes stay open to the closing tags: whether each
    // closes itself is read on from each row once, not once a tag.
    let mut open_quotes = vec!["<div t=\""; 10_000];
    open_quotes.extend(vec!["</div>"; 10_000]);
    let quoted = 2 * 4 * (open_quotes.join("\n").len() + open_quotes.len());
    let mut vim = open(HTML, &open_quotes, (0, 1));
    let spent = work(&mut vim, "%");
    assert_eq!(vim.cursor(), (19_999, 1));
    assert!(spent <= 2 * quoted, "{spent}");
    let spent = work(&mut vim, "%");
    assert_eq!(vim.cursor(), (0, 1));
    assert!(spent <= 2 * quoted, "{spent}");

    // A budget run out fails the motion and changes nothing: not the
    // cursor, the text, a register or `''`.
    set_tag_budget(Some(1_000));
    let mut vim = open(HTML, &rows, (0, 1));
    vim.feed("Ggg").unwrap();
    let mark = vim.mark('\'');
    for keys in ["%", "g%", "d%", "y%", "v%"] {
        vim.feed(keys).unwrap();
        assert!(vim.state().failed(), "{keys}");
        vim.feed("<Esc>").unwrap();
        assert_eq!(vim.cursor(), (0, 1), "{keys}");
        assert_eq!(vim.mark('\''), mark, "{keys}");
    }
    assert_eq!(vim.text(), rows.join("\n"));
    assert!(vim.registers().get('"').is_none());
    set_tag_budget(None);
    vim.feed("%").unwrap();
    assert_eq!(vim.cursor(), (29_999, 1));

    // So does the search time limit, even on a pair a few steps apart: a
    // call limit of none makes each deadline past from the start, on this
    // thread only, as a time limit of zero would.
    let row = "<p><a>x</a></p>";
    let mut vim = open(HTML, &[row], (0, 4));
    vim.feed("yl").unwrap();
    let mark = vim.mark('\'');
    let register = vim.registers().get('"').cloned();
    set_call_limit(Some(0));
    for keys in ["%", "g%", "d%", "y%", "v%"] {
        vim.feed(keys).unwrap();
        assert!(vim.state().failed(), "{keys}");
        vim.feed("<Esc>").unwrap();
        assert_eq!(vim.cursor(), (0, 4), "{keys}");
        assert_eq!(vim.mark('\''), mark, "{keys}");
    }
    set_call_limit(None);
    assert_eq!(vim.text(), row);
    assert_eq!(vim.registers().get('"').cloned(), register);
    vim.feed("%").unwrap();
    assert_eq!(vim.cursor(), (0, 8));
}
