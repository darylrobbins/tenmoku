//! `'[`, `']`, `'.` and the changelist after a `!` filter or `:r !`
//! (`VimState::apply_filter` and `VimState::apply_read_command`) at the
//! buffer's boundaries and for output that changes nothing. Each case was
//! probed with nvim 0.12.4 (`nvim --headless --clean` on five rows, `aaaa`
//! to `eeee`, the filters run through `true`, `cat`, `tr c C` and `echo`;
//! `'[` and `'.` print as {N + 1, 0} for row N, `']` {0, 0} when unset,
//! and `:changes` lists the logical line, `-invalid-` past the end).

use fe_vim::{HeadlessVim, HostCommand};

const ROWS: &str = "aaaa\nbbbb\ncccc\ndddd\neeee";

type Mark = Option<(u32, usize)>;

fn filtered(rows: std::ops::Range<u32>, output: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    let mut state = std::mem::take(vim.state_mut());
    state.apply_filter(vim.model_mut(), rows, output);
    *vim.state_mut() = state;
    vim
}

fn read(below: u32, output: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(ROWS, (0, 0));
    let mut state = std::mem::take(vim.state_mut());
    state.apply_read_command(vim.model_mut(), below, output);
    *vim.state_mut() = state;
    vim
}

fn marks(vim: &HeadlessVim) -> [Mark; 3] {
    ['[', ']', '.'].map(|name| vim.mark(name))
}

/// The lines `:changes` lists, oldest first (its last row, `>`, is the
/// position after them).
fn changelist(vim: &mut HeadlessVim) -> Vec<String> {
    vim.feed(":changes<CR>").unwrap();
    match vim.state_mut().take_host_command() {
        Some(HostCommand::Pick(pick)) => pick
            .rows
            .iter()
            .map(|row| row.cells[1].clone())
            .filter(|line| !line.is_empty())
            .collect(),
        other => panic!(":changes: {other:?}"),
    }
}

#[test]
fn filter_change_marks_at_buffer_boundaries() {
    let at = |row| Some((row, 0));
    // `1,2!true`: `']` unset.
    assert_eq!(marks(&filtered(0..2, "")), [at(0), None, at(0)]);
    // `1,5!true`: the same, the buffer one empty row.
    assert_eq!(marks(&filtered(0..5, "")), [at(0), None, at(0)]);
    // `4,5!true`: `'[` and `'.` on line 4, past the end of the three left;
    // `']` on line 3.
    let vim = filtered(3..5, "");
    assert_eq!(vim.text(), "aaaa\nbbbb\ncccc");
    assert_eq!(marks(&vim), [None, at(2), None]);
    // `0r !true`: `']` unset.
    assert_eq!(marks(&read(0, "")), [at(0), None, at(0)]);
    // `5r !true`: `'[` and `'.` on line 6, past the last of five.
    assert_eq!(marks(&read(5, "")), [None, at(4), None]);
    // `2r !true`, in the middle: line 3 and line 2.
    assert_eq!(marks(&read(2, "")), [at(2), at(1), at(2)]);
    // `0r !echo x` and `5r !echo x`: the rows put in.
    assert_eq!(marks(&read(0, "x\n")), [at(0), at(0), at(0)]);
    assert_eq!(marks(&read(5, "x\n")), [at(5), at(5), at(5)]);
}

#[test]
fn filter_changelist_records_the_logical_position() {
    // `2,4!tr c C` changes line 3 only, `2,4!cat` nothing: line 2 each.
    let changed = "bbbb\nCCCC\ndddd\n";
    assert_eq!(changelist(&mut filtered(1..4, changed)), ["2"]);
    assert_eq!(changelist(&mut filtered(1..4, "bbbb\ncccc\ndddd\n")), ["2"]);
    // Only the last row changes: still line 2 (`2,4!sed s/dddd/DDDD/`).
    assert_eq!(changelist(&mut filtered(1..4, "bbbb\ncccc\nDDDD\n")), ["2"]);
    // Empty filters and reads add an entry too, at the first row put in's
    // place: `1,2!true` line 1, `4,5!true` line 4 (past the end), `0r !true`
    // line 1, `5r !true` line 6 (past the end), `2r !true` line 3.
    assert_eq!(changelist(&mut filtered(0..2, "")), ["1"]);
    assert_eq!(changelist(&mut filtered(3..5, "")), ["4"]);
    assert_eq!(changelist(&mut read(0, "")), ["1"]);
    assert_eq!(changelist(&mut read(5, "")), ["6"]);
    assert_eq!(changelist(&mut read(2, "")), ["3"]);
    assert_eq!(changelist(&mut read(5, "x\n")), ["6"]);
    // `1,5!true` line 1.
    assert_eq!(changelist(&mut filtered(0..5, "")), ["1"]);
}
