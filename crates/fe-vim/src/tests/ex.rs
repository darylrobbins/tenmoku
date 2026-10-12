use serde_json::json;

use super::{Measuring, commit, registers};
use crate::{Address, Base, Ex, ExCommand, ExError, HeadlessVim, LineSearch, Part, Step, VimState};
use fe_buffer::Origin;

/// What `line` parses to.
fn parsed(line: &str) -> Ex {
    Ex::parse(line).unwrap_or_else(|error| panic!("{line}: {error}"))
}

fn line(base: Base, offset: i64) -> Address {
    Address {
        base,
        parts: match offset {
            0 => vec![],
            offset => vec![Part::Offset(offset)],
        },
        ..Address::default()
    }
}

#[test]
fn entering_parsing_and_running() {
    // `:` opens an ex line; a count before it, or a visual mode, gives the
    // line a range to start from.
    let invocation = commit(":");
    assert_eq!(invocation.command.as_str(), "vim.command_line");
    assert_eq!(invocation.args, json!({"kind": "ex"}));
    assert_eq!(commit("3:").args, json!({"kind": "ex", "count": 3}));
    assert_eq!(run_cancels("d:"), Step::Cancel);

    let mut vim = HeadlessVim::new("a\nb\nc\nd\ne", (1, 0));
    vim.feed("3:").unwrap();
    assert_eq!(vim.mode(), "cmdline");
    assert_eq!(vim.state().command_line_prefill(), ".,.+2");
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.state().command_line_prefill(), "");
    vim.feed("vj:").unwrap();
    assert_eq!(vim.state().command_line_prefill(), "'<,'>");
    assert_eq!((vim.mark('<'), vim.mark('>')), (Some((1, 0)), Some((2, 0))));
    // The line runs in normal mode, the caret on `'<`; cancelling it stays
    // there.
    vim.feed("<Esc>").unwrap();
    assert_eq!((vim.mode(), vim.cursor()), ("normal", (1, 0)));

    // Parsing: the range's specifiers, then the command.
    assert_eq!(
        parsed("5"),
        Ex {
            range: vec![line(Base::Line(5), 0)],
            command: ExCommand::Goto,
        }
    );
    assert_eq!(parsed("$").range, vec![line(Base::Last, 0)]);
    assert_eq!(
        parsed(".,+2d"),
        Ex {
            range: vec![line(Base::Current, 0), line(Base::Current, 2)],
            command: ExCommand::Delete {
                register: None,
                count: None,
            },
        }
    );
    assert_eq!(
        parsed("3;/x/d").range,
        vec![
            Address {
                semicolon: true,
                ..line(Base::Line(3), 0)
            },
            Address {
                parts: vec![Part::Search(LineSearch::Pattern {
                    pattern: "x".to_string(),
                    forward: true,
                })],
                ..Address::default()
            },
        ]
    );
    assert_eq!(
        parsed("'<,'>d").range,
        vec![line(Base::Mark('<'), 0), line(Base::Mark('>'), 0)]
    );
    assert_eq!(
        parsed("d a 3"),
        Ex {
            range: vec![],
            command: ExCommand::Delete {
                register: Some('a'),
                count: Some(3),
            },
        }
    );
    assert_eq!(
        parsed("noh"),
        Ex {
            range: vec![],
            command: ExCommand::NoHighlight,
        }
    );
    assert_eq!(parsed("nohlsearch"), parsed("noh"));
    for (line, error) in [
        ("frobnicate", ExError::NotACommand("frobnicate".to_string())),
        ("no", ExError::NotACommand("no".to_string())),
        ("d!", ExError::NoBang),
        ("d 3x", ExError::Trailing("x".to_string())),
        ("d 0", ExError::ZeroCount),
        ("2noh", ExError::NoRange),
    ] {
        assert_eq!(Ex::parse(line), Err(error), "{line}");
    }

    // `:.,+2d` is one transaction: one `u` undoes it, and `.` repeats the
    // change before it, not it.
    let mut vim = HeadlessVim::new("abc\nb\nc\nd\ne", (0, 0));
    vim.feed("x:2<CR>:.,+2d<CR>").unwrap();
    assert_eq!((vim.text().as_str(), vim.cursor()), ("bc\ne", (1, 0)));
    assert_eq!(
        registers(&vim),
        vec![
            ('"', "b\nc\nd\n".to_string(), true),
            ('1', "b\nc\nd\n".to_string(), true),
            ('-', "a".to_string(), false),
        ]
    );
    vim.feed("u").unwrap();
    assert_eq!(
        (vim.text().as_str(), vim.cursor()),
        ("bc\nb\nc\nd\ne", (1, 0))
    );
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), "bc\n\nc\nd\ne");

    // `:noh` clears the highlight until the next search.
    let mut vim = HeadlessVim::new("a\nb\nb", (0, 0));
    vim.feed("/b<CR>").unwrap();
    assert!(vim.state().highlight().is_some());
    vim.feed(":noh<CR>").unwrap();
    assert!(vim.state().highlight().is_none());
    assert_eq!((vim.text().as_str(), vim.cursor()), ("a\nb\nb", (1, 0)));
    vim.feed("n").unwrap();
    assert!(vim.state().highlight().is_some());

    // An unknown command, or a line past the end, changes nothing and says
    // why.
    for (keys, message) in [
        (":frobnicate<CR>", "E492: Not an editor command: frobnicate"),
        (":2,9d<CR>", "E16: Invalid range"),
        (":9d<CR>", "E16: Invalid range"),
        (":3,2d<CR>", "E493: Backwards range given"),
        (":+2147483645<CR>", "E1247: Line number out of range"),
        // The range is read before the command is looked up.
        (":/zz/frob<CR>", "E486: Pattern not found: zz"),
        (":'<frob<CR>", "E20: Mark not set"),
        // A line before the first fails the command, not the range.
        (":-5<CR>", "E16: Invalid range"),
        (":-5d<CR>", "E16: Invalid range"),
        (":-5frob<CR>", "E492: Not an editor command: -5frob"),
        (":-5noh<CR>", "E481: No range allowed"),
        // The range is checked before the arguments are read.
        (":-5d xxx<CR>", "E16: Invalid range"),
        (":-5d 0<CR>", "E16: Invalid range"),
        (":-5s/a/b/z<CR>", "E16: Invalid range"),
        // A bang is refused before the range is checked.
        (":-5d!<CR>", "E477: No ! allowed"),
        (":-2&!<CR>", "E477: No ! allowed"),
        (":-5&&<CR>", "E16: Invalid range"),
        // Each offset is checked as it's applied.
        (
            ":1+2147483647-2147483647d<CR>",
            "E1247: Line number out of range",
        ),
    ] {
        let mut vim = HeadlessVim::new("a\nb\nc", (1, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.state().message(), Some(message), "{keys}");
        assert_eq!(
            (vim.text().as_str(), vim.cursor(), vim.mode()),
            ("a\nb\nc", (1, 0), "normal"),
            "{keys}"
        );
        assert!(registers(&vim).is_empty(), "{keys}");
    }
    // A goto goes to its last line, whatever its first is; zero offsets
    // are no-ops, even at the largest line number.
    for (keys, cursor) in [
        (":-5,3<CR>", (2, 0)),
        (":-5;3<CR>", (2, 0)),
        (":2147483647+0;2<CR>", (1, 0)),
        (":2147483647-0;2<CR>", (1, 0)),
    ] {
        let mut vim = HeadlessVim::new("a\nb\nc\nd", (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.cursor(), cursor, "{keys}");
        assert_eq!(vim.state().message(), None, "{keys}");
    }
    // A later line replaces a negative one, a comment ends the command,
    // and searches and offsets chain in order.
    let mut vim = HeadlessVim::new("a\nb\nc\nd", (0, 0));
    vim.feed(":-2,1,2d<CR>").unwrap();
    assert_eq!(vim.text(), "c\nd");
    let mut vim = HeadlessVim::new("a\nb\nc\nd", (0, 0));
    vim.feed(":d \" 3<CR>").unwrap();
    assert_eq!(vim.text(), "b\nc\nd");
    let mut vim = HeadlessVim::new("a\nx\nb\nc\nx\nd", (0, 0));
    vim.feed(":/x/+1/x/d<CR>").unwrap();
    assert_eq!(vim.text(), "a\nx\nb\nc\nd");
    assert_eq!(
        parsed("1 /x/+1d").range,
        vec![Address {
            base: Base::Line(1),
            parts: vec![
                Part::Search(LineSearch::Pattern {
                    pattern: "x".to_string(),
                    forward: true,
                }),
                Part::Offset(1),
            ],
            ..Address::default()
        }]
    );

    // Deleting from an empty buffer leaves the registers alone.
    let mut vim = HeadlessVim::new("a\nb", (0, 0));
    vim.feed(":%d<CR>:d<CR>").unwrap();
    assert_eq!(
        registers(&vim),
        vec![
            ('"', "a\nb\n".to_string(), true),
            ('1', "a\nb\n".to_string(), true),
        ]
    );

    // The caret lands on the line that took the deleted one's place even
    // when another origin inserts a row above while text is measured.
    let mut vim = HeadlessVim::new("a\nb\n  c", (0, 0));
    let mut split = vim.model().split();
    let mut edited = false;
    let _measuring = Measuring::new(move |after_edit| {
        if after_edit && !edited {
            edited = true;
            let ai = Origin::Ai {
                agent: "agent".into(),
                session: "session".into(),
            };
            split.buffer_mut().edit([(0..0, "X\n")], ai);
        }
    });
    vim.feed(":2d<CR>").unwrap();
    assert_eq!((vim.text().as_str(), vim.cursor()), ("X\na\n  c", (2, 2)));
    drop(_measuring);

    // Alone, a line past the end goes to the last.
    let mut vim = HeadlessVim::new("a\nb\nc", (0, 0));
    vim.feed(":9<CR>").unwrap();
    assert_eq!((vim.cursor(), vim.state().message()), ((2, 0), None));
}

/// What `keys` do through a [`VimState`], from nothing pending.
fn run_cancels(keys: &str) -> Step {
    let mut state = VimState::new();
    let mut last = Step::Forward;
    for key in crate::parse_keys(keys).unwrap() {
        last = state.feed(key);
    }
    last
}

#[test]
fn ex_line_commands_land_past_rows_put_in_while_measuring() {
    // Measuring text as the model catches up after an ex line's edit may
    // edit the buffer through another model: the caret lands where the
    // command put it all the same, here a row lower for the `X` row
    // another origin puts at the start meanwhile.
    let run = |keys: &str, measuring: bool| {
        let mut vim = HeadlessVim::new("a\n  bb\n  cc\ndd\nee", (4, 1));
        let mut split = vim.model().split();
        let mut edited = !measuring;
        let _measuring = Measuring::new(move |after_edit| {
            if after_edit && !edited {
                edited = true;
                let ai = Origin::Ai {
                    agent: "agent".into(),
                    session: "session".into(),
                };
                split.buffer_mut().edit([(0..0, "X\n")], ai);
            }
        });
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    for keys in [
        ":2,3j<CR>",
        ":2,3j!<CR>",
        ":2j 3<CR>",
        ":4t1<CR>",
        ":4m1<CR>",
        ":2,3><CR>",
        ":2,4sort<CR>",
    ] {
        let (plain, at) = run(keys, false);
        let (edited, landed) = run(keys, true);
        assert_eq!(edited, format!("X\n{plain}"), "{keys}");
        assert_eq!(landed, (at.0 + 1, at.1), "{keys}");
    }
}

#[test]
fn sort_ignores_arguments_on_zero_or_one_line_and_fails_past_the_end() {
    // A range of at most one line (or line 0) sorts nothing, so its
    // arguments are never read; past the last line it's E16.
    for keys in [
        ":1sort z<CR>",
        ":2sort //<CR>",
        ":0sort z<CR>",
        ":0sort nn<CR>",
        ":0sort /a<CR>",
    ] {
        let mut vim = HeadlessVim::new("b\na", (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.state().message(), None, "{keys}");
        assert_eq!(vim.text(), "b\na", "{keys}");
    }
    for keys in [":3sort z<CR>", ":3sort nn<CR>", ":3sort /a<CR>"] {
        let mut vim = HeadlessVim::new("b\na", (0, 0));
        vim.feed(keys).unwrap();
        assert_eq!(vim.state().message(), Some("E16: Invalid range"), "{keys}");
        assert_eq!(vim.text(), "b\na", "{keys}");
    }
}
