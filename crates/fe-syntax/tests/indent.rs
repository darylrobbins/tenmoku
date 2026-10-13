#![cfg(all(
    feature = "rust",
    feature = "python",
    feature = "javascript",
    feature = "typescript",
    feature = "go",
    feature = "c",
    feature = "cpp",
    feature = "java",
    feature = "json",
    feature = "lua",
    feature = "css",
    feature = "bash",
    feature = "toml",
    feature = "yaml"
))]

use fe_buffer::Buffer;
use fe_syntax::{
    IndentSuggestion::{self, Relative, Zero},
    Syntax, closer_outdents, language_by_name, suggest_indent,
};

fn parse(language: &str, text: &str) -> (Syntax, Buffer) {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    (syntax, buffer)
}

/// The suggestion for `row` of `text`.
fn at(language: &str, text: &str, row: u32) -> Option<IndentSuggestion> {
    let (syntax, buffer) = parse(language, text);
    suggest_indent(&syntax, buffer.text(), row)
}

/// The suggestion for a new blank row after `row`, as Enter at its end makes.
fn after(language: &str, text: &str, row: u32) -> Option<IndentSuggestion> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.insert(row as usize + 1, "");
    at(language, &lines.join("\n"), row + 1)
}

fn closer(language: &str, text: &str, row: u32) -> Option<u32> {
    let (syntax, buffer) = parse(language, text);
    closer_outdents(&syntax, buffer.text(), row)
}

fn rel(basis_row: u32, delta: i32) -> Option<IndentSuggestion> {
    Some(Relative { basis_row, delta })
}

/// Every row's suggestion as columns, with four per level, beside its own
/// indentation: for a well-indented sample they agree.
fn columns(language: &str, text: &str) -> Vec<(usize, Option<usize>)> {
    let (syntax, buffer) = parse(language, text);
    let lines: Vec<&str> = text.split('\n').collect();
    let indent = |row: usize| lines[row].len() - lines[row].trim_start().len();
    (0..lines.len())
        .filter(|&row| !lines[row].trim().is_empty())
        .map(|row| {
            let suggested = match suggest_indent(&syntax, buffer.text(), row as u32) {
                Some(Zero) => Some(0),
                Some(Relative { basis_row, delta }) => {
                    Some((indent(basis_row as usize) as i32 + 4 * delta) as usize)
                }
                None => None,
            };
            (indent(row), suggested)
        })
        .collect()
}

#[track_caller]
fn assert_reindents(language: &str, text: &str) {
    for (row, (actual, suggested)) in columns(language, text).into_iter().enumerate() {
        assert_eq!(
            Some(actual),
            suggested,
            "{language} non-blank row #{row}\n{text}"
        );
    }
}

/// Checks a well-indented sample: it parses without errors, and a blank row
/// inserted after each row gets the next row's indentation, or a level more
/// when the next row closes a region it's in. Rows in `ambiguous` are left out: the
/// next row could as well be a level deeper, as after a Python block's last
/// row.
fn blank_row_failures(language: &str, text: &str, ambiguous: &[u32]) -> Vec<String> {
    let mut failures = Vec::new();
    let (syntax, _) = parse(language, text);
    let root = syntax.tree().unwrap().root_node();
    if root.has_error() {
        return vec![format!("{language} sample has errors: {}", root.to_sexp())];
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let indent = |row: usize| lines[row].len() - lines[row].trim_start().len();
    for row in 0..lines.len() - 1 {
        if ambiguous.contains(&(row as u32)) || lines[row + 1].trim().is_empty() {
            continue;
        }
        let next = row + 1;
        // Before a closer the blank row is inside what it closes, but before
        // an `else` after a `}` it's outside the block the `}` ends.
        let bracket =
            lines[next].trim_start().starts_with([')', ']', '}']) || lines[next].trim() == "end";
        let after_closer = closer(language, text, row as u32).is_some();
        let closes = closer(language, text, next as u32).is_some() && (bracket || !after_closer);
        let expected = indent(next) + if closes { 4 } else { 0 };
        let mut with_blank = lines.clone();
        with_blank.insert(next, "");
        let suggested = match at(language, &with_blank.join("\n"), next as u32) {
            Some(Zero) => Some(0),
            Some(Relative { basis_row, delta }) => {
                Some((indent(basis_row as usize) as i32 + 4 * delta) as usize)
            }
            None => None,
        };
        if suggested != Some(expected) {
            failures.push(format!(
                "{language}: a blank row after row {row} `{}`: {suggested:?}, not {expected}",
                lines[row]
            ));
        }
    }
    failures
}

#[test]
fn blank_rows_take_the_next_rows_indentation() {
    let mut failures = Vec::new();
    let samples = SAMPLES
        .iter()
        .map(|&(language, text)| (language, text, &[][..]));
    for (language, text, ambiguous) in samples.chain(SHAPES.iter().copied()) {
        if language != "python" {
            failures.extend(blank_row_failures(language, text, ambiguous));
        }
        for (row, (actual, suggested)) in columns(language, text).into_iter().enumerate() {
            if Some(actual) != suggested {
                failures.push(format!(
                    "{language}: non-blank row #{row}: {suggested:?}, not {actual}"
                ));
            }
        }
    }
    failures.extend(blank_row_failures("python", PYTHON, &[]));
    // After `pass`, the `except` comes out a level, or the block goes on.
    failures.extend(blank_row_failures("python", PYTHON_SHAPES, &[20]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// For each row of `text`: the suggestion for a new blank row after it, as
/// `(basis_row, delta)`, and the row [`closer_outdents`] names for it.
type Expected<'a> = &'a [(u32, (u32, i32), Option<u32>)];

#[track_caller]
fn assert_rows(language: &str, text: &str, expected: Expected) {
    for &(row, (basis_row, delta), closes) in expected {
        assert_eq!(
            after(language, text, row),
            rel(basis_row, delta),
            "{language}: a new row after row {row}"
        );
        assert_eq!(
            closer(language, text, row),
            closes,
            "{language}: row {row} closes"
        );
    }
}

#[test]
fn well_indented_rows_keep_their_indentation() {
    for (language, text) in SAMPLES {
        assert_reindents(language, text);
    }
}

#[test]
fn rust_rows() {
    assert_rows(
        "rust",
        RUST,
        &[
            // After an opener: its row plus one.
            (0, (0, 1), None),
            (5, (5, 1), None),
            (9, (9, 1), None),
            // After a one-row statement: the same.
            (1, (1, 0), None),
            (3, (3, 0), None),
            // Inside a block.
            (6, (5, 1), None),
            (10, (9, 1), None),
            // Closers line up with their opener's row; `} else {` with the
            // `if`'s, and so does the row after it, one deeper.
            (4, (2, 1), Some(2)),
            (7, (5, 0), Some(5)),
            (8, (8, 0), Some(4)),
            (11, (9, 0), Some(9)),
            (15, (15, 1), Some(12)),
            (18, (18, 0), Some(0)),
        ],
    );
}

#[test]
fn javascript_rows() {
    assert_rows(
        "javascript",
        JAVASCRIPT,
        &[
            (0, (0, 1), None),
            (5, (5, 1), None),
            (9, (9, 1), None),
            (12, (12, 1), None),
            (1, (1, 0), None),
            (3, (3, 0), None),
            (6, (5, 1), None),
            (13, (12, 1), None),
            (4, (2, 1), Some(2)),
            (7, (5, 0), Some(5)),
            (11, (9, 0), Some(9)),
            (14, (12, 0), Some(12)),
            // An unbraced `if`'s body, and the row after it.
            (15, (15, 1), None),
            (16, (16, -1), None),
            (20, (20, 1), Some(17)),
            (24, (24, 0), Some(0)),
        ],
    );
}

#[test]
fn typescript_rows() {
    assert_rows(
        "typescript",
        TYPESCRIPT,
        &[
            (0, (0, 1), None),
            (1, (0, 1), None),
            (2, (2, 0), Some(0)),
            (8, (8, 1), None),
            (9, (8, 1), None),
            (10, (8, 0), Some(8)),
            (12, (12, 1), None),
            (14, (12, 0), Some(12)),
            (7, (7, 1), Some(4)),
        ],
    );
    assert_rows(
        "tsx",
        TSX,
        &[
            (0, (0, 1), None),
            (1, (1, 1), None),
            (3, (1, 0), Some(1)),
            (4, (4, 1), None),
            (5, (5, 1), None),
            (7, (7, 0), Some(5)),
            (8, (4, 0), Some(4)),
        ],
    );
}

#[test]
fn go_rows() {
    assert_rows(
        "go",
        GO,
        &[
            (0, (0, 1), None),
            (5, (5, 1), None),
            (9, (9, 1), None),
            (1, (1, 0), None),
            (3, (3, 0), None),
            (6, (5, 1), None),
            (4, (2, 1), Some(2)),
            (7, (7, 0), Some(5)),
            (11, (11, 0), Some(9)),
            (12, (12, 0), None),
            (13, (13, 0), Some(0)),
        ],
    );
}

#[test]
fn c_rows() {
    assert_rows(
        "c",
        C,
        &[
            (1, (1, 1), None),
            (6, (6, 1), None),
            (10, (10, 1), None),
            (2, (2, 0), None),
            (4, (4, 0), None),
            (7, (7, 0), None),
            (5, (3, 1), Some(3)),
            (8, (6, 0), Some(6)),
            (12, (10, 0), Some(10)),
            (16, (16, 1), Some(13)),
            (20, (20, 0), Some(1)),
        ],
    );
    assert_rows(
        "cpp",
        CPP,
        &[
            (0, (0, 1), None),
            (1, (0, 1), Some(0)),
            (2, (2, 1), None),
            (7, (7, 1), None),
            (4, (4, 0), None),
            (9, (7, 0), Some(7)),
            (6, (6, 1), Some(3)),
            (13, (0, 0), Some(0)),
        ],
    );
}

#[test]
fn java_rows() {
    assert_rows(
        "java",
        JAVA,
        &[
            (0, (0, 1), None),
            (1, (1, 1), None),
            (6, (6, 1), None),
            (10, (10, 1), None),
            (2, (2, 0), None),
            (4, (4, 0), None),
            (11, (10, 1), None),
            (5, (3, 1), Some(3)),
            (8, (6, 0), Some(6)),
            (12, (10, 0), Some(10)),
            (16, (13, 1), Some(13)),
            (20, (20, 0), Some(0)),
        ],
    );
}

#[test]
fn json_rows() {
    assert_rows(
        "json",
        JSON,
        &[
            (0, (0, 1), None),
            (1, (1, 1), None),
            (5, (5, 1), None),
            (2, (1, 1), None),
            (8, (8, 0), None),
            (4, (0, 1), Some(1)),
            (7, (0, 1), Some(5)),
            (9, (9, 0), Some(0)),
        ],
    );
}

#[test]
fn lua_rows() {
    assert_rows(
        "lua",
        LUA,
        &[
            (0, (0, 1), None),
            (2, (2, 1), None),
            (5, (5, 1), None),
            (9, (9, 1), None),
            (1, (1, 0), None),
            (3, (3, 0), None),
            (10, (9, 1), None),
            (4, (4, 1), Some(2)),
            (7, (7, 0), Some(5)),
            (8, (8, 0), Some(2)),
            (11, (11, 0), Some(9)),
            (13, (13, 0), Some(0)),
        ],
    );
}

#[test]
fn css_rows() {
    assert_rows(
        "css",
        CSS,
        &[
            (0, (0, 1), None),
            (3, (3, 1), None),
            (4, (4, 1), None),
            (1, (1, 0), None),
            (5, (5, 0), None),
            (2, (2, 0), Some(0)),
            (6, (6, 0), Some(4)),
            (7, (7, 0), Some(3)),
        ],
    );
}

#[test]
fn python_blocks() {
    assert_rows(
        "python",
        PYTHON,
        &[
            // After a line ending in `:`, one level more.
            (0, (0, 1), None),
            (1, (1, 1), None),
            (3, (3, 1), None),
            // After `return` or `pass`, one level less.
            (2, (2, -1), None),
            (4, (4, -1), None),
            (10, (10, -1), Some(8)),
            // Brackets.
            (5, (5, 1), None),
            (6, (5, 1), None),
            (7, (7, 0), Some(5)),
            (8, (8, 1), None),
        ],
    );
    let text = "if a:\n    b = 1\nelse:\n    c = 2\n";
    assert_eq!(at("python", text, 2), rel(0, 0));
    assert_eq!(closer("python", text, 2), Some(0));
    assert_eq!(after("python", text, 1), rel(1, 0));
    assert_eq!(after("python", text, 2), rel(2, 1));
    // A body not yet typed.
    assert_eq!(after("python", "def f():", 0), rel(0, 1));
    assert_eq!(after("python", "def f(a):\n    while a:", 1), rel(1, 1));
    // `break` dedents too, and a `return` over several rows only after its
    // last.
    let text = "for a in b:\n    if a:\n        break\n    return f(\n        a,\n    )";
    assert_eq!(after("python", text, 2), rel(2, -1));
    assert_eq!(after("python", text, 3), rel(3, 1));
    assert_eq!(after("python", text, 5), rel(5, -1));
}

/// The columns `suggestion` comes to in `text`, at four per level.
fn columns_of(text: &str, suggestion: Option<IndentSuggestion>) -> Option<usize> {
    let lines: Vec<&str> = text.split('\n').collect();
    match suggestion? {
        Zero => Some(0),
        Relative { basis_row, delta } => {
            let line = lines[basis_row as usize];
            let indent = line.len() - line.trim_start().len();
            usize::try_from(indent as i32 + 4 * delta).ok()
        }
    }
}

#[test]
fn finished_statements_and_loops() {
    // After an unbraced `else` body, back to the block's level.
    let text = "int f(int a) {\n    if (a) { return 1; } else\n        return 2;\n\n}";
    for language in ["c", "cpp"] {
        assert_eq!(
            columns_of(text, at(language, text, 3)),
            Some(4),
            "{language}"
        );
    }
    let text = "function f(a) {\n    if (a) { x = 0 } else\n        x = 1\n\n}";
    for language in ["javascript", "typescript", "tsx"] {
        assert_eq!(
            columns_of(text, at(language, text, 3)),
            Some(4),
            "{language}"
        );
    }
    // An unbraced loop body, its row and the rows after.
    let text = "void f() {\n    for (int i=0; i<3; ++i)\n        g();\n}";
    assert_eq!(columns_of(text, at("c", text, 2)), Some(8));
    assert_eq!(columns_of(text, after("c", text, 1)), Some(8));
    assert_eq!(columns_of(text, after("c", text, 2)), Some(4));
    let text = "void f() {\n    if (a)\n        for (int i=0; i<3; ++i)\n            g();\n}";
    assert_eq!(columns_of(text, at("c", text, 2)), Some(8));
    assert_eq!(columns_of(text, at("c", text, 3)), Some(12));
    assert_eq!(columns_of(text, after("c", text, 3)), Some(4));
    // A `break` in a loop stays in the loop; one ending a case ends it.
    let text = "void f() {\n    while (a) {\n        break;\n\n    }\n}";
    assert_eq!(columns_of(text, at("c", text, 3)), Some(8));
    let text = "void f() {\n    switch (a) {\n        case 1:\n            g();\n            break;\n\n    }\n}";
    assert_eq!(columns_of(text, at("c", text, 5)), Some(8));
}

#[test]
fn wide_brackets() {
    let fields: String = (0..2050).map(|i| format!("        f{i}: 1,\n")).collect();
    let text = format!("fn f() {{\n    let s = S {{\n{fields}    }};\n}}");
    let closing = 2052;
    assert_eq!(columns_of(&text, after("rust", &text, 2)), Some(8));
    assert_eq!(after("rust", &text, 1), rel(1, 1));
    assert_eq!(closer("rust", &text, closing), Some(1));
    assert_eq!(at("rust", &text, closing), rel(1, 0));
    let args = "1px, ".repeat(2100);
    let text = format!("a {{\n    b: f(\n        {args}1px\n    );\n}}");
    assert_eq!(closer("css", &text, 3), Some(1));
    assert_eq!(at("css", &text, 3), rel(1, 0));
    assert_eq!(at("css", &text, 2), rel(1, 1));
}

#[test]
fn wide_malformed_brackets() {
    // One flat error of N openers, a `;`, N closers and N commas. The work
    // on it grows linearly: quadratic, as it was, this took minutes.
    const N: usize = 50_000;
    let text = format!("{};\n{}\n{}\n", "(".repeat(N), ")".repeat(N), ",".repeat(N));
    let (syntax, buffer) = parse("c", &text);
    let root = syntax.tree().unwrap().root_node();
    assert!(root.is_error() && root.child_count() as usize > 3 * N);
    let start = std::time::Instant::now();
    for row in 0..4 {
        assert!(suggest_indent(&syntax, buffer.text(), row).is_some());
    }
    assert_eq!(closer_outdents(&syntax, buffer.text(), 1), Some(0));
    assert_eq!(closer_outdents(&syntax, buffer.text(), 2), None);
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 30, "took {elapsed:?}");
}

/// Unbraced loops whose headers span rows, in an unbraced `if` body or with
/// the `(` on a row of its own.
const LOOP_HEADERS: &[(&str, &str)] = &[
    (
        "c",
        "\
void f() {
    if (a)
        for (
            int i=0; i<3; ++i
        )
            g();
    for
    (
        i=0; i<3; ++i
    )
        g();
}",
    ),
    (
        "javascript",
        "\
function f() {
    if (a)
        for (
            let i = 0; i < 3; ++i
        )
            g();
    for
    (
        i = 0; i < 3; ++i
    )
        g();
}",
    ),
    (
        "java",
        "\
class A {
    void f() {
        if (a)
            for (
                int i = 0; i < 3; ++i
            )
                g();
        for
        (
            i = 0; i < 3; ++i
        )
            g();
    }
}",
    ),
];

#[test]
fn loop_headers_over_several_rows() {
    // The review's example: inside the header's brackets the `if` body's
    // level holds, and the loop's own level starts after the `)`.
    let text = LOOP_HEADERS[0].1;
    let expected = [
        (2, 8),
        (3, 12),
        (4, 8),
        (5, 12),
        (6, 4),
        (7, 4),
        (8, 8),
        (9, 4),
        (10, 8),
    ];
    for (row, columns) in expected {
        assert_eq!(
            columns_of(text, at("c", text, row)),
            Some(columns),
            "row {row}"
        );
    }
    assert_eq!(columns_of(text, after("c", text, 2)), Some(12));
    assert_eq!(columns_of(text, after("c", text, 6)), Some(4));
    assert_eq!(columns_of(text, after("c", text, 7)), Some(8));
    assert_eq!(closer("c", text, 4), Some(2));
    assert_eq!(closer("c", text, 9), Some(7));
    let mut failures = Vec::new();
    for &(language, text) in LOOP_HEADERS {
        let languages: &[&str] = match language {
            "c" => &["c", "cpp"],
            "javascript" => &["javascript", "typescript", "tsx"],
            _ => &["java"],
        };
        for language in languages {
            failures.extend(blank_row_failures(language, text, &[]));
            for (row, (actual, suggested)) in columns(language, text).into_iter().enumerate() {
                if Some(actual) != suggested {
                    failures.push(format!(
                        "{language}: row {row}: {suggested:?}, not {actual}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn unfinished_blocks() {
    // After a finished statement in a block whose `}` isn't typed yet, the
    // block's level holds.
    for (languages, text) in [
        (&["c", "cpp"][..], "void f() {\n    g();\n"),
        (
            &["c", "cpp"],
            "void f() {\n    if (a) {\n        g();\n    }\n",
        ),
        (
            &["javascript", "typescript", "tsx"],
            "function f() {\n    g();\n",
        ),
        (
            &["javascript", "typescript", "tsx"],
            "function f() {\n    if (a) {\n        g();\n    }\n",
        ),
    ] {
        let row = text.matches('\n').count() as u32;
        for language in languages {
            assert_eq!(
                columns_of(text, at(language, text, row)),
                Some(4),
                "{language}: {text:?}"
            );
        }
    }
    let text = "class A {\n    void f() {\n        g();\n";
    assert_eq!(columns_of(text, at("java", text, 3)), Some(8));
    // A finished loop body stops indenting, though the block around it is
    // unfinished, or has an error elsewhere.
    let text = "class A {\n    void f() {\n        for (int i=0; i<3; ++i)\n            g();\n";
    assert_eq!(columns_of(text, at("java", text, 4)), Some(8));
    let text = "void f() {\n    while (a)\n        g();\n";
    assert_eq!(columns_of(text, at("c", text, 3)), Some(4));
    let text = "void f() {\n    for (i=0; i<3; ++i)\n        g();\n\n    x = ;\n}";
    assert_eq!(columns_of(text, at("c", text, 3)), Some(4));
    let text = "function f() {\n    while (a)\n        g();\n";
    assert_eq!(columns_of(text, at("javascript", text, 3)), Some(4));
}

#[test]
fn brackets_in_an_error() {
    // Two openers on one row and one closer: one opener stays open.
    let text = "((;\n\n)\n";
    assert_eq!(at("c", text, 1), rel(0, 1));
    assert_eq!(closer("c", text, 2), Some(0));
    assert_eq!(at("c", text, 2), rel(0, 0));
    // Nested, each on its own row: each closer names its own opener.
    let text = "(\n    (\n        ;\n    )\n)\n";
    let (syntax, _) = parse("c", text);
    assert!(syntax.tree().unwrap().root_node().has_error());
    assert_eq!(closer("c", text, 3), Some(1));
    assert_eq!(closer("c", text, 4), Some(0));
    for row in 1..5 {
        let line = text.split('\n').nth(row).unwrap();
        let indent = line.len() - line.trim_start().len();
        assert_eq!(
            columns_of(text, at("c", text, row as u32)),
            Some(indent),
            "row {row}"
        );
    }
    assert_eq!(columns_of(text, after("c", text, 1)), Some(8));
    assert_eq!(columns_of(text, after("c", text, 3)), Some(4));
}

/// `do` loops and C++'s range-based `for`, with unbraced bodies.
const DO_LOOPS: &[(&[&str], &str)] = &[
    (
        &["c", "cpp"],
        "\
void f() {
    do
        g();
    while (a);
    do {
        g();
    } while (a);
    g();
}",
    ),
    (
        &["cpp"],
        "\
void f() {
    for (auto x : xs)
        g(x);
    for (auto x : xs) {
        g(x);
    }
    g();
}",
    ),
    (
        &["javascript", "typescript", "tsx"],
        "\
function f() {
    do
        g();
    while (a);
    do {
        g();
    } while (a);
    g();
}",
    ),
    (
        &["java"],
        "\
class A {
    void f() {
        do
            g();
        while (a);
        do {
            g();
        } while (a);
        g();
    }
}",
    ),
];

#[test]
fn do_loops_and_range_fors() {
    let mut failures = Vec::new();
    for &(languages, text) in DO_LOOPS {
        for language in languages {
            failures.extend(blank_row_failures(language, text, &[]));
            for (row, (actual, suggested)) in columns(language, text).into_iter().enumerate() {
                if Some(actual) != suggested {
                    failures.push(format!(
                        "{language}: row {row}: {suggested:?}, not {actual}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn errors_inside_finished_constructs() {
    // An error inside a loop header, an unbraced body or a closed block
    // doesn't keep it indenting the rows after it.
    let cases: &[(&[&str], &str, usize)] = &[
        (
            &["c", "cpp"],
            "void f() {\n    for (i = ; i < 3; ++i)\n        g();\n\n}",
            4,
        ),
        (
            &["c", "cpp"],
            "void f() {\n    if (a)\n        x = ;\n\n}",
            4,
        ),
        (
            &["javascript", "typescript", "tsx"],
            "function f() {\n    for (i = ; i < 3; ++i)\n        g();\n\n}",
            4,
        ),
        (
            &["javascript", "typescript", "tsx"],
            "function f() {\n    if (a)\n        x = ;\n\n}",
            4,
        ),
        (
            &["java"],
            "class A {\n    void f() {\n        if (a)\n            x = ;\n\n    }\n}",
            8,
        ),
        (
            &["java"],
            "class A {\n    void f() {\n        for (i = ; i < 3; ++i)\n            g();\n\n    }\n}",
            8,
        ),
        (&["c", "cpp"], "void f() {\n    x = ;\n}\n", 0),
        (
            &["javascript", "typescript", "tsx"],
            "function f() {\n    x = ;\n}\n",
            0,
        ),
        (
            &["java"],
            "class A {\n    void f() {\n        x = ;\n    }\n\n}",
            4,
        ),
        (&["rust"], "fn f() {\n    let x = ;\n}\n", 0),
        (&["go"], "func f() {\n    x = ;\n}\n", 0),
        (&["json"], "{\n    \"a\": ,\n}\n", 0),
        (&["css"], "a {\n    color: ;;\n}\n", 0),
    ];
    for &(languages, text, expected) in cases {
        let row = text.split('\n').position(|line| line.is_empty()).unwrap() as u32;
        for language in languages {
            let (syntax, _) = parse(language, text);
            let root = syntax.tree().unwrap().root_node();
            assert!(root.has_error(), "{language}: {text:?} has no error");
            let suggested = columns_of(text, at(language, text, row));
            assert_eq!(suggested, Some(expected), "{language}: {text:?}");
        }
    }
}

#[test]
fn wide_malformed_brackets_on_many_rows() {
    // Unclosed openers each on a row of their own: a level a row, and the
    // rows counted stay linear.
    const N: usize = 100_000;
    let text = "(\n".repeat(N);
    let (syntax, buffer) = parse("c", &text);
    assert!(syntax.tree().unwrap().root_node().has_error());
    let start = std::time::Instant::now();
    let suggested = suggest_indent(&syntax, buffer.text(), N as u32);
    assert_eq!(suggested, rel(N as u32 - 1, 1));
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 30, "took {elapsed:?}");
}

#[test]
fn go_switch_braces_on_their_own_rows() {
    // gofmt keeps cases at the switch's level, whichever row its `{` is on.
    assert_eq!(closer("go", GO_SWITCH, 7), Some(4));
    assert_eq!(at("go", GO_SWITCH, 7), rel(3, 0));
    assert_eq!(at("go", GO_SWITCH, 5), rel(3, 0));
    assert_eq!(at("go", GO_SWITCH, 6), rel(5, 1));
}

/// The first row of `text` from `from` on starting, after its indentation,
/// with `start`.
fn row_of(text: &str, start: &str, from: u32) -> u32 {
    let mut lines = text.split('\n').skip(from as usize);
    from + lines
        .position(|line| line.trim_start().starts_with(start))
        .unwrap() as u32
}

/// Each kind of closing bracket names its opener's row, whatever holds it.
/// The rows' own indentation is checked with the rest of the samples'.
#[test]
fn closers_of_every_bracket() {
    for (language, text, closers) in [
        (
            "rust",
            RUST_SHAPES,
            &[("];", "let a = x["), ("),", "B("), (");", "let t = (")][..],
        ),
        (
            "javascript",
            JAVASCRIPT_SHAPES,
            &[("];", "const a = x["), ("],", "k: ["), (")", "g(")],
        ),
        ("go", GO_SHAPES, &[("](a)", "h["), (")", "g(")]),
        (
            "c",
            C_SHAPES,
            &[("];", "int w["), ("};", "int v[]"), (");", "g(")],
        ),
        ("java", JAVA_SHAPES, &[("];", "int a = x["), (");", "g(")]),
        ("lua", LUA_SHAPES, &[("]", "local a = x["), (")", "g(")]),
        ("css", CSS_SHAPES, &[("] {", "a["), (");", "width: calc(")]),
        (
            "python",
            PYTHON_SHAPES,
            &[("]", "a = x["), ("),", "\"k\": ("), (")", "g(")],
        ),
    ] {
        for &(closing, opening) in closers {
            let opener = row_of(text, opening, 0);
            let row = row_of(text, closing, opener + 1);
            assert_eq!(
                closer(language, text, row),
                Some(opener),
                "{language}: `{closing}`"
            );
        }
    }
}

#[test]
fn closers_at_their_own_row() {
    // A closer, or an `else`, already on its row lines up with its opener.
    assert_eq!(at("rust", RUST, 8), rel(2, 0));
    assert_eq!(at("rust", RUST, 15), rel(12, 0));
    assert_eq!(at("rust", RUST, 7), rel(5, 0));
    assert_eq!(at("c", C, 16), rel(13, 0));
    assert_eq!(at("java", JAVA, 16), rel(13, 0));
    assert_eq!(at("javascript", JAVASCRIPT, 20), rel(17, 0));
    assert_eq!(at("go", GO, 7), rel(5, 0));
    assert_eq!(at("lua", LUA, 4), rel(2, 0));
    // Rows that start with something else don't close.
    assert_eq!(closer("rust", RUST, 3), None);
    assert_eq!(closer("rust", "fn f() {\n\n}", 1), None);
}

#[test]
fn else_after_an_unbraced_body() {
    // The pattern needs the `if`'s body, rows before the `else`.
    let text = "if (a)\n    g(\n        a,\n    );\nelse\n    h();";
    assert_eq!(at("javascript", text, 4), rel(0, 0));
    assert_eq!(closer("javascript", text, 4), Some(0));
    assert_eq!(after("javascript", text, 4), rel(4, 1));
    let text = "void f(int a) {\n    if (a)\n        g();\n    else\n        h();\n}";
    assert_eq!(at("c", text, 3), rel(1, 0));
    assert_eq!(closer("c", text, 3), Some(1));
}

#[test]
fn unclosed_openers() {
    assert_eq!(after("rust", "fn f() {", 0), rel(0, 1));
    assert_eq!(after("rust", "fn f() {\n    foo(", 1), rel(1, 1));
    assert_eq!(after("rust", "fn f() {\n    foo(\n}", 1), rel(1, 1));
    assert_eq!(after("javascript", "foo(", 0), rel(0, 1));
    assert_eq!(after("go", "func f() {", 0), rel(0, 1));
    assert_eq!(after("json", "{\n    \"a\": [", 1), rel(1, 1));
    assert_eq!(after("lua", "if a then", 0), rel(0, 1));
    assert_eq!(after("css", "a {", 0), rel(0, 1));
    // Nothing to go on: the row before is copied.
    assert_eq!(after("c", "void f(int a) {\n    if (a)", 1), rel(1, 0));
}

#[test]
fn zero_comments_and_strings() {
    // A preprocessor directive has no indentation; the row after it does.
    let text = "void f(void) {\n    int a;\n#if X\n    int b;\n#endif\n}";
    assert_eq!(at("c", text, 2), Some(Zero));
    assert_eq!(at("cpp", text, 4), Some(Zero));
    assert_eq!(after("c", text, 4), rel(4, 1));
    assert_eq!(at("c", text, 3), rel(2, 1));
    // Inside a docstring or a block comment, the row before is copied.
    let text = "def f(a):\n    \"\"\"Doc\n      more\n    \"\"\"\n    return a";
    assert_eq!(at("python", text, 2), rel(1, 0));
    assert_eq!(after("python", text, 2), rel(2, 0));
    assert_eq!(after("python", text, 3), rel(1, 0));
    let text = "/**\n * doc\n */\nf();";
    assert_eq!(after("javascript", text, 1), rel(1, 0));
    assert_eq!(after("javascript", text, 3), rel(3, 0));
    // A trailing comment is passed over.
    assert_eq!(after("rust", "fn f() { // c\n}", 0), rel(0, 1));
}

#[test]
fn no_suggestion() {
    // Without a tree.
    let buffer = Buffer::new("fn f() {\n\n}");
    let syntax = Syntax::new(language_by_name("rust").unwrap(), &buffer.snapshot());
    assert_eq!(suggest_indent(&syntax, buffer.text(), 1), None);
    assert_eq!(closer_outdents(&syntax, buffer.text(), 2), None);
    // Without an indents query.
    assert_eq!(after("bash", "if a; then", 0), None);
    assert_eq!(after("toml", "[a]", 0), None);
    assert_eq!(closer("yaml", "a:\n  - b\n]", 2), None);
    // In an injected layer: a tagged template's CSS. Its first and last
    // rows are the host's.
    let text = "const s = css`\n  color: red;\n\n  margin: 0;\n`;\nf();";
    assert_eq!(at("javascript", text, 1), None);
    assert_eq!(at("javascript", text, 2), None);
    assert_eq!(at("javascript", text, 3), None);
    assert_eq!(after("javascript", text, 0), None);
    assert_eq!(at("javascript", text, 4), rel(3, 0));
    assert_eq!(at("javascript", text, 5), rel(0, 0));
    // A row past the end.
    assert_eq!(at("rust", "fn f() {}", 1), None);
}

#[test]
fn indents_files_name_their_source() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/queries/indents");
    let third_party =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../THIRD_PARTY.md"))
            .unwrap();
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    for language in [
        "rust",
        "python",
        "javascript",
        "typescript",
        "tsx",
        "go",
        "c",
        "cpp",
        "java",
        "json",
        "lua",
        "css",
    ] {
        assert!(files.contains(&format!("{language}.scm")), "{language}");
    }
    for file in files {
        let text = std::fs::read_to_string(format!("{dir}/{file}")).unwrap();
        let header: String = text
            .lines()
            .take_while(|line| line.starts_with(';'))
            .collect();
        for needle in [
            "nvim-treesitter",
            "commit 1a4f47b03dfe92d28de7d5654c67327ef008c9e6",
            "path runtime/queries/",
            "license Apache-2.0",
        ] {
            assert!(header.contains(needle), "{file} header lacks {needle}");
        }
        let row = format!("| `crates/fe-syntax/queries/indents/{file}` |");
        let row = third_party.lines().find(|line| line.starts_with(&row));
        let row = row.unwrap_or_else(|| panic!("THIRD_PARTY.md has no row for {file}"));
        assert!(
            row.contains("`1a4f47b03dfe92d28de7d5654c67327ef008c9e6`"),
            "{row}"
        );
        assert!(row.contains("Apache-2.0"), "{row}");
    }
}

const SAMPLES: &[(&str, &str)] = &[
    ("rust", RUST),
    ("python", PYTHON),
    ("javascript", JAVASCRIPT),
    ("typescript", TYPESCRIPT),
    ("tsx", TSX),
    ("go", GO),
    ("c", C),
    ("cpp", CPP),
    ("java", JAVA),
    ("json", JSON),
    ("lua", LUA),
    ("css", CSS),
];

/// Brackets of every kind, on the opener's row and on a row of their own,
/// and statements over several rows.
const SHAPES: &[(&str, &str, &[u32])] = &[
    ("rust", RUST_SHAPES, &[]),
    // The rows left out: after a case's last statement, the case could go
    // on; a blank row between an unbraced body and its `else` could be in
    // the body or not.
    ("javascript", JAVASCRIPT_SHAPES, &[22, 29, 34]),
    ("typescript", JAVASCRIPT_SHAPES, &[22, 29, 34]),
    ("tsx", JAVASCRIPT_SHAPES, &[22, 29, 34]),
    ("go", GO_SHAPES, &[23]),
    ("go", GO_SWITCH, &[]),
    ("typescript", TYPESCRIPT_SHAPES, &[]),
    ("tsx", TYPESCRIPT_SHAPES, &[]),
    ("c", C_SHAPES, &[28, 33, 52]),
    ("cpp", C_SHAPES, &[28, 33, 52]),
    ("java", JAVA_SHAPES, &[26, 31]),
    ("lua", LUA_SHAPES, &[]),
    ("css", CSS_SHAPES, &[]),
];

const RUST_SHAPES: &str = "\
mod m
{
    struct S
    {
        a: u8,
    }
    enum E
    {
        A,
        B(
            u8,
        ),
    }
    impl S
    {
        fn f(&self, x: &[u8]) -> u8
        {
            let a = x[
                0
            ];
            let s = S
            {
                a: 1,
            };
            let t = (
                1,
                2,
            );
            let v = vec![
                1,
            ];
            let b = a
                + 1;
            match a {
                0 => 1,
                _ => {
                    2
                }
            }
        }
    }
    trait T
    {
        fn g(
            &self,
        );
    }
    fn h<U>(u: U) -> U
    where
        U: Clone,
    {
        if true {
            u
        }
        else
        {
            u
        }
    }
}";

const JAVASCRIPT_SHAPES: &str = "\
function f(x)
{
    const a = x[
        0
    ];
    const b = a +
        1
    const c = {
        k: [
            1,
        ],
    }
    g(
        a,
        b,
    )
    switch (a)
    {
        case 1:
            g(a);
            break;
        default:
            g(b);
    }
    if (a) {
        g(a);
    } else
        g(b);
    if (a)
        g(a);
    else {
        g(b);
    }
    if (a)
        g(a);
    else
        g(b);
    if (a)
    {
        g(a);
    }
    else
    {
        g(b);
    }
    if (a) {
        g(a);
    } else if (b) {
        g(b);
    } else {
        g(c);
    }
    for (let i = 0; i < 2; i++)
        g(i);
    if (a)
        for (const x of c)
            g(x);
    while (a) {
        g(a);
        break;
    }
    if (a) { g(a) } else
        x = 1
    return a;
}";

const GO_SHAPES: &str = "\
package p

func f(x []int) int {
    a := x[
        0]
    b := x[
        1:2]
    c := map[string]int{
        \"k\": 1,
    }
    g(
        a,
        b,
    )
    if a > 0 {
        g(c)
    } else if b > 0 {
        g(b)
    } else {
        g(a)
    }
    switch a {
    case 1:
        g(a)
    default:
        g(b)
    }
    h[
        int,
    ](a)
    return a
}";

const GO_SWITCH: &str = "\
package p

func f(a bool) {
    switch
    {
    case a:
        g(a)
    }
}";

const TYPESCRIPT_SHAPES: &str = "\
interface A
{
    a: number;
}
enum E
{
    A,
    B,
}
type T = {
    a: number;
};";

const C_SHAPES: &str = "\
int f(int *x)
{
    int a = x[
        0
    ];
    int v[] = {
        1,
        2,
    };
    int w[
        2
    ];
    g(
        a
    );
    if (a)
    {
        g(a);
    }
    else
    {
        g(v[0]);
    }
    if (a) {
        g(a);
    } else
        g(a);
    if (a)
        g(a);
    else {
        g(a);
    }
    if (a)
        g(a);
    else
        g(a);
    for (a = 0; a < 2; a++)
        g(a);
    if (a)
        for (a = 0; a < 2; a++)
            g(a);
    while (a) {
        g(a);
        break;
    }
    if (a) { g(a); } else
        return a;
    switch (a) {
        case 1:
            g(a);
            break;
        default:
            g(a);
    }
    return a;
}";

const JAVA_SHAPES: &str = "\
class A
{
    int f(int[] x)
    {
        int a = x[
            0
        ];
        int[] v = {
            1,
        };
        g(
            a
        );
        if (a > 0)
        {
            g(a);
        }
        else
        {
            g(a);
        }
        if (a > 0) {
            g(a);
        } else
            g(a);
        if (a > 0)
            g(a);
        else {
            g(a);
        }
        if (a > 0)
            g(a);
        else
            g(a);
        for (int i = 0; i < 2; i++)
            g(i);
        if (a > 0)
            for (int y : x)
                g(y);
        while (a > 0) {
            g(a);
            break;
        }
        if (a > 0) { g(a); } else
            return a;
        return a;
    }
}";

const LUA_SHAPES: &str = "\
local function f(x)
    local a = x[
        1
    ]
    local t = {
        a = {
            1,
        },
    }
    g(
        a
    )
    if a then
        g(a)
    elseif t then
        g(t)
    else
        g(x)
    end
    for i = 1, 2 do
        g(i)
    end
    while a do
        g(a)
    end
    return a
        + 1
end";

const CSS_SHAPES: &str = "\
a[
    href
] {
    width: calc(
        1px + 2px
    );
    color:
        red;
}
@media (min-width: 1px)
{
    b
    {
        color: blue;
    }
}";

const PYTHON_SHAPES: &str = "\
def f(x):
    a = x[
        0
    ]
    b = {
        \"k\": (
            1,
            2,
        ),
    }
    c = a + \\
        1
    g(
        a,
    )
    while a:
        g(a)
        break
    try:
        g(a)
        pass
    except E:
        raise
    return g(
        a,
    )";

const JAVASCRIPT: &str = "\
function f(a) {
    const b = 1;
    if (a > b) {
        g(a);
    } else {
        h(
            a,
        );
    }
    const v = [
        1,
    ];
    const o = {
        k: 1,
    };
    if (a)
        g(a);
    if (a) {
        g(a);
    }
    else {
        h(a);
    }
    return b;
}";

const TYPESCRIPT: &str = "\
interface A {
    a: number;
}
function f(a: number): number {
    if (a > 1) {
        g(a);
    }
    else {
        h(
            a,
        );
    }
    return [
        1,
    ].length;
}";

const TSX: &str = "\
function F(a: number) {
    const v = [
        a,
    ];
    return (
        <div>
            {a}
        </div>
    );
}";

const GO: &str = "\
func f(a int) {
    b := 1
    if a > b {
        g(a)
    } else {
        h(
            a,
        )
    }
    v := []int{
        1,
    }
    g(v)
}";

const C: &str = "\
#define N 1
int f(int a) {
    int b = 1;
    if (a > b) {
        g(a);
    } else {
        h(
            a
        );
    }
    int v[] = {
        1,
    };
    if (a) {
        g(a);
    }
    else {
        h(a);
    }
    return b;
}";

const CPP: &str = "\
class A {
public:
    int f(int a) {
        if (a) {
            g(a);
        }
        else {
            h(
                a
            );
        }
        return a;
    }
};";

const JAVA: &str = "\
class A {
    void f(int a) {
        int b = 1;
        if (a > b) {
            g(a);
        } else {
            h(
                a
            );
        }
        int[] v = {
            1,
        };
        if (a > 0) {
            g(a);
        }
        else {
            h(a);
        }
    }
}";

const JSON: &str = "\
{
    \"a\": [
        1,
        2
    ],
    \"b\": {
        \"c\": 1
    },
    \"d\": 2
}";

const LUA: &str = "\
local function f(a)
    local b = 1
    if a > b then
        g(a)
    else
        h(
            a
        )
    end
    local t = {
        1,
    }
    return b
end";

const CSS: &str = "\
a {
    color: red;
}
@media (min-width: 1px) {
    b {
        color: blue;
    }
}";

const RUST: &str = "\
fn f(a: u8) {
    let b = 1;
    if a > b {
        g(a);
    } else {
        h(
            a,
        );
    }
    let v = [
        1,
    ];
    if a {
        g(a);
    }
    else {
        h(a);
    }
}";

const PYTHON: &str = "\
def f(a):
    if a:
        return 1
    for x in a:
        pass
    b = g(
        a,
    )
    return [
        1,
    ]";
