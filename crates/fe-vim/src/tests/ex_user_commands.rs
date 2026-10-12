use crate::{ExError, HeadlessVim, HostCommand as H};

fn host(text: &str, keys: &str) -> (HeadlessVim, Option<H>) {
    let mut vim = HeadlessVim::new(text, (1, 1));
    vim.feed(keys).unwrap();
    let command = vim.state_mut().take_host_command();
    (vim, command)
}

const TEXT: &str = "a\nb\nc\nd\ne\nf";

fn user_cmd(name: &str, bang: bool, range: Option<std::ops::Range<u32>>, args: &str) -> Option<H> {
    Some(H::User {
        name: name.into(),
        bang,
        range,
        args: args.into(),
    })
}

#[test]
fn user() {
    assert_eq!(
        host(TEXT, ":Gitsigns stage_hunk<CR>").1,
        user_cmd("Gitsigns", false, None, "stage_hunk")
    );
    assert_eq!(
        host(TEXT, "2GVjj<Esc>:'<,'>Gitsigns stage_hunk<CR>").1,
        user_cmd("Gitsigns", false, Some(1..4), "stage_hunk")
    );
    assert_eq!(
        host(TEXT, "3GVjj<Esc>:'<,'>Gitsigns stage_hunk<CR>").1,
        user_cmd("Gitsigns", false, Some(2..5), "stage_hunk")
    );
    assert_eq!(
        host(TEXT, ":Git! commit --amend<CR>").1,
        user_cmd("Git", true, None, "commit --amend")
    );
    assert_eq!(
        host(TEXT, ":Gitsigns<CR>").1,
        user_cmd("Gitsigns", false, None, "")
    );
    assert_eq!(host(TEXT, ":gitsigns<CR>").1, None);
    assert_eq!(
        ExError::NotACommand("gitsigns".into()).to_string(),
        "E492: Not an editor command: gitsigns"
    );
    let (vim, _) = host(TEXT, ":gitsigns<CR>");
    assert_eq!(
        vim.state().message(),
        Some("E492: Not an editor command: gitsigns")
    );
}

#[test]
fn diff_commands() {
    let get = |rows| Some(H::DiffGet { rows });
    let put = |rows| Some(H::DiffPut { rows });
    for line in ["diffget", "diffg"] {
        assert_eq!(host(TEXT, &format!("3G:{line}<CR>")).1, get(2..3), "{line}");
    }
    assert_eq!(host(TEXT, ":3,4diffget<CR>").1, get(2..4));
    for line in ["diffput", "diffpu"] {
        assert_eq!(host(TEXT, &format!("2G:{line}<CR>")).1, put(1..2), "{line}");
    }
    assert_eq!(host(TEXT, ":3,4diffput<CR>").1, put(2..4));
    for line in ["diffsplit a.rs", "diffs a.rs"] {
        assert_eq!(
            host(TEXT, &format!(":{line}<CR>")).1,
            Some(H::DiffSplit {
                path: "a.rs".into()
            })
        );
    }
    let (vim, command) = host(TEXT, ":diffsplit<CR>");
    assert_eq!(command, None);
    assert_eq!(vim.state().message(), Some("E471: Argument required"));
}

#[test]
fn zz_zq() {
    let wq = Some(H::WriteQuit {
        force: false,
        only_if_modified: true,
    });
    assert_eq!(host(TEXT, "ZZ").1, wq);
    assert_eq!(host(TEXT, "ZQ").1, Some(H::Quit { force: true }));
    let (vim, command) = host(TEXT, "Zx");
    assert_eq!(command, None);
    assert_eq!(vim.state().pending().kind(), None);
    assert_eq!(vim.text(), TEXT);
    // `z` keys are untouched.
    let (vim, _) = host(TEXT, "zZ");
    assert_eq!(vim.state().pending().kind(), None);
}
