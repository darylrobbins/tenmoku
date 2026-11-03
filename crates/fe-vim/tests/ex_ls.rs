//! `:ls`, `:buffers` and `:files` as the host receives them: a
//! `HostCommand::Buffer(BufferCommand::List)`, abbreviated as nvim 0.12.4
//! does (`fullcommand()`: `ls`, `buffers` and `files` have no shorter
//! forms; `:l` is `:list`, `:buffer` and `:file` are other commands). The
//! host's `Pick` of the rows has the columns number, flags, name and line.

use fe_vim::{BufferCommand, BufferRow, HeadlessVim, HostCommand, Pick, PickThen};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

#[test]
fn the_three_names_list() {
    for line in [
        "ls", "buffers", "files", "ls!", "buffers!", "files!", "  ls", "ls \"x",
    ] {
        assert_eq!(
            run(line),
            (Some(HostCommand::Buffer(BufferCommand::List)), None),
            ":{line}"
        );
    }
}

#[test]
fn abbreviations_are_not_the_list() {
    for line in ["buffer", "bu", "buf", "buffer!"] {
        let (command, _) = run(line);
        assert_ne!(
            command,
            Some(HostCommand::Buffer(BufferCommand::List)),
            ":{line}"
        );
    }
    for line in ["lsx", "buffer s", "filesx", "bufferss"] {
        let (command, message) = run(line);
        assert_ne!(
            command,
            Some(HostCommand::Buffer(BufferCommand::List)),
            ":{line}"
        );
        let _ = message;
    }
}

#[test]
fn a_range_is_refused() {
    let (command, message) = run("1ls");
    assert_eq!(command, None);
    assert_eq!(message.as_deref(), Some("E481: No range allowed"));
}

#[test]
fn the_pick_has_the_columns() {
    let row = |number, current, alternate, active, modified, name: &str, line| BufferRow {
        number,
        current,
        alternate,
        active,
        modified,
        name: name.into(),
        line,
    };
    let pick = Pick::buffers([
        row(1, true, false, true, false, "a.txt", 3),
        row(2, false, true, false, true, "src/b.rs", 1),
        row(4, false, false, true, false, "[No Name]", 12),
    ]);
    assert_eq!(pick.title, "Buffers");
    assert_eq!(pick.header, "nr  flags name  line");
    let cells: Vec<Vec<&str>> = pick
        .rows
        .iter()
        .map(|row| row.cells.iter().map(String::as_str).collect())
        .collect();
    assert_eq!(
        cells,
        [
            ["1", "%a", "a.txt", "line 3"],
            ["2", "#h+", "src/b.rs", "line 1"],
            ["4", "a", "[No Name]", "line 12"],
        ]
    );
    let then: Vec<&PickThen> = pick.rows.iter().map(|row| &row.then).collect();
    assert_eq!(
        then,
        [
            &PickThen::Buffer(1),
            &PickThen::Buffer(2),
            &PickThen::Buffer(4)
        ]
    );
    let invocation = PickThen::Buffer(2).invocation();
    assert_eq!(invocation.command.as_str(), "vim.ex_line");
    assert_eq!(invocation.args, serde_json::json!({"text": "b 2"}));
}
