//! `:jumps` and `:changes` as the host receives them: a `HostCommand::Pick`
//! whose rows are neovim's lists, and whose choice moves through the list as
//! `<C-o>`/`<C-i>` or `g;`/`g,` with that count do.

use fe_vim::{HeadlessVim, HostCommand, Pick};

const TEXT: &str = "alpha one\n\t beta two\ngamma three\ndelta\nepsilon\nzeta";

/// Jumps (`j` isn't one), edits on rows 3, 6 and 4, and back to the top.
const SETUP: &str = "jGggjjxGxggjjjix<Esc>gg";

fn vim(keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed(SETUP).unwrap();
    vim.feed(keys).unwrap();
    vim
}

fn picked(vim: &mut HeadlessVim, line: &str) -> Pick {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    match vim.state_mut().take_host_command() {
        Some(HostCommand::Pick(pick)) => pick,
        other => panic!(":{line}: {other:?}"),
    }
}

fn cells(pick: &Pick) -> Vec<Vec<&str>> {
    pick.rows
        .iter()
        .map(|row| row.cells.iter().map(String::as_str).collect())
        .collect()
}

/// Choosing row `ix` of `line`'s list, in a vim after `keys`, against `twin`
/// typed in another after the same.
fn picking(line: &str, keys: &str, ix: usize, twin: &str) {
    let mut vim = vim(keys);
    let pick = picked(&mut vim, line);
    let invocation = pick.rows[ix]
        .then
        .invocation()
        .context(fe_vim::context(vim.state()));
    vim.dispatch(&invocation).unwrap();
    let mut other = self::vim(keys);
    other.feed(twin).unwrap();
    assert_eq!(vim.cursor(), other.cursor(), ":{line} row {ix}");
    // The list position moved the same: the same step goes the same way.
    for next in ["<C-o>", "<C-i>", "g;", "g,"] {
        vim.feed(next).unwrap();
        other.feed(next).unwrap();
        assert_eq!(vim.cursor(), other.cursor(), ":{line} row {ix} then {next}");
    }
}

#[test]
fn jumps_and_changes_list_as_nvim() {
    // nvim 0.12 `--clean --headless`, the same keys fed with
    // `nvim_feedkeys(keys, 'tx')` on the same text and `nvim_exec2('jumps')`
    // and `('changes')` after. (Probed with nvim 0.12.6, the release binary.)
    // nvim's first row there, `5 1 0 alpha one`, is opening the file's own
    // jump, which fe-vim has no counterpart of, so the fresh list leaves it
    // out (a later entry on its row, as in the second list, hides it).
    let mut fresh = vim("");
    let pick = picked(&mut fresh, "jumps");
    assert_eq!(pick.title, "Jumps");
    assert_eq!(pick.header, "jump line  col file/text");
    assert_eq!(
        cells(&pick),
        [
            ["4", "2", "0", "beta two"],
            ["3", "3", "0", "amma three"],
            ["2", "6", "0", "eta"],
            ["1", "4", "0", "xdelta"],
            [">", "", "", ""],
        ]
    );
    let pick = picked(&mut fresh, "changes");
    assert_eq!(pick.title, "Changes");
    assert_eq!(pick.header, "change line  col text");
    assert_eq!(
        cells(&pick),
        [
            ["3", "3", "0", "amma three"],
            ["2", "6", "0", "eta"],
            ["1", "4", "0", "xdelta"],
            [">", "", "", ""],
        ]
    );

    // After `<C-o><C-o>g;` the lists are at their entries.
    let mut vim = vim("<C-o><C-o>g;");
    assert_eq!(
        cells(&picked(&mut vim, "jumps")),
        [
            ["2", "2", "0", "beta two"],
            ["1", "3", "0", "amma three"],
            [">0", "6", "0", "eta"],
            ["1", "4", "0", "xdelta"],
            ["2", "1", "0", "alpha one"],
        ]
    );
    assert_eq!(
        cells(&picked(&mut vim, "changes")),
        [
            ["2", "3", "0", "amma three"],
            ["1", "6", "0", "eta"],
            [">0", "4", "0", "xdelta"],
        ]
    );
    assert_eq!(picked(&mut vim, "ju").title, "Jumps");

    // Choosing a row goes as that many `<C-o>`s, `<C-i>`s, `g;`s or `g,`s.
    picking("jumps", "", 0, "4<C-o>");
    picking("jumps", "", 2, "2<C-o>");
    picking("jumps", "", 3, "<C-o>");
    picking("jumps", "", 4, "");
    picking("jumps", "<C-o><C-o>g;", 0, "2<C-o>");
    picking("jumps", "<C-o><C-o>g;", 4, "2<C-i>");
    picking("changes", "", 0, "3g;");
    picking("changes", "", 2, "g;");
    picking("changes", "g;g;", 2, "g,");
    picking("changes", "g;g;", 0, "g;");
}

#[test]
fn jumps_and_changes_take_no_arguments() {
    let mut vim = vim("");
    for (line, error) in [
        ("2jumps", "E481: No range allowed"),
        ("jumps!", "E477: No ! allowed"),
        ("changes x", "E488: Trailing characters: x"),
    ] {
        vim.feed(&format!(":{line}<CR>")).unwrap();
        assert_eq!(vim.state_mut().take_host_command(), None, "{line}");
        assert_eq!(vim.state().message(), Some(error), "{line}");
    }
}

/// The row a list is at goes to that entry, wherever the cursor has moved
/// since, and the synthetic last row stays where the cursor is.
#[test]
fn the_current_row_goes_to_its_entry() {
    // `<C-o><C-o>` is at row 6, then `g;` moved to row 4.
    let mut vim = vim("<C-o><C-o>g;");
    assert_eq!(vim.cursor(), (3, 0));
    let pick = picked(&mut vim, "jumps");
    let invocation = pick.rows[2].then.invocation();
    vim.dispatch(&invocation.context(fe_vim::context(vim.state())))
        .unwrap();
    assert_eq!(vim.cursor(), (5, 0));
    // Still at that entry: one `<C-i>` goes to the next.
    vim.feed("<C-i>").unwrap();
    assert_eq!(vim.cursor(), (3, 0));

    let mut vim = self::vim("l");
    let pick = picked(&mut vim, "changes");
    let last = pick.rows.last().unwrap();
    assert_eq!(last.cells[0], ">");
    let invocation = last.then.invocation();
    vim.dispatch(&invocation.context(fe_vim::context(vim.state())))
        .unwrap();
    assert_eq!(vim.cursor(), (0, 1));
}

/// `<C-o>` drops the newest entry on the cursor's row first, so the rows
/// before it are one fewer away than they are listed.
#[test]
fn picking_with_the_newest_entry_on_the_cursor_row() {
    let mut vim = HeadlessVim::new("one\ntwo\nthree\nfour", (0, 0));
    vim.feed("Gggjjj").unwrap();
    let pick = picked(&mut vim, "jumps");
    assert_eq!(
        cells(&pick),
        [
            ["2", "1", "0", "one"],
            ["1", "4", "0", "four"],
            [">", "", "", ""]
        ]
    );
    for (ix, want) in [(0, (0, 0)), (1, (3, 0)), (2, (3, 0))] {
        let mut vim = HeadlessVim::new("one\ntwo\nthree\nfour", (0, 0));
        vim.feed("Gggjjj").unwrap();
        let pick = picked(&mut vim, "jumps");
        let invocation = pick.rows[ix].then.invocation();
        vim.dispatch(&invocation.context(fe_vim::context(vim.state())))
            .unwrap();
        assert_eq!(vim.cursor(), want, "row {ix}");
    }
}
