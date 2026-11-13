//! Failed assertions read from test output. `assert_current.txt` is the
//! `fails_eq` block of the recorded `libtest_mixed.txt` (Rust 1.98);
//! `assert_legacy.txt` is the same failure as Rust before 1.73 wrote it
//! (`core::panicking::assert_failed_inner` and the `panicked at '…', loc`
//! panic line of those versions), written by hand: no older toolchain is
//! installed to record it.

use fe_runlog::{AssertOp, Assertion};

#[test]
fn current_and_legacy_assert_eq_forms() {
    let expected = Some(Assertion {
        op: AssertOp::Eq,
        left: "[1, 2]".to_owned(),
        right: "[1, 3]".to_owned(),
        message: Some("lists differ for x".to_owned()),
    });
    assert_eq!(
        Assertion::parse(include_str!("fixtures/assert_current.txt")),
        expected
    );
    assert_eq!(
        Assertion::parse(include_str!("fixtures/assert_legacy.txt")),
        expected
    );

    // assert_ne!, without a message, in both forms.
    let ne = Some(Assertion {
        op: AssertOp::Ne,
        left: "2".to_owned(),
        right: "2".to_owned(),
        message: None,
    });
    assert_eq!(
        Assertion::parse(
            "\nthread 'fails_ne' (1316144) panicked at crates/fe-runlog/tests/sample.rs:12:5:\n\
             assertion `left != right` failed\n  left: 2\n right: 2\n"
        ),
        ne
    );
    assert_eq!(
        Assertion::parse(
            "thread 'fails_ne' panicked at 'assertion failed: `(left != right)`\n  \
             left: `2`,\n right: `2`', src/lib.rs:12:5\n\
             note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n"
        ),
        ne
    );
    assert_eq!(AssertOp::Ne.symbol(), "!=");
}

#[test]
fn multiline_values_are_kept() {
    // Recorded: a value whose Debug writes two lines.
    let current = Assertion::parse(
        "\nthread 'fails_multiline' (1316456) panicked at crates/fe-runlog/tests/sample.rs:26:5:\n\
         assertion `left == right` failed\n  left: first line\nsecond line\n right: first line\nsecond line\n\
         note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n",
    )
    .expect("an assertion");
    assert_eq!(current.left, "first line\nsecond line");
    assert_eq!(current.right, "first line\nsecond line");
    assert_eq!(current.message, None);

    // A message over two lines, and a pretty-printed value, at the end of
    // the output.
    let message = Assertion::parse(
        "thread 't' panicked at src/lib.rs:1:1:\n\
         assertion `left == right` failed: first\nsecond\n  left: Point {\n    x: 1,\n}\n right: Point {\n    x: 2,\n}",
    )
    .expect("an assertion");
    assert_eq!(message.message.as_deref(), Some("first\nsecond"));
    assert_eq!(message.left, "Point {\n    x: 1,\n}");
    assert_eq!(message.right, "Point {\n    x: 2,\n}");

    // The legacy form, with and without a message.
    let legacy = Assertion::parse(
        "thread 't' panicked at 'assertion failed: `(left == right)`\n  \
         left: `first line\nsecond line`,\n right: `third line\nfourth line`: why\nnot', src/lib.rs:3:9\n",
    )
    .expect("an assertion");
    assert_eq!(legacy.left, "first line\nsecond line");
    assert_eq!(legacy.right, "third line\nfourth line");
    assert_eq!(legacy.message.as_deref(), Some("why\nnot"));

    let legacy = Assertion::parse(
        "thread 't' panicked at 'assertion failed: `(left == right)`\n  \
         left: `a\nb`,\n right: `c\nd`', src/lib.rs:3:9\n",
    )
    .expect("an assertion");
    assert_eq!(
        (legacy.left.as_str(), legacy.right.as_str()),
        ("a\nb", "c\nd")
    );
    assert_eq!(legacy.message, None);
}

#[test]
fn a_non_assertion_panic_is_none() {
    // Recorded.
    assert_eq!(
        Assertion::parse(
            "\nthread 'panics_plainly' (1316146) panicked at crates/fe-runlog/tests/sample.rs:31:5:\n\
             not an assertion\n"
        ),
        None
    );
    // `assert!`, in both forms, and a test that returned an error.
    assert_eq!(
        Assertion::parse("thread 't' panicked at src/lib.rs:1:1:\nassertion failed: x > 1\n"),
        None
    );
    assert_eq!(
        Assertion::parse("thread 't' panicked at 'assertion failed: x > 1', src/lib.rs:1:1\n"),
        None
    );
    assert_eq!(Assertion::parse("Error: \"no such file\"\n"), None);
    assert_eq!(Assertion::parse(""), None);
}
