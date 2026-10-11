//! What `Ex::parse` makes of the ex line commands (`:y`, `:j`, `:m`, `:t`
//! and `:co`, `:>` and `:<`, `:sort`), their abbreviations and arguments,
//! and the errors nvim gives for bad ones.

use fe_vim::{Address, Base, Ex, ExCommand, ExError, LineSearch, Part};

fn command(line: &str) -> ExCommand {
    Ex::parse(line)
        .unwrap_or_else(|error| panic!("{line}: {error}"))
        .command
}

fn error(line: &str) -> ExError {
    Ex::parse(line).expect_err(line)
}

fn at(base: Base, parts: Vec<Part>) -> Address {
    Address {
        base,
        parts,
        ..Address::default()
    }
}

fn line(n: i32) -> Address {
    at(Base::Line(n), vec![])
}

#[test]
fn ex_parses_yank() {
    let yank = |register, count| ExCommand::Yank { register, count };
    for name in ["y", "ya", "yan", "yank"] {
        assert_eq!(command(name), yank(None, None), "{name}");
    }
    assert_eq!(command("y a"), yank(Some('a'), None));
    assert_eq!(command("ya A"), yank(Some('A'), None));
    assert_eq!(command("y b 3"), yank(Some('b'), Some(3)));
    assert_eq!(command("y3"), yank(None, Some(3)));
    assert_eq!(command("yank 9"), yank(None, Some(9)));
    // `-` and `_` are registers; `#` is `"`; a `"` starts a comment.
    assert_eq!(command("y-"), yank(Some('-'), None));
    assert_eq!(command("y _"), yank(Some('_'), None));
    assert_eq!(command("y #"), yank(Some('"'), None));
    assert_eq!(command("y \"a"), yank(None, None));
    let ex = Ex::parse("2,4y z").unwrap();
    assert_eq!(ex.range, vec![line(2), line(4)]);

    assert_eq!(error("y 0"), ExError::ZeroCount);
    assert_eq!(error("y a0"), ExError::ZeroCount);
    assert_eq!(error("y!"), ExError::NoBang);
    assert_eq!(error("y %"), ExError::Trailing("%".into()));
    assert_eq!(error("y a b"), ExError::Trailing("b".into()));
    assert_eq!(error("y3a"), ExError::Trailing("a".into()));
    assert_eq!(error("y 2 l"), ExError::Trailing("l".into()));
    assert_eq!(error("yb"), ExError::NotACommand("yb".into()));
    assert_eq!(error("y 0").to_string(), "E939: Positive count required");
}

#[test]
fn ex_parses_join() {
    let join = |raw, count| ExCommand::Join { raw, count };
    for name in ["j", "jo", "joi", "join"] {
        assert_eq!(command(name), join(false, None), "{name}");
        assert_eq!(command(&format!("{name}!")), join(true, None), "{name}!");
    }
    assert_eq!(command("j 3"), join(false, Some(3)));
    assert_eq!(command("j!3"), join(true, Some(3)));
    assert_eq!(command("join! 2"), join(true, Some(2)));
    // The print flags, after the count, are taken and ignored.
    assert_eq!(command("j l"), join(false, None));
    assert_eq!(command("j 2 lp#"), join(false, Some(2)));
    assert_eq!(command("j l p"), join(false, None));
    assert_eq!(command("j 2 l # p"), join(false, Some(2)));
    assert_eq!(command("%j"), join(false, None));

    assert_eq!(error("j0"), ExError::ZeroCount);
    assert_eq!(error("j a"), ExError::Trailing("a".into()));
    assert_eq!(error("j 3 x"), ExError::Trailing("x".into()));
    assert_eq!(error("j l2"), ExError::Trailing("2".into()));
    assert_eq!(error("j 2 l p x"), ExError::Trailing("x".into()));
    assert_eq!(error("j lx"), ExError::Trailing("x".into()));
    assert_eq!(error("j!!"), ExError::Trailing("!".into()));
    assert_eq!(error("ja"), ExError::NotACommand("ja".into()));
}

#[test]
fn ex_parses_move_and_copy() {
    for name in ["m", "mo", "mov", "move"] {
        assert_eq!(
            command(&format!("{name}0")),
            ExCommand::Move { to: line(0) }
        );
    }
    for name in ["t", "co", "cop", "copy"] {
        assert_eq!(
            command(&format!("{name} $")),
            ExCommand::Copy {
                to: at(Base::Last, vec![])
            }
        );
    }
    assert_eq!(
        command("t."),
        ExCommand::Copy {
            to: at(Base::Current, vec![])
        }
    );
    assert_eq!(
        command("m 'a"),
        ExCommand::Move {
            to: at(Base::Mark('a'), vec![])
        }
    );
    assert_eq!(
        command("m+"),
        ExCommand::Move {
            to: at(Base::Current, vec![Part::Offset(1)])
        }
    );
    assert_eq!(
        command("t/x/-1"),
        ExCommand::Copy {
            to: at(
                Base::Current,
                vec![
                    Part::Search(LineSearch::Pattern {
                        pattern: "x".into(),
                        forward: true,
                    }),
                    Part::Offset(-1),
                ],
            )
        }
    );
    // Blanks and numbers after the address add to it; what follows it is
    // ignored, as in nvim.
    assert_eq!(
        command("m 3 4"),
        ExCommand::Move {
            to: at(Base::Line(3), vec![Part::Offset(4)])
        }
    );
    assert_eq!(
        command("move $ x"),
        ExCommand::Move {
            to: at(Base::Last, vec![])
        }
    );
    assert_eq!(command("m3,4"), ExCommand::Move { to: line(3) });
    let ex = Ex::parse("'<,'>t0").unwrap();
    assert_eq!(
        ex.range,
        vec![at(Base::Mark('<'), vec![]), at(Base::Mark('>'), vec![])]
    );

    // No address (or `%`) is nvim's E16.
    for line in ["m", "t", "co", "m %", "t ,3", "m \"", "t x"] {
        assert_eq!(error(line), ExError::InvalidRange, "{line}");
    }
    assert_eq!(error("m! 3"), ExError::NoBang);
    assert_eq!(error("t!0"), ExError::NoBang);
    assert_eq!(error("c 0"), ExError::NotACommand("c 0".into()));
    assert_eq!(error("ta 0"), ExError::NotACommand("ta 0".into()));
    assert_eq!(
        ExError::IntoItself.to_string(),
        "E134: Cannot move a range of lines into itself"
    );
}

#[test]
fn ex_parses_shift() {
    let shift = |left, times, count| ExCommand::Shift { left, times, count };
    assert_eq!(command(">"), shift(false, 1, None));
    assert_eq!(command("<"), shift(true, 1, None));
    assert_eq!(command(">>>"), shift(false, 3, None));
    assert_eq!(command("> > "), shift(false, 2, None));
    assert_eq!(command("<< 3"), shift(true, 2, Some(3)));
    assert_eq!(command(">3"), shift(false, 1, Some(3)));
    assert_eq!(command("> 2 #"), shift(false, 1, Some(2)));
    assert_eq!(command(">l"), shift(false, 1, None));
    assert_eq!(command("> # p"), shift(false, 1, None));
    assert_eq!(command("< l #"), shift(true, 1, None));
    assert_eq!(command("<< 2 p l"), shift(true, 2, Some(2)));
    let ex = Ex::parse("2,4>> 2").unwrap();
    assert_eq!(ex.range, vec![line(2), line(4)]);

    assert_eq!(error("> 0"), ExError::ZeroCount);
    assert_eq!(error(">!"), ExError::NoBang);
    assert_eq!(error("> x"), ExError::Trailing("x".into()));
    assert_eq!(error("><"), ExError::Trailing("<".into()));
    assert_eq!(error("> 2x"), ExError::Trailing("x".into()));
}

#[test]
fn ex_parses_sort() {
    let sort =
        |reverse, ignore_case, numeric, unique, by_match, pattern: Option<&str>| ExCommand::Sort {
            reverse,
            ignore_case,
            numeric,
            unique,
            by_match,
            pattern: pattern.map(str::to_string),
        };
    let plain = sort(false, false, false, false, false, None);
    assert_eq!(command("sor"), plain);
    assert_eq!(command("sort"), plain);
    assert_eq!(
        command("sort!"),
        sort(true, false, false, false, false, None)
    );
    assert_eq!(
        command("sort i"),
        sort(false, true, false, false, false, None)
    );
    assert_eq!(
        command("sort n"),
        sort(false, false, true, false, false, None)
    );
    assert_eq!(
        command("sort u"),
        sort(false, false, false, true, false, None)
    );
    assert_eq!(
        command("sort! iu"),
        sort(true, true, false, true, false, None)
    );
    assert_eq!(
        command("sort /b/"),
        sort(false, false, false, false, false, Some("b"))
    );
    assert_eq!(
        command(r"sort /\d/ r"),
        sort(false, false, false, false, true, Some(r"\d"))
    );
    assert_eq!(
        command("sort n/1/"),
        sort(false, false, true, false, false, Some("1"))
    );
    assert_eq!(
        command("sort //"),
        sort(false, false, false, false, false, Some(""))
    );
    // Any other character delimits the pattern; a `"` starts a comment.
    assert_eq!(
        command("sort ,a,"),
        sort(false, false, false, false, false, Some("a"))
    );
    assert_eq!(command("sort \"x"), plain);
    let ex = Ex::parse("2,5sort").unwrap();
    assert_eq!(ex.range, vec![line(2), line(5)]);

    assert_eq!(error("so"), ExError::NotACommand("so".into()));
    assert_eq!(error("sort z"), ExError::InvalidArgument("z".into()));
    assert_eq!(error("sort iz q"), ExError::InvalidArgument("z q".into()));
    assert_eq!(
        error("sort iz q").to_string(),
        "E475: Invalid argument: z q"
    );
    assert_eq!(error("sort /a"), ExError::MissingDelimiter("a".into()));
    assert_eq!(error("sort /[/"), ExError::MissingDelimiter("[/".into()));
    assert_eq!(error("sort!!"), ExError::MissingDelimiter("".into()));
    assert_eq!(error("sort 3"), ExError::MissingDelimiter("".into()));
    assert_eq!(
        error("sort /a").to_string(),
        "E654: Missing delimiter after search pattern: a"
    );
    // One number kind and one pattern, as in nvim.
    assert_eq!(error("sort nn"), ExError::TwoNumberKinds);
    assert_eq!(error("sort n n"), ExError::TwoNumberKinds);
    assert_eq!(error("sort nn").to_string(), "E474: Invalid argument");
    assert_eq!(
        error("sort /a/ /b/"),
        ExError::InvalidArgument("/b/".into())
    );
    assert_eq!(error("sort /a//b/"), ExError::InvalidArgument("/b/".into()));
    assert_eq!(
        error("sort /a/ n /b/ i"),
        ExError::InvalidArgument("/b/ i".into())
    );
    assert_eq!(error("sort /a/ //"), ExError::InvalidArgument("//".into()));
    // Other flags may repeat.
    assert_eq!(
        command("sort ii uu rr"),
        sort(false, true, false, true, true, None)
    );
    for flag in ["x", "o", "b", "f", "l"] {
        assert_eq!(
            error(&format!("sort {flag}")),
            ExError::Unsupported(format!(":sort {flag}"))
        );
    }
}
