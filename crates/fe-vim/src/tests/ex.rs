use serde_json::json;

use super::{commit, registers};
use crate::{Address, Base, Ex, ExCommand, ExError, HeadlessVim, LineSearch, Step, VimState};

/// What `line` parses to.
fn parsed(line: &str) -> Ex {
    Ex::parse(line).unwrap_or_else(|error| panic!("{line}: {error}"))
}

fn line(base: Base, offset: i64) -> Address {
    Address {
        base,
        offset,
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
                searches: vec![LineSearch::Pattern {
                    pattern: "x".to_string(),
                    forward: true,
                }],
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
