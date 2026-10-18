//! `Ex::completions`: the command line's names for `<Tab>`, nvim's
//! `getcompletion(prefix, 'cmdline')` lists (probed on nvim 0.12.x with
//! `--clean --headless`) kept to the commands fe-vim runs.

use fe_vim::{Ex, ExError};

fn runs(name: &str) -> bool {
    !matches!(Ex::parse(name), Err(ExError::NotACommand(_)))
}

/// nvim's list for the prefix, as probed, and the offset of the name.
fn check(line: &str, offset: usize, nvim: &[&str]) {
    let expected: Vec<&str> = nvim.iter().copied().filter(|name| runs(name)).collect();
    assert_eq!(Ex::completions(line), Some((offset, expected)), "{line}");
}

#[test]
fn completions_match_neovim_for_commands_fe_vim_runs() {
    assert_eq!(Ex::completions("s/x"), None);
    check("noh", 0, &["nohlsearch"]);
    check("sor", 0, &["sort"]);
    check("norm", 0, &["normal"]);
    check("pu", 0, &["put"]);
    check("marks", 0, &["marks"]);
    check("reg", 0, &["registers"]);
    check("'<,'>noh", 5, &["nohlsearch"]);
    check(":  y", 3, &["yank"]);
    check("j", 0, &["join", "jumps"]);
    check("qa", 0, &["qall"]);
    check("wa", 0, &["wall"]);
    check("Ex", 0, &["Explore"]);
    check("cop", 0, &["copen", "copy"]);
    check("diffs", 0, &["diffsplit"]);
    check(
        "x",
        0,
        &[
            "xall",
            "xit",
            "xmap",
            "xmapclear",
            "xmenu",
            "xnoremap",
            "xnoremenu",
            "xunmap",
            "xunmenu",
        ],
    );
    check("q", 0, &["qall", "quit", "quitall"]);
    check("wq", 0, &["wq", "wqall"]);
    check(
        "e",
        0,
        &["earlier", "echo", "edit", "ex", "exit", "exusage"],
    );
    check(
        "w",
        0,
        &[
            "wNext",
            "wall",
            "while",
            "wincmd",
            "windo",
            "winpos",
            "winsize",
            "wnext",
            "wprevious",
            "wq",
            "wqall",
            "write",
            "wshada",
            "wundo",
            "wviminfo",
        ],
    );
    check(
        "d",
        0,
        &[
            "debug",
            "debuggreedy",
            "defer",
            "delcommand",
            "delete",
            "delfunction",
            "delmarks",
            "detach",
            "diffget",
            "diffoff",
            "diffpatch",
            "diffput",
            "diffsplit",
            "diffthis",
            "diffupdate",
            "digraphs",
            "display",
            "djump",
            "dlist",
            "doautoall",
            "doautocmd",
            "drop",
            "dsearch",
            "dsplit",
        ],
    );
    check(
        "s",
        0,
        &[
            "sNext",
            "sall",
            "sandbox",
            "sargument",
            "saveas",
            "sbNext",
            "sball",
            "sbfirst",
            "sblast",
            "sbmodified",
            "sbnext",
            "sbprevious",
            "sbrewind",
            "sbuffer",
            "scriptencoding",
            "scriptnames",
            "set",
            "setfiletype",
            "setglobal",
            "setlocal",
            "sfind",
            "sfirst",
            "sign",
            "silent",
            "simalt",
            "slast",
            "sleep",
            "smagic",
            "smap",
            "smapclear",
            "smenu",
            "snext",
            "snomagic",
            "snoremap",
            "snoremenu",
            "sort",
            "source",
            "spelldump",
            "spellgood",
            "spellinfo",
            "spellrare",
            "spellrepall",
            "spellundo",
            "spellwrong",
            "split",
            "sprevious",
            "srewind",
            "stag",
            "startgreplace",
            "startinsert",
            "startreplace",
            "stjump",
            "stop",
            "stopinsert",
            "stselect",
            "substitute",
            "sunhide",
            "sunmap",
            "sunmenu",
            "suspend",
            "sview",
            "swapname",
            "syncbind",
            "syntax",
            "syntime",
        ],
    );
}

#[test]
fn the_name_must_end_the_line() {
    assert_eq!(Ex::completions("s/x"), None);
    assert_eq!(Ex::completions("noh "), None);
    assert_eq!(Ex::completions("'<,'>"), None);
    assert_eq!(Ex::completions(""), None);
}

#[test]
fn every_completion_is_a_command() {
    let mut seen = 0;
    for letter in ('a'..='z').chain('A'..='Z') {
        let (_, names) = Ex::completions(&letter.to_string()).unwrap();
        for name in names {
            seen += 1;
            assert!(runs(name), "{name} is E492");
        }
    }
    assert!(seen >= 30, "{seen}");
}
