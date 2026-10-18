//! `:marks [chars]` as the host receives it: a `HostCommand::Pick` whose rows
//! are neovim's list, and whose choice jumps as `` `x `` does.

use fe_vim::{HeadlessVim, HostCommand, Pick, PickThen};

const TEXT: &str = "alpha one\n\t beta\x01 two\ngamma three\ndelta";

/// Marks `a`, `B` and `c`, a jump (so `'` is set), a visual extent (`<`,
/// `>`) and an insert (`[`, `]`, `^`, `.`).
const SETUP: &str = "wmajjmBjlllmcggvl<Esc>jA x<Esc>gg";

fn vim() -> HeadlessVim {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed(SETUP).unwrap();
    vim
}

fn run(vim: &mut HeadlessVim, line: &str) -> (Option<HostCommand>, Option<String>) {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn picked(vim: &mut HeadlessVim, line: &str) -> Pick {
    match run(vim, line) {
        (Some(HostCommand::Pick(pick)), _) => pick,
        other => panic!(":{line}: {other:?}"),
    }
}

fn cells(pick: &Pick) -> Vec<Vec<&str>> {
    pick.rows
        .iter()
        .map(|row| row.cells.iter().map(String::as_str).collect())
        .collect()
}

#[test]
fn marks_list_as_neovim() {
    // nvim 0.12 `--clean --headless`, `nvim_exec2('marks', {output = true})`
    // after the same keys on the same text. nvim's `"` mark (row 1, col 0)
    // and its file marks in other files have no counterpart in fe-vim, so
    // the lists leave them out.
    let mut vim = vim();
    let pick = picked(&mut vim, "marks");
    assert_eq!(pick.title, "Marks");
    assert_eq!(pick.header, "mark line  col file/text");
    assert_eq!(
        cells(&pick),
        [
            ["'", "2", "12", "beta^A two x"],
            ["a", "1", "6", "alpha one"],
            ["c", "4", "4", "delta"],
            ["B", "3", "6", "gamma three"],
            ["[", "2", "11", "beta^A two x"],
            ["]", "2", "13", "beta^A two x"],
            ["^", "2", "13", "beta^A two x"],
            [".", "2", "11", "beta^A two x"],
            ["<", "1", "6", "alpha one"],
            [">", "1", "7", "alpha one"],
        ]
    );
    let then: Vec<_> = pick.rows.iter().map(|row| row.then.clone()).collect();
    assert_eq!(then[1], PickThen::Jump('a'));

    // Named by characters, in nvim's order whichever order they are typed.
    for line in ["marks aB", "marks Ba", "marks a B", "marks  B a "] {
        assert_eq!(
            cells(&picked(&mut vim, line)),
            [["a", "1", "6", "alpha one"], ["B", "3", "6", "gamma three"]],
            "{line}"
        );
    }
    assert_eq!(
        cells(&picked(&mut vim, "marks b a")),
        [["a", "1", "6", "alpha one"]]
    );

    let (command, message) = run(&mut vim, "marks zq");
    assert_eq!(command, None);
    assert_eq!(message.as_deref(), Some("E283: No marks matching \"zq\""));
    let (command, message) = run(&mut vim, "2marks");
    assert_eq!(command, None);
    assert_eq!(message.as_deref(), Some("E481: No range allowed"));
    let (command, message) = run(&mut vim, "marks!");
    assert_eq!(command, None);
    assert_eq!(message.as_deref(), Some("E477: No ! allowed"));
}

#[test]
fn picking_a_mark_jumps_as_backtick() {
    let mut vim = vim();
    let pick = picked(&mut vim, "marks");
    for name in ['a', 'B', '\'', '<', '>'] {
        let row = pick
            .rows
            .iter()
            .find(|row| row.then == PickThen::Jump(name))
            .unwrap();
        let mut picker = self::vim();
        picker.feed("G").unwrap();
        // The context the editor publishes, as the host's dispatch carries it.
        let invocation = row
            .then
            .invocation()
            .context(fe_vim::context(picker.state()));
        picker.dispatch(&invocation).unwrap();
        let mut twin = self::vim();
        twin.feed(&format!("G`{name}")).unwrap();
        assert_eq!(picker.cursor(), twin.cursor(), "{name}");
        assert_eq!(picker.mode(), twin.mode(), "{name}");
        assert_eq!(picker.mark('\''), twin.mark('\''), "{name}");
    }
}

// `:r`'s range, validated before the lines are read.

/// `:{line}` on a three-line text, the host command it left and the message.
fn read_range(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("a\nb\nc", (1, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn read_below(below: u32) -> Option<HostCommand> {
    Some(HostCommand::Read {
        path: Some("f.txt".into()),
        below,
    })
}

#[test]
fn read_validates_its_range() {
    for (line, error) in [
        ("0-1r f.txt", "E16: Invalid range"),
        ("-3r f.txt", "E16: Invalid range"),
        ("3,0r f.txt", "E493: Backwards range given"),
        ("4r f.txt", "E16: Invalid range"),
    ] {
        let (command, message) = read_range(line);
        assert_eq!(command, None, "{line}");
        assert_eq!(message.as_deref(), Some(error), "{line}");
    }
    assert_eq!(read_range("0r f.txt").0, read_below(0));
    assert_eq!(read_range("0,0r f.txt").0, read_below(0));
    assert_eq!(read_range("1,3r f.txt").0, read_below(3));
}
