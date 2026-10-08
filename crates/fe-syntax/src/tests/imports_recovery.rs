//! TSX's `<Foo extends …>` misread (a JSX attribute named `extends` read as a
//! type parameter's constraint), recovered by TypeScript's own rule: only an
//! `extends` followed by `=`, `>` or `/` is an attribute.

use std::{cell::Cell, time::Duration};

use fe_buffer::Buffer;

use crate::{ImportsError, Syntax, language_by_name, module_specifiers};

thread_local! {
    /// The bytes the TSX recovery has read on this thread: each byte its scan
    /// passes, once however many lookaheads read it, and each byte given to
    /// a corrected parse.
    pub(crate) static RECOVERY_READS: Cell<usize> = const { Cell::new(0) };
}

/// A tree for `text`, parsed with no deadline, so a loaded machine can't
/// make the parse give up.
fn syntax(language: &str, text: &str) -> Syntax {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    assert!(syntax.parse_until(Duration::MAX, || false));
    syntax
}

fn specifiers(language: &str, text: &str) -> Vec<String> {
    let found = module_specifiers(&syntax(language, text), &mut || false).unwrap();
    found.into_iter().map(|r| text[r].to_string()).collect()
}

/// An attribute the grammar misreads, then an import it would swallow.
const ATTRIBUTE: &str = "const el = <Foo extends=\"/\" />; import a from './a';\n";
const IMPORT: &str = "import a from './a';\n";

#[test]
fn a_generic_arrow_after_a_jsx_extends_attribute_keeps_its_require() {
    let cases: [(&str, &[&str]); 5] = [
        (
            "const f = <T extends string>(x:T)=> require('real');\n",
            &["real", "./a"],
        ),
        (
            "const f = <T extends string>(x: T) => x; require('after');\n",
            &["after", "./a"],
        ),
        (
            "function g<T extends U = V>(x: T) { return require('inner'); }\n",
            &["inner", "./a"],
        ),
        (
            "class C<T extends D> { m() { return require('m'); } }\n",
            &["m", "./a"],
        ),
        ("type X = Foo<T extends string ? 1 : 2>;\n", &["./a"]),
    ];
    for (case, expected) in cases {
        let text = format!("const el = <Foo extends=\"/\" />;\n{case}{IMPORT}");
        assert_eq!(specifiers("tsx", &text), expected, "{case:?}");
    }
}

#[test]
fn apostrophes_outside_strings_keep_later_imports() {
    for before in [
        "const r = /'/;",
        "const p = <p>don't</p>;",
        "const t = `don't`;",
    ] {
        let text = format!("{before} {ATTRIBUTE}");
        assert_eq!(specifiers("tsx", &text), ["./a"], "{before:?}");
    }
}

#[test]
fn line_comments_end_at_every_line_terminator() {
    for terminator in ["\r", "\u{2028}", "\u{2029}"] {
        let text = format!("// c{terminator}{ATTRIBUTE}");
        // A buffer holds no CR (`Text::new` asserts it), so text with one is
        // loaded as a file is: a lone CR becomes a line feed.
        let buffer = Buffer::from_file_contents(&text);
        let mut syntax = Syntax::new(language_by_name("tsx").unwrap(), buffer.current());
        syntax.finish(syntax.parse_job().run());
        let found = module_specifiers(&syntax, &mut || false).unwrap();
        let text = buffer.text().to_string();
        let found: Vec<_> = found.into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["./a"], "{terminator:?}");
    }
}

#[test]
fn tag_names_with_combining_marks_and_joiners_keep_later_imports() {
    for name in ["Fo\u{301}o", "Fo\u{200d}o"] {
        let text = format!("const el = <{name} extends=\"/\" />;\n{IMPORT}");
        assert_eq!(specifiers("tsx", &text), ["./a"], "{name:?}");
    }
}

/// How many times `module_specifiers` polls a callback that turns true on
/// its 3rd poll, and what it returns.
fn cancelled_on_the_third_poll(
    text: &str,
) -> (Result<Vec<std::ops::Range<usize>>, ImportsError>, usize) {
    let syntax = syntax("tsx", text);
    let mut polls = 0;
    let result = module_specifiers(&syntax, &mut || {
        polls += 1;
        polls >= 3
    });
    (result, polls)
}

#[test]
fn a_cancel_stops_the_scan_in_a_long_gap() {
    let text = format!(
        "const x = <A extends=\"/\" />;\n{}{ATTRIBUTE}",
        "// padding\n".repeat(100_000)
    );
    assert!(text.len() > 1 << 20);
    let (result, polls) = cancelled_on_the_third_poll(&text);
    assert_eq!(result, Err(ImportsError::Cancelled));
    assert!(polls <= 4, "polled {polls} times");
    assert_eq!(specifiers("tsx", &text), ["./a"]);
}

#[test]
fn a_cancel_stops_the_scan_in_a_long_whitespace_run() {
    let text = format!(
        "const el = <A extends{}=\"/\" />;\n{IMPORT}",
        " ".repeat(1 << 20)
    );
    let (result, polls) = cancelled_on_the_third_poll(&text);
    assert_eq!(result, Err(ImportsError::Cancelled));
    assert!(polls <= 4, "polled {polls} times");
    assert_eq!(specifiers("tsx", &text), ["./a"]);
}

/// The modules a tsx `text` imports, checking that the recovery reads at
/// most 4 bytes per byte of it: one pass of the scan and one corrected parse
/// read 2, and at least the scan's pass must be counted. Work is counted, not timed, so load can't fail this. A recovery
/// that reads more is cancelled as soon as it passes the bound, so a
/// quadratic one fails at once rather than running for hours.
pub(crate) fn read_in_linear_time(text: &str) -> Vec<String> {
    let syntax = syntax("tsx", text);
    let bound = 4 * text.len();
    let before = RECOVERY_READS.with(Cell::get);
    let reads = || RECOVERY_READS.with(Cell::get) - before;
    let found = module_specifiers(&syntax, &mut || reads() > bound);
    let reads = reads();
    // (At least one pass: the recovery ran, and its reads were counted.)
    assert!(
        (text.len()..=bound).contains(&reads),
        "read {reads} bytes of {}",
        text.len()
    );
    let found = found.unwrap();
    found.into_iter().map(|r| text[r].to_string()).collect()
}

#[test]
fn overlapping_lookaheads_are_read_in_linear_time() {
    let spaces = " ".repeat(1 << 20);
    let texts = [
        format!(
            "const el = {}*/{spaces}extends=\"/\" />; {IMPORT}",
            "<a /* ".repeat(100_000)
        ),
        format!(
            "const el = {}\nextends=\"/\" />; {IMPORT}",
            "<a // ".repeat(100_000)
        ),
    ];
    for text in texts {
        assert_eq!(read_in_linear_time(&text), ["./a"]);
    }
}
