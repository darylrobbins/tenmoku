//! The buffer list's commands as the host receives them: `:b`, `:bn`,
//! `:bN`, `:bp`, `:bd`, `:bf`, `:br` and `:bl`. The window and buffer lists
//! are the host's, which the nvim oracle can't see, so the facts are
//! stated here, from nvim 0.12.4 (`nvim --clean`, three files open):
//!
//! - `fullcommand()`: `b`, `bu`, `buf`, `buffe` are `buffer`; `bn`, `bne`
//!   `bnext`; `bN` `bNext`; `bp`, `bpr` `bprevious`; `bd`, `bde`
//!   `bdelete`; `bf` `bfirst`; `br` `brewind`; `bl`, `bla` `blast`; and
//!   `bm` is `bmodified`, `bufd` `bufdo`, `bun` `bunload`, `bw` `bwipeout`,
//!   which fe-vim doesn't run.
//! - `:b 0` and `:b0` are E939, `:0b` and `:0bd` E16; `:b 2 3` is
//!   "E488: Trailing characters: 3", `:b 1x` a name (E94 for it);
//!   `:b 4294967296` looks for buffer 2147483647.
//! - `:bn 0` is E939, `:0bn` goes one on, `:2bn 3` three; `:bn x` and
//!   `:bn 2x` are E488; `:bf 2` and `:bl 1` are E488, `:2bf` is `:bf`.
//! - `:1,3b` and `:%b` go to buffer 3, the range's last; `:b "x` is `:b`.
//! - `:%bd` deletes every buffer, `:2,3bd` two; `:bd x"` looks for `x`.
//! - No `:b` command gives E471: with no argument each acts on the current
//!   buffer. `:bufdo` with none does, but isn't a buffer command here.

use fe_vim::{BufferAddress, BufferCommand, BufferTarget, HeadlessVim, HostCommand};

fn run(line: &str) -> (Option<HostCommand>, Option<String>) {
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(&format!(":{line}<CR>")).unwrap();
    let message = vim.state().message().map(String::from);
    (vim.state_mut().take_host_command(), message)
}

fn host(line: &str) -> BufferCommand {
    match run(line) {
        (Some(HostCommand::Buffer(command)), None) => command,
        other => panic!(":{line} gave {other:?}"),
    }
}

fn error(line: &str) -> String {
    let (command, message) = run(line);
    assert_eq!(command, None, ":{line}");
    message.unwrap_or_else(|| panic!(":{line} gave no message"))
}

fn switch(target: BufferTarget) -> BufferCommand {
    BufferCommand::Switch(target)
}

fn at(number: u32) -> BufferTarget {
    let at = BufferAddress::Number(number);
    BufferTarget::Range {
        first: at,
        last: at,
    }
}

fn name(name: &str) -> BufferTarget {
    BufferTarget::Name(name.into())
}

fn delete(target: BufferTarget, force: bool) -> BufferCommand {
    BufferCommand::Delete { target, force }
}

#[test]
fn buffer() {
    for line in [
        "b", "bu", "buf", "buff", "buffe", "buffer", "b!", "b %", "b \"x",
    ] {
        assert_eq!(host(line), switch(BufferTarget::Current), ":{line}");
    }
    for line in ["b 3", "b3", "buffer 3", "b! 3", "b 3 \"x", "b  3  "] {
        assert_eq!(host(line), switch(BufferTarget::Number(3)), ":{line}");
    }
    for line in ["3b", "3buffer", "1,3b", "3b!"] {
        assert_eq!(host(line), switch(at(3)), ":{line}");
    }
    let last = BufferAddress::Last;
    assert_eq!(
        host("%b"),
        switch(BufferTarget::Range { first: last, last })
    );
    assert_eq!(
        host("$b"),
        switch(BufferTarget::Range { first: last, last })
    );
    let current = BufferAddress::Current;
    assert_eq!(
        host(".b"),
        switch(BufferTarget::Range {
            first: current,
            last: current
        })
    );
    // The argument wins over the range.
    assert_eq!(host("2b 3"), switch(BufferTarget::Number(3)));
    for line in ["b#", "b #", "b # "] {
        assert_eq!(host(line), switch(BufferTarget::Alternate), ":{line}");
    }
    assert_eq!(host("b alp"), switch(name("alp")));
    assert_eq!(host("b src/main.rs "), switch(name("src/main.rs")));
    assert_eq!(host("b abc def"), switch(name("abc def")));
    assert_eq!(host("b 1x"), switch(name("1x")));
    assert_eq!(host("b#3"), switch(name("#3")));
    assert_eq!(
        host("b 4294967296"),
        switch(BufferTarget::Number(2147483647))
    );
    assert_eq!(
        host("b 2147483648"),
        switch(BufferTarget::Number(2147483647))
    );

    assert_eq!(error("b 0"), "E939: Positive count required");
    assert_eq!(error("b0"), "E939: Positive count required");
    assert_eq!(error("0b"), "E16: Invalid range");
    assert_eq!(error("b 2 3"), "E488: Trailing characters: 3");
    assert_eq!(error("b 1 x"), "E488: Trailing characters: x");
    assert_eq!(error("b +1"), "+cmd: not supported yet");
    assert!(error(".+1b").ends_with("not supported yet"));
}

#[test]
fn next_and_previous() {
    for line in ["bn", "bne", "bnex", "bnext", "bn!", "0bn", "bn \"x"] {
        assert_eq!(host(line), BufferCommand::Next(1), ":{line}");
    }
    for line in ["bn 2", "bn2", "2bn", "bnext  2", "5bn 2"] {
        assert_eq!(host(line), BufferCommand::Next(2), ":{line}");
    }
    for line in [
        "bN",
        "bNe",
        "bNext",
        "bp",
        "bpr",
        "bpre",
        "bprevious",
        "bp!",
    ] {
        assert_eq!(host(line), BufferCommand::Previous(1), ":{line}");
    }
    for line in ["bp 3", "3bp", "bN 3", "3bNext"] {
        assert_eq!(host(line), BufferCommand::Previous(3), ":{line}");
    }
    assert_eq!(host("bn 99999999999"), BufferCommand::Next(2147483647));
    assert_eq!(error("bn 0"), "E939: Positive count required");
    assert_eq!(error("bp 0"), "E939: Positive count required");
    assert_eq!(error("bn x"), "E488: Trailing characters: x");
    assert_eq!(error("bn 2x"), "E488: Trailing characters: x");
    assert_eq!(error("bp 2 3"), "E488: Trailing characters: 3");
}

#[test]
fn first_and_last() {
    for line in [
        "bf", "bfi", "bfirst", "bf!", "2bf", "br", "bre", "brewind", "bf \"c",
    ] {
        assert_eq!(host(line), BufferCommand::First, ":{line}");
    }
    for line in ["bl", "bla", "blast", "bl!", "3bl"] {
        assert_eq!(host(line), BufferCommand::Last, ":{line}");
    }
    assert_eq!(error("bf 2"), "E488: Trailing characters: 2");
    assert_eq!(error("bfirst x"), "E488: Trailing characters: x");
    assert_eq!(error("bl 1"), "E488: Trailing characters: 1");
}

#[test]
fn delete_buffers() {
    for line in ["bd", "bde", "bdel", "bdelete", "bd %", "bd \"x"] {
        assert_eq!(host(line), delete(BufferTarget::Current, false), ":{line}");
    }
    assert_eq!(host("bd!"), delete(BufferTarget::Current, true));
    assert_eq!(host("bdelete!"), delete(BufferTarget::Current, true));
    assert_eq!(host("bd 2"), delete(BufferTarget::Number(2), false));
    assert_eq!(host("bd2"), delete(BufferTarget::Number(2), false));
    assert_eq!(host("bd! 1"), delete(BufferTarget::Number(1), true));
    assert_eq!(host("bd 2 \"x"), delete(BufferTarget::Number(2), false));
    assert_eq!(host("2bd"), delete(at(2), false));
    assert_eq!(host("bd#"), delete(BufferTarget::Alternate, false));
    assert_eq!(host("bd alpha"), delete(name("alpha"), false));
    assert_eq!(host("bd x\""), delete(name("x"), false));
    let range = |first, last| BufferTarget::Range { first, last };
    use BufferAddress::{Last, Number};
    assert_eq!(host("%bd"), delete(range(Number(1), Last), false));
    assert_eq!(host("2,3bd!"), delete(range(Number(2), Number(3)), true));
    assert_eq!(host("$bd"), delete(range(Last, Last), false));

    assert_eq!(error("bd 0"), "E939: Positive count required");
    assert_eq!(error("0bd"), "E16: Invalid range");
    assert!(error("bd 2 3").ends_with("not supported yet"));
    assert!(error("bd a b").ends_with("not supported yet"));
}

#[test]
fn other_b_names_are_unchanged() {
    // nvim's `:bufdo`, `:bmodified`, `:bunload`, `:bwipeout`, `:ball`,
    // `:badd` and `:buffers` (`:ls`): not buffer commands here.
    for line in [
        "bufdo",
        "bufd",
        "bm",
        "bmodified",
        "bun",
        "bunload",
        "bw",
        "bwipeout",
        "ba",
        "ball",
        "bad",
        "badd",
        "buffers",
        "bfoo",
        "bnexts",
    ] {
        assert!(error(line).starts_with("E492"), ":{line}");
    }
    // `:bro` is still `:browse`.
    assert_eq!(error("bro"), "E471: Argument required");
}

/// A command after a buffer command would run in the buffer it leaves, as
/// the host switches once the line has run: the line is refused before
/// any of it runs, the text and cursor as they were, nothing for the host.
#[test]
fn no_command_after_one() {
    for line in [
        "bn | 2",
        "bn|bn",
        "b 2 | d",
        "b 99 | d",
        "b alpha|d",
        "bd! | d",
        "bf | d",
        "2bp|d",
    ] {
        let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
        vim.feed(&format!(":{line}<CR>")).unwrap();
        assert_eq!(
            vim.state().message(),
            Some("| after a buffer command: not supported yet"),
            ":{line}"
        );
        assert_eq!(vim.state_mut().take_host_command(), None, ":{line}");
        assert_eq!(vim.text(), "one\ntwo\n", ":{line}");
        assert_eq!(vim.cursor(), (0, 0), ":{line}");
    }
    // A comment's `|` is the comment's.
    assert_eq!(host("bn \"x | d"), BufferCommand::Next(1));
    // One before runs first, as in nvim.
    let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
    vim.feed(":2 | bn<CR>").unwrap();
    assert_eq!(vim.state().message(), None);
    assert_eq!(
        vim.state_mut().take_host_command(),
        Some(HostCommand::Buffer(BufferCommand::Next(1)))
    );
    assert_eq!(vim.cursor(), (1, 0));
}

/// Nor under `:g` or `:normal`, which would go on in the buffer left: the
/// buffer command fails there, stopping them, nothing for the host.
#[test]
fn not_under_global_or_normal() {
    for line in [
        "normal :b 2<lt>CR>x",
        "normal :b 99<lt>CR>x",
        "norm! :bn<lt>CR>dd",
        "g/./bn",
        "g/./bd",
        "v/zzz/b 2",
    ] {
        let mut vim = HeadlessVim::new("one\ntwo\n", (0, 0));
        vim.feed(&format!(":{line}<CR>")).unwrap();
        assert_eq!(
            vim.state().message(),
            Some("a buffer command in :g or :normal: not supported yet"),
            ":{line}"
        );
        assert_eq!(vim.state_mut().take_host_command(), None, ":{line}");
        assert_eq!(vim.text(), "one\ntwo\n", ":{line}");
    }
}

#[test]
fn completes_the_names() {
    let names = |line| fe_vim::Ex::completions(line).map(|(_, names)| names);
    assert_eq!(names("bn"), Some(vec!["bnext"]));
    let all = names("b").unwrap();
    for full in [
        "bNext",
        "bdelete",
        "bfirst",
        "blast",
        "bnext",
        "bprevious",
        "brewind",
        "buffer",
    ] {
        assert!(all.contains(&full), "{full} in {all:?}");
    }
}
