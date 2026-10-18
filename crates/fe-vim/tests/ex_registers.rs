//! `:reg[isters]` and `:di[splay]` as the host receives them: a
//! `HostCommand::Pick` whose rows are neovim's list, and whose choice puts
//! the register as `"xp` does.

use fe_vim::{HeadlessVim, HostCommand, Pick, PickThen};

const TEXT: &str = "alpha one\nbeta \x01\ttwo\ngamma three\ndelta\nx\ty\x1bz";

/// A charwise yank into `0` and `b`, a linewise one into `a`, a linewise
/// delete into `1`, a linewise yank into `x`, a `<C-v>` yank into `k`, a charwise yank of the last line, `x<Tab>y<Esc>z`, into `c`,
/// and a search for `/`.
const SETUP: &str = "wyw\"ayyj\"bywjdd\"xyyggl<C-v>jl\"kyG0\"cy$/gam<CR>";

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
fn registers_list_as_neovim() {
    // nvim 0.12.6 `--clean --headless`, `nvim_exec2('registers', {output =
    // true})` after the same keys on the same text. nvim's `.`, `:`, `%`, `#`
    // and clipboard rows have no counterpart here (the clipboard is empty),
    // and the lists leave them out.
    let mut vim = vim();
    let pick = picked(&mut vim, "registers");
    assert_eq!(pick.title, "Registers");
    assert_eq!(pick.header, "Type Name Content");
    assert_eq!(
        cells(&pick),
        [
            ["c", "\"\"", "x^Iy^[z"],
            ["c", "\"0", "one"],
            ["l", "\"1", "gamma three^J"],
            ["l", "\"a", "alpha one^J"],
            ["c", "\"b", "^A^I"],
            ["c", "\"c", "x^Iy^[z"],
            ["b", "\"k", " on^J^A^I"],
            ["l", "\"x", "delta^J"],
            ["c", "\"/", "gam"],
        ]
    );
    assert_eq!(pick.rows[3].then, PickThen::Put('a'));

    assert_eq!(
        cells(&picked(&mut vim, "registers a x c")),
        [
            ["l", "\"a", "alpha one^J"],
            ["c", "\"c", "x^Iy^[z"],
            ["l", "\"x", "delta^J"]
        ]
    );
    assert_eq!(
        cells(&picked(&mut vim, "di \"0")),
        [["c", "\"\"", "x^Iy^[z"], ["c", "\"0", "one"]]
    );
    for line in ["reg b a", "regi a b", "display ba"] {
        assert_eq!(
            cells(&picked(&mut vim, line)),
            [["l", "\"a", "alpha one^J"], ["c", "\"b", "^A^I"]],
            "{line}"
        );
    }

    // Blanks around the names are ignored, none left is every register.
    let all = cells(&picked(&mut vim, "registers"))
        .iter()
        .map(|cells| {
            cells
                .iter()
                .map(|cell| cell.to_string())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for line in ["reg ", "reg  \t", "di ", "di \t", "registers\t "] {
        let rows = cells(&picked(&mut vim, line))
            .iter()
            .map(|cells| {
                cells
                    .iter()
                    .map(|cell| cell.to_string())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(rows, all, "{line:?}");
    }

    // Nothing set, or no such register: the header alone, with no error.
    for line in ["reg !", "reg q"] {
        let (command, message) = run(&mut vim, line);
        match command {
            Some(HostCommand::Pick(pick)) => assert!(pick.rows.is_empty(), "{line}"),
            other => panic!(":{line}: {other:?}"),
        }
        assert_eq!(message, None, "{line}");
    }

    assert_eq!(
        run(&mut vim, "reg!"),
        (None, Some("E477: No ! allowed".into()))
    );
    assert_eq!(
        run(&mut vim, "2reg"),
        (None, Some("E481: No range allowed".into()))
    );
}

#[test]
fn picking_a_register_puts_as_quote_p() {
    for name in ['0', 'a', 'k'] {
        let mut vim = vim();
        let pick = picked(&mut vim, "registers");
        let row = pick
            .rows
            .iter()
            .find(|row| row.then == PickThen::Put(name))
            .unwrap_or_else(|| panic!("no row for {name}"));
        let mut twin = self::vim();
        picked(&mut twin, "registers");
        let invocation = row.then.invocation().context(fe_vim::context(vim.state()));
        vim.dispatch(&invocation).unwrap();
        twin.feed(&format!("\"{name}p")).unwrap();
        assert_eq!(vim.text(), twin.text(), "{name}");
        assert_eq!(vim.cursor(), twin.cursor(), "{name}");
        assert_eq!(vim.mode(), twin.mode(), "{name}");
        assert_eq!(vim.registers(), twin.registers(), "{name}");
    }
}
