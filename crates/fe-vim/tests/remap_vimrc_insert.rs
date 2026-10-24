use fe_vim::{Remap, RemapMode, VimrcIssue, parse_vimrc};

const I: RemapMode = RemapMode::Insert;
const C: RemapMode = RemapMode::CmdLine;
const N: RemapMode = RemapMode::Normal;
const X: RemapMode = RemapMode::Visual;
const O: RemapMode = RemapMode::OperatorPending;

fn remap(modes: &[RemapMode], from: &str, to: &str, recursive: bool) -> Remap {
    Remap {
        modes: modes.to_vec(),
        from: from.into(),
        to: to.into(),
        recursive,
    }
}

#[test]
fn insert_and_cmdline_commands_load() {
    let vimrc = parse_vimrc(
        "imap a b\ninoremap c d\ncmap e f\ncnoremap g h\nmap! i j\nnoremap! k l\nmap m n\nnoremap o p\n",
    );
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![
            remap(&[I], "a", "b", true),
            remap(&[I], "c", "d", false),
            remap(&[C], "e", "f", true),
            remap(&[C], "g", "h", false),
            remap(&[I, C], "i", "j", true),
            remap(&[I, C], "k", "l", false),
            remap(&[N, X, O], "m", "n", true),
            remap(&[N, X, O], "o", "p", false),
        ]
    );
}

#[test]
fn insert_and_cmdline_unmap_and_clear() {
    let vimrc = parse_vimrc(
        "imap a 1\ncmap a 2\nmap! b 3\niunmap a\ncunmap b\nimap c 4\nimapclear\ncmap d 5\ncmapclear\nmap! e 6\nunmap! e\nmap! f 7\n",
    );
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(vimrc.remaps, vec![remap(&[I, C], "f", "7", true),]);

    // `iunmap` and `cunmap` take out one mode of a `map!`.
    let vimrc = parse_vimrc("imap a 1\ncmap a 2\nmap! b 3\niunmap a\ncunmap b\n");
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(
        vimrc.remaps,
        vec![remap(&[C], "a", "2", true), remap(&[I], "b", "3", true)]
    );

    // `mapclear!` clears both modes, and what follows it still loads.
    let vimrc = parse_vimrc("imap a 1\ncmap b 2\nmapclear!\ninoremap g 8\n");
    assert_eq!(vimrc.issues, vec![]);
    assert_eq!(vimrc.remaps, vec![remap(&[I], "g", "8", false)]);
}

#[test]
fn nmap_bang_is_still_unsupported() {
    let vimrc = parse_vimrc("nmap! a b\n");
    assert_eq!(
        vimrc.issues,
        vec![VimrcIssue {
            line: 1,
            message: "not supported: nmap!".into()
        }]
    );
    assert_eq!(vimrc.remaps, vec![]);
}
