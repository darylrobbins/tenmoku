use std::{
    fs,
    sync::{Arc, atomic::AtomicBool},
};

use fe_project::{Replacer, SearchError, SearchOptions, SearchQuery, search};

const FILES: [(&str, &str); 4] = [
    ("a.txt", "foo Foo foobar\n_foo (foo)\n\nend foo"),
    (
        "crlf.txt",
        "foo one\r\nbar foo\r\n\r\nfoo$\r\nlone \r cr foo\r\n",
    ),
    ("b.rs", "fn foo() {}\nlet x = foo;\n"),
    ("tail.txt", "foo\r"),
];

fn query(pattern: &str, regex: bool, case_sensitive: bool, whole_word: bool) -> SearchQuery {
    SearchQuery {
        pattern: pattern.into(),
        regex,
        case_sensitive,
        whole_word,
    }
}

#[test]
fn edits_match_search_ranges() {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in FILES {
        fs::write(dir.path().join(name), text).unwrap();
    }
    let queries = [
        query("foo", false, false, false),
        query("foo", false, true, false),
        query("foo", false, false, true),
        query("fo+", true, true, false),
        query("^", true, true, false),
        query("$", true, true, false),
        query("^$", true, true, false),
        query("o\\b", true, false, false),
        query(".", true, true, false),
    ];
    for q in &queries {
        let mut found = Vec::new();
        search(
            dir.path(),
            q,
            &SearchOptions::default(),
            Arc::new(AtomicBool::new(false)),
            |batch| found.extend(batch),
        )
        .unwrap();
        let replacer = Replacer::new(q, "X").unwrap();
        let mut total = 0;
        for (name, text) in FILES {
            let want: Vec<_> = found
                .iter()
                .filter(|f| f.rel == name)
                .flat_map(|f| &f.lines)
                .flat_map(|l| {
                    l.ranges.iter().map(move |r| {
                        l.line_offset as usize + r.start..l.line_offset as usize + r.end
                    })
                })
                .collect();
            let got: Vec<_> = replacer.edits(text).into_iter().map(|(r, _)| r).collect();
            assert_eq!(got, want, "{q:?} in {name}");
            assert_eq!(replacer.count(text), got.len());
            total += got.len();
        }
        assert!(total > 0, "{q:?} matched nothing");
    }
}

fn expand(pattern: &str, regex: bool, template: &str, text: &str) -> String {
    let replacer = Replacer::new(&query(pattern, regex, true, false), template).unwrap();
    replacer
        .edits(text)
        .into_iter()
        .map(|(_, s)| s)
        .collect::<Vec<_>>()
        .join("|")
}

#[test]
fn template_table() {
    let cases = [
        ("a(b)(c)?", "abx", "[$0]", "[ab]"),
        ("a(b)(c)?", "abx", "$&$&", "abab"),
        ("a(b)(c)?", "abx", "$1-$2-", "b--"),
        ("a(b)", "ab", "$1$1", "bb"),
        ("a(b)", "ab", "$3", ""),
        ("(?<n>b)", "b", "${n}${n}", "bb"),
        ("(?<n>b)", "b", "${zz}.", "."),
        ("b", "b", "$$1", "$1"),
        ("b", "b", "a\\nb\\tc\\\\d", "a\nb\tc\\d"),
        ("b", "b", "\\q $ $x", "\\q $ $x"),
        (
            "(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)",
            "abcdefghij",
            "$10$1",
            "ja",
        ),
        ("(a)", "a", "$123", "3"),
        ("(?<é>b)", "b", "${é}Z${é}", "bZb"),
        ("b", "b", "${é}Z", "Z"),
    ];
    for (pattern, text, template, want) in cases {
        assert_eq!(expand(pattern, true, template, text), want, "{template}");
    }
    assert_eq!(expand("a.c", false, "$0 \\n $1", "a.c"), "$0 \\n $1");
}

#[test]
fn captures_inside_code_points() {
    // Empty matches and byte-oriented ones can fall inside a char: the
    // ranges are search's, and a partial char expands lossily.
    assert_eq!(expand("", true, "[$0]", "é"), "[]|[]|[]");
    assert_eq!(
        expand("(?-u:.)", true, "<$&>", "é"),
        "<\u{fffd}>|<\u{fffd}>"
    );
    assert_eq!(expand("(?-u:\\xC3)", true, "<$&>", "é"), "<\u{fffd}>");
}

#[test]
fn invalid_pattern_and_count() {
    let q = query("(", true, true, false);
    assert!(matches!(
        Replacer::new(&q, "x"),
        Err(SearchError::InvalidPattern(_))
    ));
    let r = Replacer::new(&query("o", false, true, false), "0").unwrap();
    let text = "foo\r\nboo\n";
    assert_eq!(r.count(text), r.edits(text).len());
    assert_eq!(r.count(text), 4);
}
