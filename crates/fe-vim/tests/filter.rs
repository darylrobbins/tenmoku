//! `!{motion}`, `!!` and visual `!` (the filter's command line), and
//! `:{range}!{cmd}` and `:r !{cmd}` (the host commands they leave), with
//! `VimState::apply_filter` and `VimState::apply_read_command` putting the
//! output in. The prefills, cursors and messages were probed with nvim
//! 0.12.4 (`nvim --headless --clean`, the filters run through `sort`,
//! `tr`, `head`, `printf` and `true`), noted case by case.

use fe_command::{Actor, Invocation};
use fe_vim::{
    HeadlessVim, HostCommand, Remap, RemapCommand, RemapMode, RemapSet, SHELL_REFUSED,
    ShellCommand, ShellPart,
};

/// Eight rows: two paragraphs.
const PARAGRAPHS: &str = "a1\na2\na3\n\nb1\nb2\nb3\nb4";

/// Five rows to filter.
const ROWS: &str = "  c1\nb2\na3\nx4\ny5";

/// Feeds `keys` to a vim on `text` at `cursor`.
fn after(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    vim
}

/// The command line `keys` open, and the caret then.
#[track_caller]
fn prefill(text: &str, cursor: (u32, usize), keys: &str) -> (String, (u32, usize)) {
    let vim = after(text, cursor, keys);
    assert_eq!(vim.mode(), "cmdline", "{keys:?}");
    (vim.state().command_line_prefill().to_string(), vim.cursor())
}

/// The host command and message an ex `line` leaves, typed on `text` at
/// `cursor`.
fn submit(vim: &mut HeadlessVim, line: &str) -> (Option<HostCommand>, Option<String>) {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

/// The same, on a fresh vim.
fn line(text: &str, cursor: (u32, usize), line: &str) -> (Option<HostCommand>, Option<String>) {
    submit(&mut HeadlessVim::new(text, cursor), line)
}

/// The same, submitted by `actor` rather than typed.
fn submit_as(
    vim: &mut HeadlessVim,
    line: &str,
    actor: Actor,
) -> (Option<HostCommand>, Option<String>) {
    vim.feed(":").unwrap();
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": line }))
        .actor(actor)
        .context(fe_vim::context(vim.state()));
    vim.dispatch(&submit).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn text(text: &str) -> ShellPart {
    ShellPart::Text(text.to_string())
}

fn command(parts: Vec<ShellPart>) -> ShellCommand {
    ShellCommand { parts }
}

fn filter(rows: std::ops::Range<u32>, parts: Vec<ShellPart>) -> Option<HostCommand> {
    Some(HostCommand::Filter {
        rows,
        command: command(parts),
        actor: Actor::User,
    })
}

fn read(below: u32, parts: Vec<ShellPart>) -> Option<HostCommand> {
    Some(HostCommand::ReadCommand {
        below,
        command: command(parts),
        actor: Actor::User,
    })
}

/// Runs `apply` on `vim`'s state and model, as the host does once the
/// command has run.
fn host_applies(
    vim: &mut HeadlessVim,
    apply: impl FnOnce(&mut fe_vim::VimState, &mut fe_editor::EditorModel),
) {
    let mut state = std::mem::take(vim.state_mut());
    apply(&mut state, vim.model_mut());
    *vim.state_mut() = state;
}

#[test]
fn filter_keys_prefill_the_range() {
    let line = |prefill: &str| prefill.to_string();
    // nvim: `!ip` on a1 gives `:.,.+2!`, the caret staying at {1, 0}; from
    // a2's column 1 too, the caret going to the paragraph's start, {1, 0}.
    assert_eq!(prefill(PARAGRAPHS, (0, 0), "!ip"), (line(".,.+2!"), (0, 0)));
    assert_eq!(prefill(PARAGRAPHS, (1, 1), "!ip"), (line(".,.+2!"), (0, 0)));
    // On b2 the paragraph runs to the last row: `:.,$!`, caret {5, 0}.
    assert_eq!(prefill(PARAGRAPHS, (5, 0), "!ip"), (line(".,$!"), (4, 0)));
    // `!j` `:.,.+1!`, the caret staying (from "  abc" column 3: {1, 3});
    // `!k` from a2 the same, the caret going up to {1, 0}.
    assert_eq!(prefill(PARAGRAPHS, (0, 0), "!j"), (line(".,.+1!"), (0, 0)));
    assert_eq!(
        prefill("  abc\n  def\nghi", (0, 3), "!j"),
        (line(".,.+1!"), (0, 3))
    );
    assert_eq!(prefill(PARAGRAPHS, (1, 0), "!k"), (line(".,.+1!"), (0, 0)));
    // `3!!` `:.,.+2!`, `!!` `:.!`, `5!!` on the last row nothing (it fails).
    assert_eq!(prefill(PARAGRAPHS, (0, 0), "3!!"), (line(".,.+2!"), (0, 0)));
    assert_eq!(prefill(PARAGRAPHS, (0, 0), "!!"), (line(".!"), (0, 0)));
    assert_eq!(prefill(PARAGRAPHS, (7, 0), "!!"), (line(".!"), (7, 0)));
    let vim = after(PARAGRAPHS, (7, 0), "5!!");
    assert_eq!(vim.mode(), "normal");
    // `!G` from a2 `:.,$!` (caret {2, 0}), `!gg` from b1 `:.,.+4!` (caret
    // {1, 0}), `!3j` `:.,.+3!`, `!w` `:.!`.
    assert_eq!(prefill(PARAGRAPHS, (1, 0), "!G"), (line(".,$!"), (1, 0)));
    assert_eq!(prefill(PARAGRAPHS, (4, 0), "!gg"), (line(".,.+4!"), (0, 0)));
    assert_eq!(prefill(PARAGRAPHS, (1, 0), "!3j"), (line(".,.+3!"), (1, 0)));
    assert_eq!(prefill(PARAGRAPHS, (0, 0), "!w"), (line(".!"), (0, 0)));
    // From the first row's column 4 of "  abc def": `!!` and `!iw` go to
    // {1, 2} (where `_` lands, the word's start), `3!!` and `!j` stay at
    // {1, 4}, `!ip` goes to {1, 0}.
    let words = "  abc def\n  ghi\njkl\n\nmno";
    assert_eq!(prefill(words, (0, 4), "!!"), (line(".!"), (0, 2)));
    assert_eq!(prefill(words, (0, 4), "!iw"), (line(".!"), (0, 2)));
    assert_eq!(prefill(words, (0, 4), "3!!"), (line(".,.+2!"), (0, 4)));
    assert_eq!(prefill(words, (0, 4), "!ip"), (line(".,.+2!"), (0, 0)));
    assert_eq!(prefill(words, (0, 4), "!j"), (line(".,.+1!"), (0, 4)));
    // `!G` is a jump: `''` goes back to where it began.
    let vim = after("  abc\n  def\nghi\njkl", (1, 3), "!G");
    assert_eq!(vim.mark('\''), Some((1, 3)));
    // Visual `!`: `:'<,'>!`, the caret on `'<` ({2, 0}), charwise too.
    assert_eq!(prefill(PARAGRAPHS, (1, 0), "Vj!"), (line("'<,'>!"), (1, 0)));
    assert_eq!(prefill(PARAGRAPHS, (1, 0), "vj!"), (line("'<,'>!"), (1, 0)));
    // The line carries nothing else: submitted as it is, it filters the
    // rows (by the last command), and cancelled it changes nothing.
    let mut vim = after(PARAGRAPHS, (0, 0), "!ip<Esc>");
    assert_eq!((vim.mode(), vim.text()), ("normal", PARAGRAPHS.to_string()));
    assert_eq!(vim.state_mut().take_host_command(), None);
}

#[test]
fn filter_line_becomes_a_host_command() {
    // nvim filters `:.,+2` (rows 1 to 3) and `:%` (all).
    assert_eq!(
        line(ROWS, (0, 0), ".,+2!sort").0,
        filter(0..3, vec![text("sort")])
    );
    assert_eq!(
        line(ROWS, (0, 0), "%!tr a b").0,
        filter(0..5, vec![text("tr a b")])
    );
    // The command is the rest of the line, `|` and `"` included.
    assert_eq!(
        line(ROWS, (1, 0), ".! sort | uniq \"x\"").0,
        filter(1..2, vec![text("sort | uniq \"x\"")])
    );
    // The keys' line, submitted with a command typed.
    let mut vim = after(PARAGRAPHS, (1, 1), "!ipsort -r<CR>");
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(0..3, vec![text("sort -r")])
    );
    // A range past the last line is E16, backwards E493.
    assert_eq!(
        line(ROWS, (0, 0), "4,9!sort"),
        (None, Some("E16: Invalid range".into()))
    );
    assert_eq!(
        line(ROWS, (0, 0), "3,2!sort"),
        (None, Some("E493: Backwards range given".into()))
    );

    // `:r !{cmd}` below the range's line (the cursor's), `:0r` above the
    // first; `:r!{cmd}` the same.
    assert_eq!(line(ROWS, (1, 0), "r !date").0, read(2, vec![text("date")]));
    assert_eq!(
        line(ROWS, (1, 0), "0r !date").0,
        read(0, vec![text("date")])
    );
    assert_eq!(
        line(ROWS, (1, 0), "$r !date +%s").0,
        read(5, vec![text("date +"), ShellPart::CurrentFile, text("s")])
    );
    assert_eq!(line(ROWS, (1, 0), "r!date").0, read(2, vec![text("date")]));
    assert_eq!(
        line(ROWS, (1, 0), "read !date").0,
        read(2, vec![text("date")])
    );
    // `:r` of a file is as it was.
    assert_eq!(
        line(ROWS, (1, 0), "r f.txt").0,
        Some(HostCommand::Read {
            path: Some("f.txt".into()),
            below: 2
        })
    );

    // With no command (or `!` alone, as nvim's `:.!!`), the last one; with
    // none yet, E34. nvim: `:1!!` "E34: No previous command".
    assert_eq!(
        line(ROWS, (0, 0), ".!"),
        (None, Some("E34: No previous command".into()))
    );
    assert_eq!(
        line(ROWS, (0, 0), ".!!"),
        (None, Some("E34: No previous command".into()))
    );
    assert_eq!(
        line(ROWS, (0, 0), "r !"),
        (None, Some("E34: No previous command".into()))
    );
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    assert_eq!(
        submit(&mut vim, "1,2!sort").0,
        filter(0..2, vec![text("sort")])
    );
    assert_eq!(submit(&mut vim, "2,3!").0, filter(1..3, vec![text("sort")]));
    assert_eq!(submit(&mut vim, "3!!").0, filter(2..3, vec![text("sort")]));
    assert_eq!(submit(&mut vim, "r !").0, read(1, vec![text("sort")]));
    // `:r !` sets it too.
    assert_eq!(submit(&mut vim, "r !date").0, read(1, vec![text("date")]));
    assert_eq!(submit(&mut vim, "%!").0, filter(0..5, vec![text("date")]));
    // A `!` in the command stays.
    assert_eq!(
        submit(&mut vim, ".!echo a!b").0,
        filter(0..1, vec![text("echo a!b")])
    );
    assert_eq!(
        submit(&mut vim, ".!echo \\!").0,
        filter(0..1, vec![text("echo !")])
    );

    // `:!ls`, with no range, runs nothing and says where to.
    let (command, message) = line(ROWS, (0, 0), "!ls");
    assert_eq!(command, None);
    assert!(message.unwrap().contains("terminal panel"));
    assert_eq!(
        vim.text(),
        ROWS,
        "nothing changes until the host applies the output"
    );
}

/// The text, caret and message a filter leaves, then the text and caret
/// `u` leaves.
type Applied = (String, (u32, usize), Option<String>, (String, (u32, usize)));

#[test]
fn apply_filter_replaces_rows_as_nvim_does() {
    /// `output` for `rows` of ROWS with the caret at `cursor`: the text,
    /// caret and message, then the text and caret `u` leaves.
    #[track_caller]
    fn applied(cursor: (u32, usize), rows: std::ops::Range<u32>, output: &str) -> Applied {
        let mut vim = HeadlessVim::new(ROWS, cursor);
        host_applies(&mut vim, |state, model| {
            state.apply_filter(model, rows, output)
        });
        let filtered = (
            vim.text(),
            vim.cursor(),
            vim.state().message().map(String::from),
        );
        vim.feed("u").unwrap();
        (
            filtered.0,
            filtered.1,
            filtered.2,
            (vim.text(), vim.cursor()),
        )
    }
    let filtered = || Some("3 lines filtered".to_string());
    let undone = |row| (ROWS.to_string(), (row, 0));
    // nvim `1,3!sort` from {5, 1}: sorted, caret {1, 0}, "3 lines
    // filtered"; `u` back with the caret at {3, 0}, the range's last row.
    assert_eq!(
        applied((4, 1), 0..3, "a3\nb2\n  c1\n"),
        ("a3\nb2\n  c1\nx4\ny5".into(), (0, 0), filtered(), undone(2))
    );
    // `2,4!tr a-z A-Z`: caret {2, 0}, undo {4, 0}.
    assert_eq!(
        applied((0, 0), 1..4, "B2\nA3\nX4\n"),
        ("  c1\nB2\nA3\nX4\ny5".into(), (1, 0), filtered(), undone(3))
    );
    // Fewer rows (`head -1`): those past the output's go.
    assert_eq!(
        applied((0, 0), 1..4, "b2\n"),
        ("  c1\nb2\ny5".into(), (1, 0), filtered(), undone(3))
    );
    // More (`printf "p\nq\nr\ns\nt\n"`).
    assert_eq!(
        applied((0, 0), 1..4, "p\nq\nr\ns\nt\n"),
        (
            "  c1\np\nq\nr\ns\nt\ny5".into(),
            (1, 0),
            filtered(),
            undone(3)
        )
    );
    // None (`true`): the caret on the row that took their place.
    assert_eq!(
        applied((0, 0), 1..4, ""),
        ("  c1\ny5".into(), (1, 0), filtered(), undone(3))
    );
    // No final line break (`printf "p\nq"`): its last line is a row.
    assert_eq!(
        applied((0, 0), 1..4, "p\nq"),
        ("  c1\np\nq\ny5".into(), (1, 0), filtered(), undone(3))
    );
    // The caret at the first row's first non-blank (`printf "  p\n"`:
    // {2, 2}).
    assert_eq!(
        applied((0, 0), 1..4, "  p\n"),
        ("  c1\n  p\ny5".into(), (1, 2), filtered(), undone(3))
    );
    // Two rows or fewer say nothing ('report'): `4,5!true` leaves {3, 0},
    // `2,2!sort` (no change, still an undo step) {2, 0}.
    assert_eq!(
        applied((0, 0), 3..5, ""),
        ("  c1\nb2\na3".into(), (2, 0), None, undone(4))
    );
    assert_eq!(
        applied((0, 0), 1..2, "b2\n"),
        (ROWS.into(), (1, 0), None, undone(1))
    );
    // Every row, none out: one empty row.
    assert_eq!(
        applied((0, 0), 0..5, ""),
        (
            "".into(),
            (0, 0),
            Some("5 lines filtered".into()),
            undone(4)
        )
    );

    // Marks on the rows kept stay, those on the rows dropped go, the rest
    // move with their rows: nvim `2,4!head -2` with a, b, c on rows 2, 4, 5
    // leaves a on 2, b unset, c on 4.
    let mut vim = after("a\nb\nc\nd\ne", (1, 0), "ma2jmbjmcgg");
    host_applies(&mut vim, |state, model| {
        state.apply_filter(model, 1..4, "x\ny\n")
    });
    assert_eq!(vim.text(), "a\nx\ny\ne");
    assert_eq!(
        (vim.mark('a'), vim.mark('b'), vim.mark('c')),
        (Some((1, 0)), None, Some((3, 0)))
    );

    // A mark on the last row kept stays too, at the text's end as well.
    let mut vim = after("a\nb\nc\nd\ne", (1, 0), "majmmjmbjmcgg");
    host_applies(&mut vim, |state, model| {
        state.apply_filter(model, 1..4, "x\ny\n")
    });
    assert_eq!(vim.text(), "a\nx\ny\ne");
    let marks = [vim.mark('a'), vim.mark('m'), vim.mark('b'), vim.mark('c')];
    assert_eq!(marks, [Some((1, 0)), Some((2, 0)), None, Some((3, 0))]);
    let mut vim = after("a\nb\nc\nd\ne", (3, 0), "mdjmegg");
    host_applies(&mut vim, |state, model| {
        state.apply_filter(model, 3..5, "x\n")
    });
    assert_eq!(vim.text(), "a\nb\nc\nx");
    assert_eq!((vim.mark('d'), vim.mark('e')), (Some((3, 0)), None));

    // `'[` and `'.` at the range's first row, `']` at the last row put in
    // (with none, the row above), whatever changed: nvim `2,4!tr c C`,
    // `2,4!cat` and `2,4!head -2` leave `'[` {2, 0} and `']` {4, 0} or
    // {3, 0}, `2,4!true` `']` {1, 0}, `'.` {2, 0} each.
    let changed = |output: &str| {
        let mut vim = HeadlessVim::new("aaaa\nbbbb\ncccc\ndddd\neeee", (0, 0));
        host_applies(&mut vim, |state, model| {
            state.apply_filter(model, 1..4, output)
        });
        [vim.mark('['), vim.mark(']'), vim.mark('.')]
    };
    let marks = |end| [Some((1, 0)), Some((end, 0)), Some((1, 0))];
    assert_eq!(changed("bbbb\nCCCC\ndddd\n"), marks(3));
    assert_eq!(changed("bbbb\ncccc\ndddd\n"), marks(3));
    assert_eq!(changed("bbbb\ncccc\n"), marks(2));
    assert_eq!(changed(""), marks(0));
    // `:r !`'s `'[` and `'.` are on the first row put in, `']` on the last;
    // with none, on the line below and the line: nvim `2r !printf
    // "xx\nyyy\n"` {3, 0} and {4, 0}, `3r !true` {4, 0} and {3, 0}.
    let read_marks = |below: u32, output: &str| {
        let mut vim = HeadlessVim::new("aaaa\nbbbb\ncccc\ndddd\neeee", (0, 0));
        host_applies(&mut vim, |state, model| {
            state.apply_read_command(model, below, output)
        });
        [vim.mark('['), vim.mark(']'), vim.mark('.')]
    };
    assert_eq!(
        read_marks(2, "xx\nyyy\n"),
        [Some((2, 0)), Some((3, 0)), Some((2, 0))]
    );
    assert_eq!(
        read_marks(3, ""),
        [Some((3, 0)), Some((2, 0)), Some((3, 0))]
    );

    // A line `!` opened and cancelled isn't the one a later line came
    // from: `.` is still `x`.
    let mut vim = after(PARAGRAPHS, (0, 0), "!!<Esc>x:.!sort<CR>");
    vim.state_mut().take_host_command();
    vim.feed("j.").unwrap();
    assert_eq!(vim.state_mut().take_host_command(), None);
    assert_eq!(vim.text(), "1\n2\na3\n\nb1\nb2\nb3\nb4");

    // `.` after `!ip sort` asks the host again, over the paragraph at the
    // caret, with the same command.
    let mut vim = after(PARAGRAPHS, (0, 0), "!ipsort<CR>");
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(0..3, vec![text("sort")])
    );
    host_applies(&mut vim, |state, model| {
        state.apply_filter(model, 0..3, "a3\na2\na1\n")
    });
    assert_eq!(vim.text(), "a3\na2\na1\n\nb1\nb2\nb3\nb4");
    vim.feed("5G.").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(4..8, vec![text("sort")])
    );
    // Visual: as many rows again.
    let mut vim = after(PARAGRAPHS, (4, 0), "Vj!tac<CR>");
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(4..6, vec![text("tac")])
    );
    vim.feed("gg.").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(0..2, vec![text("tac")])
    );
    // A line typed by hand isn't repeated: `.` is the change before.
    let mut vim = after(PARAGRAPHS, (0, 0), "x:.!sort<CR>");
    vim.state_mut().take_host_command();
    vim.feed("j.").unwrap();
    assert_eq!(vim.state_mut().take_host_command(), None);
    assert_eq!(vim.text(), "1\n2\na3\n\nb1\nb2\nb3\nb4");

    // `:r !` puts the rows below the line as `:r` puts a file's: nvim `2r
    // !printf "p\nq\n"` from {1, 0} leaves the caret on the last, {4, 0},
    // and `u` back at {1, 0}; `0r` above the first ({2, 0}).
    let read = |below: u32, output: &str| {
        let mut vim = HeadlessVim::new("  a\nb\nc", (0, 1));
        host_applies(&mut vim, |state, model| {
            state.apply_read_command(model, below, output)
        });
        let read = (vim.text(), vim.cursor());
        vim.feed("u").unwrap();
        (read, (vim.text(), vim.cursor()))
    };
    let undone = ("  a\nb\nc".to_string(), (0, 1));
    assert_eq!(
        read(2, "p\nq\n"),
        (("  a\nb\np\nq\nc".into(), (3, 0)), undone.clone())
    );
    assert_eq!(
        read(0, "p\nq\n"),
        (("p\nq\n  a\nb\nc".into(), (1, 0)), undone.clone())
    );
    assert_eq!(
        read(3, "  p\nq"),
        (("  a\nb\nc\n  p\nq".into(), (4, 0)), undone.clone())
    );
    assert_eq!(
        read(1, "  p"),
        (("  a\n  p\nb\nc".into(), (1, 2)), undone.clone())
    );
    // Nothing printed: `2r !true` leaves the caret on line 2, `0r !true`
    // on the first.
    assert_eq!(read(2, ""), (("  a\nb\nc".into(), (1, 0)), undone.clone()));
    assert_eq!(read(0, ""), (("  a\nb\nc".into(), (0, 2)), undone));
}

#[test]
fn shell_command_parts() {
    // `%` and `#` are the files, for the host to escape; `\%` and `\#` are
    // themselves.
    assert_eq!(
        line(ROWS, (0, 0), "%!echo % \\% # \\#").0,
        filter(
            0..5,
            vec![
                text("echo "),
                ShellPart::CurrentFile,
                text(" % "),
                ShellPart::AlternateFile,
                text(" #"),
            ]
        )
    );
    assert_eq!(
        line(ROWS, (0, 0), ".!cat %%#").0,
        filter(
            0..1,
            vec![
                text("cat "),
                ShellPart::CurrentFile,
                ShellPart::CurrentFile,
                ShellPart::AlternateFile,
            ]
        )
    );
    // Other backslashes stay, as `%` followed by a colon that's no
    // modifier.
    assert_eq!(
        line(ROWS, (0, 0), ".!sed 's/\\t/ /' %:x").0,
        filter(
            0..1,
            vec![text("sed 's/\\t/ /' "), ShellPart::CurrentFile, text(":x")]
        )
    );
    // A filename modifier, a buffer's number and `<cword>` are refused.
    for (line_text, word) in [
        ("%!echo %:p", "%:p"),
        ("%!echo #:h", "#:h"),
        ("%!cat #2", "#2"),
        ("%!echo <cword>", "<cword>"),
    ] {
        let mut vim = HeadlessVim::new(ROWS, (0, 0));
        let (command, message) = submit_as(&mut vim, line_text, Actor::User);
        assert_eq!(command, None, "{line_text}");
        assert_eq!(
            message,
            Some(format!("{word}: not supported yet")),
            "{line_text}"
        );
    }
    let (command, message) = line(ROWS, (0, 0), "r !cat %:h");
    assert_eq!(
        (command, message),
        (None, Some("%:h: not supported yet".into()))
    );
}

#[test]
fn filters_run_only_for_the_user() {
    let refused = Some(SHELL_REFUSED.to_string());
    assert_eq!(SHELL_REFUSED, "Shell commands run only from your own keys");
    let ai = || Actor::Ai {
        agent: "agent".into(),
        session: "s1".into(),
    };
    for actor in [
        Actor::Host("acme.x".into()),
        ai(),
        Actor::Lsp,
        Actor::Format,
    ] {
        let mut vim = HeadlessVim::new(ROWS, (0, 0));
        assert_eq!(
            submit_as(&mut vim, "%!sort", actor.clone()),
            (None, refused.clone()),
            "{actor:?}"
        );
        assert_eq!(
            submit_as(&mut vim, "r !date", actor.clone()),
            (None, refused.clone()),
            "{actor:?}"
        );
        // Refused, it doesn't become the last command either.
        assert_eq!(
            submit(&mut vim, "%!").1,
            Some("E34: No previous command".into())
        );
        // The user's filter, then `.` by the actor.
        let mut vim = after(PARAGRAPHS, (0, 0), "!ipsort<CR>");
        assert!(vim.state_mut().take_host_command().is_some());
        let repeat = Invocation::of("vim.repeat")
            .unwrap()
            .actor(actor.clone())
            .context(fe_vim::context(vim.state()));
        vim.dispatch(&repeat).unwrap();
        let message = vim.state().message().map(String::from);
        assert_eq!(
            (vim.state_mut().take_host_command(), message),
            (None, refused.clone())
        );
        // And `vim.filter` itself.
        let run = Invocation::of("vim.filter")
            .unwrap()
            .args(serde_json::json!({
                "target": { "motion": "line" },
                "command": { "parts": [{ "text": "sort" }] },
            }))
            .actor(actor)
            .context(fe_vim::context(vim.state()));
        vim.dispatch(&run).unwrap();
        let message = vim.state().message().map(String::from);
        assert_eq!(
            (vim.state_mut().take_host_command(), message),
            (None, refused.clone())
        );
    }
    // The user's `vim.filter` runs.
    let mut vim = HeadlessVim::new(ROWS, (1, 0));
    let run = Invocation::of("vim.filter")
        .unwrap()
        .args(serde_json::json!({
            "target": { "motion": "line" },
            "count": 2,
            "command": { "parts": [{ "text": "sort" }] },
        }));
    vim.dispatch(&run.context(fe_vim::context(vim.state())))
        .unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(1..3, vec![text("sort")])
    );

    // `:g` and `:normal` run none, for the user too.
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    assert_eq!(submit(&mut vim, "g/a/.!sort"), (None, refused.clone()));
    let user = || Actor::User;
    assert_eq!(
        submit_as(&mut vim, "normal !!sort<CR>", user()),
        (None, refused.clone())
    );
    assert_eq!(
        submit_as(&mut vim, "normal :.!sort<CR>", user()),
        (None, refused.clone())
    );
    assert_eq!(submit(&mut vim, "g/a/r !date"), (None, refused.clone()));
    // Nor a map's `<Cmd>fe:vim.repeat<CR>` that a `:normal` suspended
    // for, run by the host (as the user) while it waits.
    let mut vim = after(PARAGRAPHS, (0, 0), "!ipsort<CR>");
    vim.state_mut().take_host_command();
    let map = Remap {
        modes: vec![RemapMode::Normal],
        from: "R".into(),
        to: "<Cmd>fe:vim.repeat<CR>".into(),
        recursive: false,
    };
    let (set, issues) = RemapSet::new(&[map], "\\");
    assert!(issues.is_empty(), "{issues:?}");
    vim.state_mut().set_remaps(std::sync::Arc::new(set));
    vim.state_mut().set_normal_resumes(true);
    submit(&mut vim, "normal R");
    assert!(vim.state().has_normal_suspended());
    let RemapCommand(repeat) = vim.state_mut().take_remap_command().unwrap();
    vim.dispatch(&repeat.context(fe_vim::context(vim.state())))
        .unwrap();
    let message = vim.state().message().map(String::from);
    assert_eq!(
        (vim.state_mut().take_host_command(), message),
        (None, refused.clone())
    );
    // `.` of a filter inside `:normal`.
    let mut vim = after(PARAGRAPHS, (0, 0), "!ipsort<CR>");
    vim.state_mut().take_host_command();
    assert_eq!(submit(&mut vim, "normal ."), (None, refused));
    assert_eq!(vim.text(), PARAGRAPHS);
}

/// A vim with `maps`, each a normal-mode noremap `(from, to)`.
fn mapped(text: &str, maps: &[(&str, &str)]) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, (0, 0));
    let maps: Vec<Remap> = maps
        .iter()
        .map(|&(from, to)| Remap {
            modes: vec![RemapMode::Normal],
            from: from.into(),
            to: to.into(),
            recursive: false,
        })
        .collect();
    let (set, issues) = RemapSet::new(&maps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    vim.state_mut().set_remaps(std::sync::Arc::new(set));
    vim
}

/// Runs the fastedit command a map left, as the host does: as the user.
fn run_remap_command(vim: &mut HeadlessVim) {
    let RemapCommand(command) = vim.state_mut().take_remap_command().unwrap();
    vim.dispatch(&command.context(fe_vim::context(vim.state())))
        .unwrap();
}

#[test]
fn macros_never_reach_the_shell_through_remaps() {
    let refused = Some(SHELL_REFUSED.to_string());
    let maps = [
        ("R", "."),
        ("F", ":.!sort<CR>"),
        ("G", ":r !date<CR>"),
        ("C", "<Cmd>fe:vim.repeat<CR>"),
    ];
    let host_and_message = |vim: &mut HeadlessVim| {
        let message = vim.state().message().map(String::from);
        (vim.state_mut().take_host_command(), message)
    };

    // `R` mapped to `.`: typed (through the user's map, while `qa`
    // records), it asks the host again; replayed by `@a`, it doesn't.
    let mut vim = mapped(PARAGRAPHS, &maps);
    vim.feed("!ipsort<CR>").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(0..3, vec![text("sort")])
    );
    vim.feed("qaRq").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(0..3, vec![text("sort")])
    );
    vim.feed("5gg@a").unwrap();
    assert_eq!(host_and_message(&mut vim), (None, refused.clone()));
    // Then typed again, it's the user's.
    vim.feed("R").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(4..8, vec![text("sort")])
    );

    // A mapped filter line, and a mapped `:r !` line.
    let mut vim = mapped(PARAGRAPHS, &maps);
    vim.feed("qaFq").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(0..1, vec![text("sort")])
    );
    vim.feed("j@a").unwrap();
    assert_eq!(host_and_message(&mut vim), (None, refused.clone()));
    vim.feed("qbGq").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        read(2, vec![text("date")])
    );
    vim.feed("@b").unwrap();
    assert_eq!(host_and_message(&mut vim), (None, refused.clone()));
    // A filter line typed in the macro itself.
    vim.feed("qc:.!tac<CR>q").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(1..2, vec![text("tac")])
    );
    vim.feed("@c").unwrap();
    assert_eq!(host_and_message(&mut vim), (None, refused.clone()));

    // A deferred `<Cmd>fe:vim.repeat<CR>`: the host runs the user's, but
    // not a macro's.
    let mut vim = mapped(PARAGRAPHS, &maps);
    vim.feed("!ipsort<CR>").unwrap();
    vim.state_mut().take_host_command();
    vim.feed("qaCq").unwrap();
    run_remap_command(&mut vim);
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(0..3, vec![text("sort")])
    );
    vim.feed("5gg@a").unwrap();
    run_remap_command(&mut vim);
    assert_eq!(host_and_message(&mut vim), (None, refused.clone()));
    vim.feed("C").unwrap();
    run_remap_command(&mut vim);
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(4..8, vec![text("sort")])
    );
}

#[test]
fn repeating_a_search_filter_sets_the_last_search() {
    // nvim: `!*cat` on foo, then `.` on bar, leaves `\<bar\>` the last
    // search, so `n` goes to the next bar.
    let mut vim = HeadlessVim::new("foo x\nbar\nfoo y\nbar\nbaz bar", (0, 0));
    vim.feed("!*").unwrap();
    assert_eq!(vim.state().command_line_prefill(), ".,.+1!");
    vim.feed("cat<CR>").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(0..2, vec![text("cat")])
    );
    vim.feed("j.").unwrap();
    assert_eq!(
        vim.state_mut().take_host_command(),
        filter(1..3, vec![text("cat")])
    );
    vim.feed("n").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
}

/// Carried from T238's review (`remap.rs`, which this card touches): a `c`
/// after `gc` and a count is matched against the operator-pending maps.
/// nvim with `onoremap c j`: `gc2c` comments three rows, `2gcc` and `gcc`
/// two and one.
#[test]
fn a_counted_gc_target_is_mapped() {
    let check = |keys: &str, expected: &str| {
        let mut vim = HeadlessVim::new("a\nb\nc\nd\ne", (0, 0));
        vim.set_comment_tokens(Some(fe_editor::CommentTokens {
            line: Some("#"),
            block: None,
        }));
        let map = Remap {
            modes: vec![RemapMode::OperatorPending],
            from: "c".into(),
            to: "j".into(),
            recursive: false,
        };
        let (set, issues) = RemapSet::new(&[map], "\\");
        assert!(issues.is_empty(), "{issues:?}");
        vim.state_mut().set_remaps(std::sync::Arc::new(set));
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), expected, "{keys}");
    };
    check("gc2c", "# a\n# b\n# c\nd\ne");
    check("2gcc", "# a\n# b\nc\nd\ne");
    check("gcc", "# a\nb\nc\nd\ne");
}
