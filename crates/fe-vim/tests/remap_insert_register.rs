//! The key after `<C-r>` in insert and replace mode names a register,
//! read unmapped as nvim reads it; and insert and cmdline maps hide no
//! built-in (`RemapSet::hides_builtin`).

use std::sync::Arc;

use fe_vim::{HeadlessVim, Remap, RemapMode, RemapSet};

const I: RemapMode = RemapMode::Insert;
const C: RemapMode = RemapMode::CmdLine;

fn noremap(modes: &[RemapMode], from: &str, to: &str) -> Remap {
    Remap {
        modes: modes.to_vec(),
        from: from.into(),
        to: to.into(),
        recursive: false,
    }
}

/// "one two" with `one` in register `a` and `two` in `b`, `inoremap a b`.
fn vim() -> HeadlessVim {
    let mut vim = HeadlessVim::new("one two", (0, 0));
    vim.feed("\"ayiww\"byiw0").unwrap();
    let (set, issues) = RemapSet::new(&[noremap(&[I], "a", "b")], "\\");
    assert!(issues.is_empty(), "{issues:?}");
    vim.state_mut().set_remaps(Arc::new(set));
    vim
}

#[test]
fn insert_register_name_is_read_unmapped() {
    for modifier in ["", "<C-r>", "<C-o>", "<C-p>"] {
        let mut vim = vim();
        vim.feed(&format!("A <C-r>{modifier}a<Esc>")).unwrap();
        assert_eq!(vim.text(), "one two one", "<C-r>{modifier}a");
        // The map still applies to the typed text after it.
        vim.feed("A a<Esc>").unwrap();
        assert_eq!(vim.text(), "one two one b", "<C-r>{modifier}a");
    }
}

#[test]
fn replace_register_name_is_read_unmapped() {
    // `<C-r><C-o>` and `<C-r><C-p>` insert the text as it is, even here.
    for (modifier, start, end) in [
        ("", "one two", "one one"),
        ("<C-r>", "one two", "one one"),
        ("<C-o>", "oneone two", "one onetwo"),
        ("<C-p>", "oneone two", "one onetwo"),
    ] {
        let mut vim = vim();
        vim.feed(&format!("R<C-r>{modifier}a<Esc>")).unwrap();
        assert_eq!(vim.text(), start, "R<C-r>{modifier}a");
        let mut vim = self::vim();
        vim.feed(&format!("wR<C-r>{modifier}a<Esc>")).unwrap();
        assert_eq!(vim.text(), end, "wR<C-r>{modifier}a");
    }
}

#[test]
fn insert_register_name_through_a_macro() {
    let mut vim = vim();
    vim.feed("qqA <C-r>a<Esc>q").unwrap();
    assert_eq!(vim.text(), "one two one");
    assert_eq!(vim.registers().get('q').unwrap().text, "A \u{12}a\u{1b}");
    vim.feed("@q").unwrap();
    assert_eq!(vim.text(), "one two one one");
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), "one two one one one");
}

#[test]
fn insert_and_cmdline_maps_hide_no_builtin() {
    let remaps = [
        noremap(&[I, C], "g", "x"),
        noremap(&[I, C], "gs", "x"),
        noremap(&[I, C], "gS", "x"),
        noremap(&[I, C], "gsa", "x"),
        noremap(&[I, C], "gSa", "x"),
        noremap(&[I, C], "gU", "x"),
    ];
    let (set, issues) = RemapSet::new(&remaps, "\\");
    assert!(issues.is_empty(), "{issues:?}");
    assert!(
        set.hides_builtin(I).is_empty(),
        "{:?}",
        set.hides_builtin(I)
    );
    assert!(
        set.hides_builtin(C).is_empty(),
        "{:?}",
        set.hides_builtin(C)
    );
    // As they do in normal mode.
    let (set, _) = RemapSet::new(
        &[
            noremap(&[RemapMode::Normal], "gs", "x"),
            noremap(&[RemapMode::Normal], "gSa", "x"),
        ],
        "\\",
    );
    assert_eq!(set.hides_builtin(RemapMode::Normal), ["gs", "gSa"]);
}
